use std::path::Path;

use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use super::{database_error, open_workspace_connection};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tikhub::{
  get_tikhub_account_quota, quote_tikhub_connector_price, TikHubCollectionRequest,
  TikhubAccountQuota, TikhubPriceQuote,
};

pub(super) fn guard_request(
  root_path: &Path,
  run_id: &str,
  request: &TikHubCollectionRequest,
) -> AppResult<i64> {
  let quota = get_tikhub_account_quota(root_path)?;
  let quotes = request
    .paths()
    .iter()
    .map(|endpoint| quote_tikhub_connector_price(root_path, endpoint, 1))
    .collect::<AppResult<Vec<_>>>()?;
  persist_guarded_quotes(root_path, run_id, quota, quotes)
}

pub(super) fn checkpoint_quote_json(
  connection: &rusqlite::Connection,
  run_id: &str,
  request: &TikHubCollectionRequest,
) -> AppResult<String> {
  let expected_count =
    i64::try_from(request.paths().len()).map_err(|_| cost_error("计价 endpoint 数量超出范围"))?;
  let mut statement = connection
    .prepare(
      "SELECT id, quoted_cost_micros
       FROM pricing_quote_snapshot
       WHERE task_run_id = ?1
       ORDER BY rowid DESC LIMIT ?2",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_id, expected_count], |row| {
      Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })
    .map_err(database_error)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;
  if rows.len() != request.paths().len() {
    #[cfg(test)]
    return Ok(
      serde_json::json!({
        "currency": "USD",
        "amount_micros": 0,
        "billing_status": "unquoted_test_fixture"
      })
      .to_string(),
    );
    #[cfg(not(test))]
    return Err(cost_error("采集请求缺少对应的实时计价快照"));
  }
  let quoted_cost_micros = rows.iter().try_fold(0_i64, |total, (_, cost)| {
    total
      .checked_add(*cost)
      .ok_or_else(|| cost_error("检查点报价金额溢出"))
  })?;
  Ok(
    serde_json::json!({
      "currency": "USD",
      "amount_micros": quoted_cost_micros,
      "billing_status": "quoted_not_final",
      "pricing_snapshot_ids": rows.into_iter().map(|(id, _)| id).collect::<Vec<_>>()
    })
    .to_string(),
  )
}

fn persist_guarded_quotes(
  root_path: &Path,
  run_id: &str,
  quota: TikhubAccountQuota,
  quotes: Vec<TikhubPriceQuote>,
) -> AppResult<i64> {
  if quotes.is_empty() {
    return Err(cost_error("采集请求缺少可计价 endpoint"));
  }
  let quoted_micros = quotes
    .iter()
    .map(|quote| usd_to_micros(quote.total_price, "TikHub 实时报价"))
    .collect::<AppResult<Vec<_>>>()?;
  let request_quote = quoted_micros.iter().try_fold(0_i64, |total, value| {
    total
      .checked_add(*value)
      .ok_or_else(|| cost_error("TikHub 实时报价累计溢出"))
  })?;
  let balance_micros = usd_to_micros(quota.balance, "TikHub 充值余额")?;
  let free_credit_micros = usd_to_micros(quota.free_credit, "TikHub 免费额度")?;
  let available_micros = balance_micros
    .checked_add(free_credit_micros)
    .ok_or_else(|| cost_error("TikHub 可用额度累计溢出"))?;

  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let (budget_micros, accumulated_before) = load_budget_state(&transaction, run_id)?;
  let decision = evaluate_gate(
    budget_micros,
    accumulated_before,
    request_quote,
    available_micros,
  )?;
  let mut accumulated = accumulated_before;
  let now = Utc::now().to_rfc3339();
  for (quote, quoted_cost_micros) in quotes.into_iter().zip(quoted_micros) {
    accumulated = accumulated
      .checked_add(quoted_cost_micros)
      .ok_or_else(|| cost_error("累计报价溢出"))?;
    let quote_json = serde_json::json!({
      "billing_status": "quoted_not_final",
      "provider_quote": quote.quote_json,
      "base_unit_price": quote.base_unit_price,
      "request_per_day": quote.request_per_day
    });
    transaction
      .execute(
        "INSERT INTO pricing_quote_snapshot (
           id, task_run_id, endpoint_key, currency, quoted_cost_micros,
           accumulated_quote_micros, balance_micros, free_credit_micros,
           available_micros, quote_json, quoted_at
         ) VALUES (?1, ?2, ?3, 'USD', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
          Uuid::new_v4().to_string(),
          run_id,
          quote.endpoint,
          quoted_cost_micros,
          accumulated,
          balance_micros,
          free_credit_micros,
          available_micros,
          quote_json.to_string(),
          now
        ],
      )
      .map_err(database_error)?;
  }
  if accumulated != decision.accumulated_after {
    return Err(cost_error("报价账本累计结果不一致"));
  }
  transaction.commit().map_err(database_error)?;
  Ok(request_quote)
}

#[derive(Debug)]
struct GateDecision {
  accumulated_after: i64,
}

fn evaluate_gate(
  budget_micros: i64,
  accumulated_before: i64,
  request_quote: i64,
  available_micros: i64,
) -> AppResult<GateDecision> {
  let accumulated_after = accumulated_before
    .checked_add(request_quote)
    .ok_or_else(|| cost_error("累计报价溢出"))?;
  if accumulated_after > budget_micros {
    return Err(cost_error("TikHub 本次报价将超过任务预算，已停止请求"));
  }
  if request_quote > available_micros {
    return Err(cost_error("TikHub 免费额度与充值余额合计不足，已停止请求"));
  }
  Ok(GateDecision { accumulated_after })
}

fn load_budget_state(connection: &rusqlite::Connection, run_id: &str) -> AppResult<(i64, i64)> {
  let plan_json = connection
    .query_row(
      "SELECT plan.plan_json
       FROM task_run AS run
       JOIN collection_plan AS plan ON plan.id = run.plan_id
       WHERE run.id = ?1 AND run.status = 'running'",
      params![run_id],
      |row| row.get::<_, String>(0),
    )
    .map_err(database_error)?;
  let plan_json: Value =
    serde_json::from_str(&plan_json).map_err(|_| cost_error("采集计划预算不是合法 JSON"))?;
  let budget_micros = plan_json
    .pointer("/budget_limit/amount_micros")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0)
    .ok_or_else(|| cost_error("采集计划缺少有效预算上限"))?;
  let accumulated_before = connection
    .query_row(
      "SELECT accumulated_quote_micros FROM pricing_quote_snapshot
       WHERE task_run_id = ?1 ORDER BY quoted_at DESC, rowid DESC LIMIT 1",
      params![run_id],
      |row| row.get::<_, i64>(0),
    )
    .optional()
    .map_err(database_error)?
    .unwrap_or(0);
  Ok((budget_micros, accumulated_before))
}

fn usd_to_micros(value: f64, label: &str) -> AppResult<i64> {
  if !value.is_finite() || value < 0.0 || value > i64::MAX as f64 / 1_000_000.0 {
    return Err(cost_error(format!("{label}格式异常")));
  }
  Ok((value * 1_000_000.0).round() as i64)
}

fn cost_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::CostLimitError,
    message,
    AppErrorStage::Collection,
    false,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn gate_requires_both_budget_and_live_available_credit() {
    assert_eq!(
      evaluate_gate(1_000, 400, 500, 500)
        .expect("边界相等应允许")
        .accumulated_after,
      900
    );
    assert!(evaluate_gate(800, 400, 500, 5_000)
      .expect_err("累计报价超过预算必须阻止")
      .message
      .contains("预算"));
    assert!(evaluate_gate(5_000, 400, 501, 500)
      .expect_err("可用额度不足必须阻止")
      .message
      .contains("余额"));
  }

  #[test]
  fn usd_values_convert_to_exact_micros() {
    assert_eq!(usd_to_micros(0.05, "额度").expect("金额应转换"), 50_000);
    assert!(usd_to_micros(f64::NAN, "额度").is_err());
  }
}
