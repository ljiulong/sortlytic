use chrono::{DateTime, FixedOffset};
use sha2::{Digest, Sha256};

use crate::tikhub::{build_collection_request, parse_collection_page};

use super::*;

#[path = "terminal/failure.rs"]
mod failure;
#[path = "partial.rs"]
mod partial;

pub use failure::{cancel_task, fail_task_run};

struct CompletionStep {
  order: i64,
  platform: String,
  data_type: String,
  params_json: String,
  request_limit: i64,
  run_step_id: Option<String>,
  status: Option<String>,
  stop_reason: Option<String>,
  started_at: Option<String>,
  completed_at: Option<String>,
  schema_version: i64,
  step_key: String,
}

struct CompletionCheckpoint {
  page_index: i64,
  input_cursor_json: Option<String>,
  status: String,
  request_attempt_count: i64,
  provider_response_json: Option<String>,
  provider_response_hash: Option<String>,
  provider_response_size: Option<i64>,
  has_more: Option<bool>,
  next_cursor_json: Option<String>,
  record_count_received: i64,
  record_count_persisted: i64,
  cost_actual_json: String,
  last_error_code: Option<String>,
  last_error_message: Option<String>,
  retryable: bool,
  requested_at: Option<String>,
  response_received_at: Option<String>,
  committed_at: Option<String>,
}

struct CompletionLimits {
  record_limit: i64,
  budget_micros: i64,
  schema_version: i64,
}

struct CompletionTotals {
  request_count: i64,
  persisted_records: i64,
  cost_micros: i64,
  failure_count: i64,
  output_records: i64,
}

pub fn complete_task_run(
  root_path: impl AsRef<Path>,
  run_id: &str,
  _actual_cost_json: Value,
) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let run = get_task_run(&transaction, run_id)?;
  require_running(&run)?;
  let task = get_task_by_id(&transaction, &run.task_id)?;
  if task.status != "running" {
    return Err(task_error("任务已不在运行状态，不能标记成功"));
  }
  let now = Utc::now().to_rfc3339();
  let plan_id = run
    .plan_id
    .as_deref()
    .ok_or_else(|| task_error("运行记录缺少采集计划，不能标记成功"))?;
  require_executable_plan(&transaction, &run.task_id, plan_id)?;
  let totals = require_run_completion_evidence(&transaction, &run, plan_id, &now)?;

  let actual_cost_json = serde_json::json!({
    "currency": "USD",
    "billing_status": "quoted_not_final",
    "quoted_cost_micros": totals.cost_micros,
    "amount_micros": totals.cost_micros,
    "request_count": totals.request_count,
    "record_count": totals.persisted_records
  })
  .to_string();
  let (terminal_status, current_stage, error_code, error_message) =
    if totals.failure_count > 0 && totals.output_records == 0 {
      (
        "failed",
        "执行失败",
        Some("ALL_TARGETS_FAILED"),
        Some("全部采集目标失败，未获得合格账号"),
      )
    } else if totals.failure_count > 0 {
      ("partial_success", "部分成功", None, None)
    } else {
      ("success", "已完成", None, None)
    };
  let run_changed = transaction
    .execute(
      "UPDATE task_run
       SET status = ?1, ended_at = ?2, current_stage = ?3,
           cost_actual_json = ?4, error_code = ?5, error_message = ?6, retryable = 0
       WHERE id = ?7 AND task_id = ?8 AND plan_id = ?9 AND status = 'running'",
      params![
        terminal_status,
        now,
        current_stage,
        actual_cost_json,
        error_code,
        error_message,
        run_id,
        run.task_id,
        plan_id
      ],
    )
    .map_err(database_error)?;
  if run_changed != 1 {
    return Err(task_error("运行记录状态已变化，无法原子标记成功"));
  }
  let task_changed = transaction
    .execute(
      "UPDATE collection_task
       SET status = ?1, completed_at = ?2, actual_cost_json = ?3, updated_at = ?2
       WHERE id = ?4 AND status = 'running'",
      params![terminal_status, now, actual_cost_json, run.task_id],
    )
    .map_err(database_error)?;
  if task_changed != 1 {
    return Err(task_error("父任务状态已变化，无法原子标记成功"));
  }
  partial::append_completion_log(&transaction, run_id, terminal_status, &now)?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, run_id)
}

fn require_run_completion_evidence(
  connection: &Connection,
  run: &TaskRunView,
  plan_id: &str,
  completion_at: &str,
) -> AppResult<CompletionTotals> {
  let (Some(run_started_at), Some(claimed_at), Some(completion_at)) = (
    valid_timestamp(Some(&run.started_at)),
    valid_timestamp(run.claimed_at.as_deref()),
    valid_timestamp(Some(completion_at)),
  ) else {
    return Err(task_error("运行记录缺少合法的创建、领取或完成时间"));
  };
  if run_started_at > claimed_at || claimed_at > completion_at {
    return Err(task_error("运行记录的创建、领取与完成时间顺序不一致"));
  }
  let limits = load_completion_limits(connection, plan_id)?;
  let steps = load_completion_steps(connection, &run.id, plan_id)?;
  if steps.is_empty() {
    return Err(task_error("运行记录没有计划步骤，不能标记成功"));
  }
  let total_run_steps = connection
    .query_row(
      "SELECT COUNT(*) FROM task_run_step WHERE task_run_id = ?1",
      params![run.id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  if i64::try_from(steps.len()).ok() != Some(total_run_steps) {
    return Err(task_error("运行步骤快照与采集计划不完整对应，不能标记成功"));
  }

  let mut request_count = 0_i64;
  let mut persisted_records = 0_i64;
  let mut cost_micros = 0_i64;
  let mut failure_count = 0_i64;
  for step in &steps {
    let run_step_id = step
      .run_step_id
      .as_deref()
      .ok_or_else(|| task_error("运行步骤快照缺少计划步骤，不能标记成功"))?;
    let valid_stop_reason = step.stop_reason.is_none()
      || (step.schema_version >= 3
        && step.stop_reason.as_deref().is_some_and(|reason| {
          matches!(
            reason,
            "provider_exhausted" | "request_limit" | "record_limit" | "budget_limit"
          )
        }));
    if step.status.as_deref() != Some("success")
      || !valid_stop_reason
      || !valid_step_time_range(step, run_started_at, completion_at)
    {
      return Err(task_error(format!(
        "运行步骤 {} 尚未形成无冲突的成功终态",
        step.order + 1
      )));
    }
    let checkpoints = load_completion_checkpoints(connection, run_step_id)?;
    let Some(totals) = completion_chain_totals(step, &checkpoints) else {
      return Err(task_error(format!(
        "运行步骤 {} 的检查点完成证据不完整或不一致",
        step.order + 1
      )));
    };
    if step.schema_version == 2 && totals.request_count > step.request_limit {
      return Err(task_error(format!(
        "运行步骤 {} 的请求次数超过确认上限",
        step.order + 1
      )));
    }
    if step.schema_version >= 3
      && (!pipeline_requests_match_targets(connection, &run.id, step, totals.request_count)?
        || !partial::failure_evidence_matches(
          connection,
          &run.id,
          run_step_id,
          &step.step_key,
          totals.failure_count,
        )?)
    {
      return Err(task_error(format!(
        "运行步骤 {} 的目标请求证据与检查点不一致",
        step.order + 1
      )));
    }
    request_count = request_count
      .checked_add(totals.request_count)
      .ok_or_else(|| task_error("完成证据的请求次数溢出"))?;
    persisted_records = persisted_records
      .checked_add(totals.persisted_records)
      .ok_or_else(|| task_error("完成证据的持久化记录数溢出"))?;
    cost_micros = cost_micros
      .checked_add(totals.cost_micros)
      .ok_or_else(|| task_error("完成证据的成本金额溢出"))?;
    failure_count = failure_count
      .checked_add(totals.failure_count)
      .ok_or_else(|| task_error("失败目标数量溢出"))?;
  }
  if limits.schema_version == 2 && persisted_records > limits.record_limit {
    return Err(task_error("完成证据的持久化记录数超过确认上限"));
  }
  let output_records = if limits.schema_version >= 3 {
    let output_count = connection
      .query_row(
        "SELECT COUNT(*) FROM collected_account
         WHERE task_run_id = ?1 AND output_included = 1",
        params![run.id],
        |row| row.get::<_, i64>(0),
      )
      .map_err(database_error)?;
    if output_count > limits.record_limit {
      return Err(task_error("合并后的输出账号数超过确认上限"));
    }
    output_count
  } else {
    persisted_records
  };
  if cost_micros > limits.budget_micros {
    return Err(task_error("完成证据的实际成本超过确认预算"));
  }
  Ok(CompletionTotals {
    request_count,
    persisted_records,
    cost_micros,
    failure_count,
    output_records,
  })
}

fn load_completion_limits(connection: &Connection, plan_id: &str) -> AppResult<CompletionLimits> {
  let plan_json = connection
    .query_row(
      "SELECT schema_version, plan_json FROM collection_plan WHERE id = ?1",
      params![plan_id],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )
    .map_err(database_error)?;
  let plan_json = serde_json::from_str::<Value>(&plan_json.1)
    .map_err(|_| task_error("采集计划限制不是合法 JSON"))?;
  let record_limit = plan_json
    .get("record_limit")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0)
    .ok_or_else(|| task_error("采集计划缺少有效记录上限"))?;
  let budget = plan_json
    .get("budget_limit")
    .and_then(Value::as_object)
    .ok_or_else(|| task_error("采集计划缺少有效预算上限"))?;
  if budget.get("currency").and_then(Value::as_str) != Some("USD") {
    return Err(task_error("采集计划预算币种必须为 USD"));
  }
  let budget_micros = budget
    .get("amount_micros")
    .and_then(Value::as_i64)
    .filter(|value| *value >= 0)
    .ok_or_else(|| task_error("采集计划缺少有效微美元预算"))?;
  Ok(CompletionLimits {
    record_limit,
    budget_micros,
    schema_version: plan_json
      .get("schema_version")
      .and_then(Value::as_i64)
      .unwrap_or(2),
  })
}

fn load_completion_steps(
  connection: &Connection,
  run_id: &str,
  plan_id: &str,
) -> AppResult<Vec<CompletionStep>> {
  let mut statement = connection
    .prepare(
      "SELECT api_step.step_order, api_step.platform, api_step.data_type, api_step.params_json,
              api_step.request_count_estimate, run_step.id, run_step.status, run_step.stop_reason,
              run_step.started_at, run_step.completed_at, plan.schema_version,
              COALESCE(
                json_extract(plan.plan_json, '$.steps[' || api_step.step_order || '].step_key'),
                api_step.data_type
              )
       FROM api_call_step AS api_step
       JOIN collection_plan AS plan ON plan.id = api_step.plan_id
       LEFT JOIN task_run_step AS run_step
         ON run_step.api_call_step_id = api_step.id AND run_step.task_run_id = ?1
       WHERE api_step.plan_id = ?2
       ORDER BY api_step.step_order, api_step.id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_id, plan_id], |row| {
      Ok(CompletionStep {
        order: row.get(0)?,
        platform: row.get(1)?,
        data_type: row.get(2)?,
        params_json: row.get(3)?,
        request_limit: row.get(4)?,
        run_step_id: row.get(5)?,
        status: row.get(6)?,
        stop_reason: row.get(7)?,
        started_at: row.get(8)?,
        completed_at: row.get(9)?,
        schema_version: row.get(10)?,
        step_key: row.get(11)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn load_completion_checkpoints(
  connection: &Connection,
  run_step_id: &str,
) -> AppResult<Vec<CompletionCheckpoint>> {
  let mut statement = connection
    .prepare(
      "SELECT page_index, input_cursor_json, status, request_attempt_count,
              provider_response_json, provider_response_hash, provider_response_size,
              has_more, next_cursor_json, record_count_received, record_count_persisted,
              cost_actual_json, last_error_code, last_error_message, retryable,
              requested_at, response_received_at, committed_at
       FROM collection_page_checkpoint
       WHERE task_run_step_id = ?1
       ORDER BY page_index, id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_step_id], |row| {
      Ok(CompletionCheckpoint {
        page_index: row.get(0)?,
        input_cursor_json: row.get(1)?,
        status: row.get(2)?,
        request_attempt_count: row.get(3)?,
        provider_response_json: row.get(4)?,
        provider_response_hash: row.get(5)?,
        provider_response_size: row.get(6)?,
        has_more: row.get::<_, Option<i64>>(7)?.map(|value| value != 0),
        next_cursor_json: row.get(8)?,
        record_count_received: row.get(9)?,
        record_count_persisted: row.get(10)?,
        cost_actual_json: row.get(11)?,
        last_error_code: row.get(12)?,
        last_error_message: row.get(13)?,
        retryable: row.get::<_, i64>(14)? != 0,
        requested_at: row.get(15)?,
        response_received_at: row.get(16)?,
        committed_at: row.get(17)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn completion_chain_totals(
  step: &CompletionStep,
  checkpoints: &[CompletionCheckpoint],
) -> Option<CompletionTotals> {
  if checkpoints.is_empty() {
    return (step.schema_version >= 3 && step.stop_reason.as_deref() == Some("provider_exhausted"))
      .then_some(CompletionTotals {
        request_count: 0,
        persisted_records: 0,
        cost_micros: 0,
        failure_count: 0,
        output_records: 0,
      });
  }
  let (Some(step_started_at), Some(step_completed_at)) = (
    valid_timestamp(step.started_at.as_deref()),
    valid_timestamp(step.completed_at.as_deref()),
  ) else {
    return None;
  };
  let mut request_count = 0_i64;
  let mut persisted_records = 0_i64;
  let mut cost_micros = 0_i64;
  let mut failure_count = 0_i64;
  let mut previous: Option<&CompletionCheckpoint> = None;
  let mut previous_committed_at: Option<DateTime<FixedOffset>> = None;
  for (index, checkpoint) in checkpoints.iter().enumerate() {
    let is_target_failure = step.schema_version >= 3 && checkpoint.status == "failed";
    let has_valid_evidence = if is_target_failure {
      partial::failed_checkpoint_is_complete(checkpoint)
    } else {
      checkpoint.status == "completed"
        && !checkpoint.retryable
        && checkpoint.last_error_code.is_none()
        && checkpoint.last_error_message.is_none()
        && checkpoint_evidence_is_complete(step, checkpoint)
    };
    if checkpoint.page_index != index as i64 || !has_valid_evidence {
      return None;
    }
    if step.schema_version == 2 && index == 0 && checkpoint.input_cursor_json.is_some() {
      return None;
    }
    if step.schema_version == 2 {
      if let Some(previous) = previous {
        let (Some(previous_next_cursor), Some(input_cursor)) = (
          parsed_optional_json(previous.next_cursor_json.as_deref()),
          parsed_optional_json(checkpoint.input_cursor_json.as_deref()),
        ) else {
          return None;
        };
        if previous.has_more != Some(true) || previous_next_cursor != input_cursor {
          return None;
        }
      }
    }
    let is_last = index + 1 == checkpoints.len();
    if step.schema_version == 2
      && ((!is_last && checkpoint.has_more != Some(true))
        || (is_last && checkpoint.has_more != Some(false)))
    {
      return None;
    }
    let (Some(requested_at), Some(committed_at)) = (
      valid_timestamp(checkpoint.requested_at.as_deref()),
      valid_timestamp(checkpoint.committed_at.as_deref()),
    ) else {
      return None;
    };
    if (index == 0 && requested_at < step_started_at)
      || previous_committed_at.is_some_and(|previous| previous > requested_at)
      || committed_at > step_completed_at
    {
      return None;
    }
    request_count = request_count.checked_add(checkpoint.request_attempt_count)?;
    persisted_records = persisted_records.checked_add(checkpoint.record_count_persisted)?;
    cost_micros = cost_micros.checked_add(checkpoint_cost_micros(&checkpoint.cost_actual_json)?)?;
    failure_count = failure_count.checked_add(i64::from(is_target_failure))?;
    previous_committed_at = Some(committed_at);
    previous = Some(checkpoint);
  }
  Some(CompletionTotals {
    request_count,
    persisted_records,
    cost_micros,
    failure_count,
    output_records: 0,
  })
}

fn pipeline_requests_match_targets(
  connection: &Connection,
  run_id: &str,
  step: &CompletionStep,
  checkpoint_requests: i64,
) -> AppResult<bool> {
  let evidence = connection
    .query_row(
      "SELECT COALESCE(SUM(request_count), 0),
              COALESCE(SUM(CASE
                WHEN request_count > ?1 OR status IN ('pending', 'running') THEN 1 ELSE 0
              END), 0)
       FROM collection_pipeline_target
       WHERE task_run_id = ?2 AND step_key = ?3",
      params![step.request_limit, run_id, step.step_key],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )
    .map_err(database_error)?;
  Ok(evidence.0 == checkpoint_requests && evidence.1 == 0)
}

fn checkpoint_evidence_is_complete(
  step: &CompletionStep,
  checkpoint: &CompletionCheckpoint,
) -> bool {
  let (Some(response), Some(expected_hash), Some(expected_size)) = (
    checkpoint.provider_response_json.as_deref(),
    checkpoint.provider_response_hash.as_deref(),
    checkpoint.provider_response_size,
  ) else {
    return false;
  };
  let (Some(requested_at), Some(response_received_at), Some(committed_at), Some(has_more)) = (
    valid_timestamp(checkpoint.requested_at.as_deref()),
    valid_timestamp(checkpoint.response_received_at.as_deref()),
    valid_timestamp(checkpoint.committed_at.as_deref()),
    checkpoint.has_more,
  ) else {
    return false;
  };
  let response_hash = format!("{:x}", Sha256::digest(response.as_bytes()));
  let response_size = i64::try_from(response.len()).ok();
  let Some(response_json) = serde_json::from_str::<Value>(response).ok() else {
    return false;
  };
  let Some(params_json) = serde_json::from_str::<Value>(&step.params_json).ok() else {
    return false;
  };
  let Some(input_cursor) = parsed_optional_json(checkpoint.input_cursor_json.as_deref()) else {
    return false;
  };
  let Ok(request) = build_collection_request(
    &step.platform,
    &step.data_type,
    &params_json,
    input_cursor.as_ref(),
  ) else {
    return false;
  };
  let Ok(page) = parse_collection_page(&request, response_json) else {
    return false;
  };
  let Some(next_cursor) = parsed_optional_json(checkpoint.next_cursor_json.as_deref()) else {
    return false;
  };
  checkpoint.request_attempt_count > 0
    && requested_at <= response_received_at
    && response_received_at <= committed_at
    && response_size == Some(expected_size)
    && response_hash == expected_hash
    && page.has_more == has_more
    && page.next_cursor == next_cursor
    && i64::try_from(page.records.len()).ok() == Some(checkpoint.record_count_received)
    && checkpoint.record_count_persisted == checkpoint.record_count_received
    && checkpoint_cost_micros(&checkpoint.cost_actual_json).is_some()
}

fn valid_step_time_range(
  step: &CompletionStep,
  run_started_at: DateTime<FixedOffset>,
  completion_at: DateTime<FixedOffset>,
) -> bool {
  let (Some(started_at), Some(completed_at)) = (
    valid_timestamp(step.started_at.as_deref()),
    valid_timestamp(step.completed_at.as_deref()),
  ) else {
    return false;
  };
  run_started_at <= started_at && started_at <= completed_at && completed_at <= completion_at
}

fn valid_timestamp(value: Option<&str>) -> Option<DateTime<FixedOffset>> {
  DateTime::parse_from_rfc3339(value?.trim()).ok()
}

fn parsed_optional_json(value: Option<&str>) -> Option<Option<Value>> {
  match value {
    Some(value) => serde_json::from_str(value).ok().map(Some),
    None => Some(None),
  }
}

fn checkpoint_cost_micros(value: &str) -> Option<i64> {
  let cost = serde_json::from_str::<Value>(value).ok()?;
  if cost.get("currency").and_then(Value::as_str) != Some("USD") {
    return None;
  }
  cost
    .get("amount_micros")
    .and_then(Value::as_i64)
    .filter(|amount| *amount >= 0)
}

fn require_running(run: &TaskRunView) -> AppResult<()> {
  if run.status == "running" {
    Ok(())
  } else {
    Err(task_error("只有运行中的任务记录可以结束"))
  }
}
