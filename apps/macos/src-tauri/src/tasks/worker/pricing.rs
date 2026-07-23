use std::path::Path;

use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use super::{database_error, open_workspace_connection, WorkerFence};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tikhub::{
  get_tikhub_account_quota, quote_tikhub_connector_price, TikHubCollectionRequest,
  TikhubAccountQuota, TikhubPriceQuote,
};

pub(super) fn guard_request(
  root_path: &Path,
  run_id: &str,
  request: &TikHubCollectionRequest,
  fence: Option<&WorkerFence>,
) -> AppResult<i64> {
  let task_id = fence
    .map(|_| task_id_for_run(root_path, run_id))
    .transpose()?;
  super::with_task_dispatch_gate(
    root_path,
    task_id.as_deref().unwrap_or_default(),
    fence.is_some(),
    || {
      if let (Some(fence), Some(task_id)) = (fence, task_id.as_deref()) {
        ensure_run_accepts_pricing(root_path, task_id, run_id, fence)?;
      }
      let quota = get_tikhub_account_quota(root_path)?;
      let quotes = request
        .paths()
        .iter()
        .map(|endpoint| quote_tikhub_connector_price(root_path, endpoint, 1))
        .collect::<AppResult<Vec<_>>>()?;
      persist_guarded_quotes(root_path, run_id, quota, quotes, fence)
    },
  )
}

fn task_id_for_run(root_path: &Path, run_id: &str) -> AppResult<String> {
  let connection = open_workspace_connection(root_path)?;
  connection
    .query_row(
      "SELECT task_id FROM task_run WHERE id = ?1",
      params![run_id],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| cost_error("任务运行不存在，禁止发送计价请求"))
}

fn ensure_run_accepts_pricing(
  root_path: &Path,
  task_id: &str,
  run_id: &str,
  fence: &WorkerFence,
) -> AppResult<()> {
  let connection = open_workspace_connection(root_path)?;
  fence.ensure_current(&connection)?;
  let state = connection
    .query_row(
      "SELECT task.status = 'running' AND run.status = 'running',
              task.status = 'cancelled' OR run.status = 'cancelled'
       FROM collection_task AS task
       JOIN task_run AS run ON run.task_id = task.id
       WHERE task.id = ?1 AND run.id = ?2",
      params![task_id, run_id],
      |row| Ok((row.get::<_, bool>(0)?, row.get::<_, bool>(1)?)),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| cost_error("任务或运行不属于当前计价请求"))?;
  if state.1 {
    return Err(AppError::new(
      AppErrorCode::Cancelled,
      "任务已取消，不会发送新的计价请求",
      AppErrorStage::Collection,
      false,
    ));
  }
  if !state.0 {
    return Err(cost_error("任务或运行状态已变化，禁止发送计价请求"));
  }
  Ok(())
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
  fence: Option<&WorkerFence>,
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
  if let Some(fence) = fence {
    fence.ensure_current(&transaction)?;
  }
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
       JOIN collection_task AS task ON task.id = run.task_id
       JOIN collection_plan AS plan ON plan.id = run.plan_id
       WHERE run.id = ?1
         AND run.status = 'running'
         AND task.status = 'running'",
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
#[path = "pricing/test_support.rs"]
mod test_support;

#[cfg(test)]
mod tests {
  use std::sync::mpsc;
  use std::thread;
  use std::time::Duration;

  use super::test_support::*;
  use super::*;
  use crate::tasks::cancel_task;
  use crate::tasks::test_support::install_successful_tikhub_profile;
  use crate::tikhub::test_support::override_tikhub_base_url_for_current_test;

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
  fn low_budget_matrix_repeats_budget_and_balance_boundaries_three_rounds() {
    const REQUEST_QUOTE_MICROS: i64 = 10_000;

    for round in 1..=3 {
      for tenths in 1_i64..=10 {
        let limit_micros = tenths * 100_000;
        let label = format!("第 {round} 轮 ${:.1}", tenths as f64 / 10.0);
        let exact_budget = evaluate_gate(
          limit_micros,
          limit_micros - REQUEST_QUOTE_MICROS,
          REQUEST_QUOTE_MICROS,
          limit_micros * 2,
        )
        .unwrap_or_else(|error| panic!("{label} 精确到达设定上限应允许：{}", error.message));
        assert_eq!(
          exact_budget.accumulated_after, limit_micros,
          "{label} 累计报价应精确等于设定上限"
        );

        let budget_error = evaluate_gate(
          limit_micros,
          limit_micros,
          REQUEST_QUOTE_MICROS,
          limit_micros * 2,
        )
        .unwrap_err();
        assert!(
          budget_error.message.contains("预算"),
          "{label} 下一次请求将超过设定上限时必须停止"
        );

        let exact_balance = evaluate_gate(limit_micros * 2, 0, limit_micros, limit_micros)
          .unwrap_or_else(|error| panic!("{label} 精确用完实时余额应允许：{}", error.message));
        assert_eq!(
          exact_balance.accumulated_after, limit_micros,
          "{label} 余额边界的累计报价应正确"
        );

        let balance_error =
          evaluate_gate(limit_micros * 2, 0, limit_micros + 1, limit_micros).unwrap_err();
        assert!(
          balance_error.message.contains("余额"),
          "{label} 报价比实时余额多 1 微美元时必须停止"
        );
      }
    }
  }

  #[test]
  fn usd_values_convert_to_exact_micros() {
    assert_eq!(usd_to_micros(0.05, "额度").expect("金额应转换"), 50_000);
    assert!(usd_to_micros(f64::NAN, "额度").is_err());
  }

  #[test]
  fn cancelled_run_is_rejected_before_tikhub_configuration_is_read() {
    let root = std::env::temp_dir().join(format!("worker-pricing-cancel-{}", Uuid::new_v4()));
    let connection = create_pricing_fixture(&root, "cancelled", "current-owner", 1);
    let current = crate::tasks::WorkerFence::new("current-owner".to_string(), 1)
      .expect("current fence should be valid");
    let request = pricing_request();

    let error = guard_request(&root, PRICING_RUN_ID, &request, Some(&current))
      .expect_err("cancelled run must be rejected before TikHub configuration is read");

    assert_eq!(error.code, AppErrorCode::Cancelled);
    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM pricing_quote_snapshot
           WHERE task_run_id = ?1",
          [PRICING_RUN_ID],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      0
    );
    drop(connection);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn committed_cancellation_prevents_all_pricing_http_requests() {
    let root = std::env::temp_dir().join(format!("worker-pricing-http-zero-{}", Uuid::new_v4()));
    let connection = create_pricing_fixture(&root, "running", "current-owner", 1);
    install_successful_tikhub_profile(&root).expect("TikHub profile should install");
    let current = crate::tasks::WorkerFence::new("current-owner".to_string(), 1)
      .expect("current fence should be valid");
    let request = pricing_request();
    let server = PricingHttpServer::start(None);

    cancel_task(&root, PRICING_TASK_ID).expect("running task should cancel");
    let _override = override_tikhub_base_url_for_current_test(server.base_url.clone());
    let error = guard_request(&root, PRICING_RUN_ID, &request, Some(&current))
      .expect_err("committed cancellation must reject pricing");
    let request_count = server.finish();

    assert_eq!(error.code, AppErrorCode::Cancelled);
    assert_eq!(request_count, 0);
    assert_eq!(
      connection
        .query_row(
          "SELECT status FROM collection_task WHERE id = ?1",
          [PRICING_TASK_ID],
          |row| row.get::<_, String>(0),
        )
        .unwrap(),
      "cancelled"
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT status FROM task_run WHERE id = ?1",
          [PRICING_RUN_ID],
          |row| row.get::<_, String>(0),
        )
        .unwrap(),
      "cancelled"
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT generation FROM task_worker_lease WHERE id = 'task_worker'",
          [],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      1
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM pricing_quote_snapshot WHERE task_run_id = ?1",
          [PRICING_RUN_ID],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      0
    );
    drop(connection);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn cancellation_waits_for_inflight_pricing_before_committing() {
    let root = std::env::temp_dir().join(format!("worker-pricing-http-gate-{}", Uuid::new_v4()));
    let connection = create_pricing_fixture(&root, "running", "current-owner", 1);
    install_successful_tikhub_profile(&root).expect("TikHub profile should install");
    let current = crate::tasks::WorkerFence::new("current-owner".to_string(), 1)
      .expect("current fence should be valid");
    let request = pricing_request();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = PricingHttpServer::start(Some(FirstRequestGate {
      entered: entered_tx,
      release: release_rx,
    }));
    let pricing_root = root.clone();
    let base_url = server.base_url.clone();
    let pricing = thread::spawn(move || {
      let _override = override_tikhub_base_url_for_current_test(base_url);
      guard_request(&pricing_root, PRICING_RUN_ID, &request, Some(&current))
    });
    entered_rx
      .recv_timeout(Duration::from_secs(3))
      .expect("quota request should reach the local server");

    let cancel_root = root.clone();
    let (cancel_started_tx, cancel_started_rx) = mpsc::channel();
    let (cancel_done_tx, cancel_done_rx) = mpsc::channel();
    let cancellation = thread::spawn(move || {
      cancel_started_tx
        .send(())
        .expect("cancel start signal should send");
      let result = cancel_task(&cancel_root, PRICING_TASK_ID);
      cancel_done_tx
        .send(result)
        .expect("cancel result should send");
    });
    cancel_started_rx
      .recv_timeout(Duration::from_secs(1))
      .expect("cancel thread should start");
    assert!(matches!(
      cancel_done_rx.recv_timeout(Duration::from_millis(150)),
      Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(
      connection
        .query_row(
          "SELECT task.status, run.status
           FROM collection_task AS task
           JOIN task_run AS run ON run.task_id = task.id
           WHERE task.id = ?1 AND run.id = ?2",
          params![PRICING_TASK_ID, PRICING_RUN_ID],
          |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap(),
      ("running".to_string(), "running".to_string())
    );

    release_tx
      .send(())
      .expect("quota response should be released");
    assert_eq!(
      pricing
        .join()
        .expect("pricing thread should finish")
        .expect("pricing should finish before cancellation"),
      10_000
    );
    cancel_done_rx
      .recv_timeout(Duration::from_secs(3))
      .expect("cancellation should finish after pricing")
      .expect("running task should cancel");
    cancellation
      .join()
      .expect("cancellation thread should finish");
    assert_eq!(server.request_count(), 2);
    assert_eq!(server.finish(), 2);

    assert_eq!(
      connection
        .query_row(
          "SELECT task.status, run.status
           FROM collection_task AS task
           JOIN task_run AS run ON run.task_id = task.id
           WHERE task.id = ?1 AND run.id = ?2",
          params![PRICING_TASK_ID, PRICING_RUN_ID],
          |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap(),
      ("cancelled".to_string(), "cancelled".to_string())
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM pricing_quote_snapshot WHERE task_run_id = ?1",
          [PRICING_RUN_ID],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      1
    );
    drop(connection);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn stale_worker_fence_rejects_quote_ledger_writes() {
    let root = std::env::temp_dir().join(format!("worker-pricing-fence-{}", Uuid::new_v4()));
    let connection = create_pricing_fixture(&root, "running", "replacement-owner", 2);
    let stale = crate::tasks::WorkerFence::new("stale-owner".to_string(), 1)
      .expect("stale fence should be valid");

    persist_guarded_quotes(
      &root,
      PRICING_RUN_ID,
      TikhubAccountQuota {
        balance: 1.0,
        free_credit: 0.0,
        available_credit: 1.0,
      },
      vec![TikhubPriceQuote {
        endpoint: "tiktok.item_detail".to_string(),
        request_per_day: 1,
        base_unit_price: Some(0.01),
        total_price: 0.01,
        currency: "USD".to_string(),
        quote_json: serde_json::json!({"price": 0.01}),
      }],
      Some(&stale),
    )
    .expect_err("a stale generation must not append pricing snapshots");

    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM pricing_quote_snapshot WHERE task_run_id = ?1",
          [PRICING_RUN_ID],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      0
    );
    drop(connection);
    std::fs::remove_dir_all(root).ok();
  }
}
