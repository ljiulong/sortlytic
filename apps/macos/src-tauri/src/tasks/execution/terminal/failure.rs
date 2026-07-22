use std::collections::BTreeMap;

use super::*;

pub fn fail_task_run(
  root_path: impl AsRef<Path>,
  run_id: &str,
  error_code: &str,
  error_message: &str,
  retryable: bool,
) -> AppResult<TaskRunView> {
  fail_task_run_with_safe_details(
    root_path,
    run_id,
    error_code,
    error_message,
    retryable,
    &BTreeMap::new(),
  )
}

pub fn fail_task_run_with_safe_details(
  root_path: impl AsRef<Path>,
  run_id: &str,
  error_code: &str,
  error_message: &str,
  retryable: bool,
  safe_details: &BTreeMap<String, String>,
) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let run = get_task_run(&transaction, run_id)?;
  require_running(&run)?;
  let code = normalize_required("错误代码", error_code)?;
  let message = redact_sensitive_text(&normalize_required("错误信息", error_message)?);
  let now = Utc::now().to_rfc3339();
  let has_request_evidence = run_has_request_evidence(&transaction, run_id)?;
  let output_records = output_record_count(&transaction, run_id)?;
  let budget_stopped_with_results = code == "COST_LIMIT_ERROR" && output_records > 0;
  let terminal_status = if budget_stopped_with_results {
    "partial_success"
  } else {
    "failed"
  };
  let current_stage = if budget_stopped_with_results {
    "部分成功"
  } else {
    "执行失败"
  };

  settle_active_children(
    &transaction,
    run_id,
    if budget_stopped_with_results {
      "success"
    } else {
      "failed"
    },
    if budget_stopped_with_results {
      "budget_limit"
    } else {
      "terminal_error"
    },
    if budget_stopped_with_results {
      "UNCERTAIN_REQUEST_AT_BUDGET_STOP"
    } else {
      "UNCERTAIN_REQUEST_AFTER_FAILURE"
    },
    if budget_stopped_with_results {
      "BUDGET_LIMIT_REACHED"
    } else {
      "CHECKPOINT_TERMINAL_FAILURE"
    },
    &message,
    &now,
  )?;
  let terminal_cost_json = budget_stopped_with_results
    .then(|| settled_cost_summary(&transaction, run_id))
    .transpose()?;

  let run_changed = transaction
    .execute(
      "UPDATE task_run
       SET status = ?1, ended_at = ?2, current_stage = ?3, error_code = ?4,
           error_message = ?5, retryable = ?6,
           cost_actual_json = COALESCE(?7, cost_actual_json)
       WHERE id = ?8 AND status = 'running'",
      params![
        terminal_status,
        now,
        current_stage,
        code,
        message,
        i64::from(!budget_stopped_with_results && retryable && !has_request_evidence),
        terminal_cost_json.as_deref(),
        run_id
      ],
    )
    .map_err(database_error)?;
  if run_changed != 1 {
    return Err(task_error("运行记录状态已变化，无法标记终态"));
  }
  let task_changed = transaction
    .execute(
      "UPDATE collection_task
       SET status = ?1,
           completed_at = CASE WHEN ?1 = 'partial_success' THEN ?2 ELSE completed_at END,
           actual_cost_json = COALESCE(?3, actual_cost_json), updated_at = ?2
       WHERE id = ?4 AND status = 'running'",
      params![
        terminal_status,
        now,
        terminal_cost_json.as_deref(),
        run.task_id
      ],
    )
    .map_err(database_error)?;
  if task_changed != 1 {
    return Err(task_error("父任务状态已变化，无法标记终态"));
  }
  append_terminal_log(
    &transaction,
    run_id,
    current_stage,
    if budget_stopped_with_results {
      "warning"
    } else {
      "error"
    },
    &message,
    safe_details,
    &now,
  )?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, run_id)
}

fn append_terminal_log(
  connection: &Connection,
  run_id: &str,
  stage: &str,
  level: &str,
  message: &str,
  safe_details: &BTreeMap<String, String>,
  created_at: &str,
) -> AppResult<()> {
  let safe_details = safe_details
    .iter()
    .filter(|(key, _)| !is_sensitive_detail_key(key))
    .map(|(key, value)| (key, redact_sensitive_text(value)))
    .collect::<BTreeMap<_, _>>();
  connection
    .execute(
      "INSERT INTO task_log (id, task_run_id, stage, level, message, safe_details_json, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      params![
        Uuid::new_v4().to_string(),
        run_id,
        stage,
        level,
        message,
        serde_json::to_string(&safe_details).unwrap_or_else(|_| "{}".to_string()),
        created_at
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn is_sensitive_detail_key(key: &str) -> bool {
  let key = key.to_ascii_lowercase();
  [
    "token",
    "secret",
    "authorization",
    "password",
    "credential",
    "api_key",
    "api-key",
  ]
  .iter()
  .any(|needle| key.contains(needle))
}

pub fn cancel_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<CollectionTaskView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let task = get_task_by_id(&transaction, task_id)?;
  if matches!(
    task.status.as_str(),
    "success" | "partial_success" | "failed" | "cancelled"
  ) {
    return Err(task_error("终态任务不能取消"));
  }
  let active_runs = active_run_ids(&transaction, task_id)?;
  let now = Utc::now().to_rfc3339();
  let task_changed = transaction
    .execute(
      "UPDATE collection_task SET status = 'cancelled', cancelled_at = ?1, updated_at = ?1
       WHERE id = ?2 AND status IN ('draft', 'waiting_confirmation', 'queued', 'running')",
      params![now, task_id],
    )
    .map_err(database_error)?;
  if task_changed != 1 {
    return Err(task_error("任务状态已变化，无法取消"));
  }
  transaction
    .execute(
      "UPDATE task_run SET status = 'cancelled', ended_at = ?1, current_stage = '用户取消'
       WHERE task_id = ?2 AND status IN ('queued', 'running')",
      params![now, task_id],
    )
    .map_err(database_error)?;
  for run_id in active_runs {
    settle_active_children(
      &transaction,
      &run_id,
      "cancelled",
      "user_cancelled",
      "UNCERTAIN_REQUEST_AFTER_CANCEL",
      "RUN_CANCELLED",
      "任务已由用户取消",
      &now,
    )?;
    append_task_log(
      &transaction,
      &run_id,
      "用户取消",
      "warning",
      "任务已由用户取消",
    )?;
  }
  transaction.commit().map_err(database_error)?;
  get_task_by_id(&connection, task_id)
}

fn output_record_count(connection: &Connection, run_id: &str) -> AppResult<i64> {
  connection
    .query_row(
      "SELECT COUNT(*) FROM collected_account
       WHERE task_run_id = ?1 AND output_included = 1",
      params![run_id],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn settled_cost_summary(connection: &Connection, run_id: &str) -> AppResult<String> {
  let mut statement = connection
    .prepare(
      "SELECT checkpoint.cost_actual_json, checkpoint.request_attempt_count,
              checkpoint.record_count_persisted
       FROM collection_page_checkpoint AS checkpoint
       JOIN task_run_step AS run_step ON run_step.id = checkpoint.task_run_step_id
       WHERE run_step.task_run_id = ?1 AND checkpoint.status IN ('completed', 'failed')
       ORDER BY run_step.id, checkpoint.page_index, checkpoint.id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_id], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
      ))
    })
    .map_err(database_error)?;
  let mut cost_micros = 0_i64;
  let mut request_count = 0_i64;
  let mut record_count = 0_i64;
  let mut billed_checkpoint_count = 0_i64;
  for row in rows {
    let (cost_json, checkpoint_requests, checkpoint_records) = row.map_err(database_error)?;
    let parsed_cost = serde_json::from_str::<Value>(&cost_json)
      .map_err(|_| task_error("已结算检查点的成本证据不是合法 JSON"))?;
    if parsed_cost
      .as_object()
      .is_some_and(serde_json::Map::is_empty)
    {
      continue;
    }
    let checkpoint_cost = checkpoint_cost_micros(&cost_json)
      .ok_or_else(|| task_error("已结算检查点缺少有效的 USD 成本证据"))?;
    if checkpoint_requests <= 0 || checkpoint_records < 0 {
      return Err(task_error("已结算检查点的请求或记录数量无效"));
    }
    cost_micros = cost_micros
      .checked_add(checkpoint_cost)
      .ok_or_else(|| task_error("部分成功任务的成本金额溢出"))?;
    request_count = request_count
      .checked_add(checkpoint_requests)
      .ok_or_else(|| task_error("部分成功任务的请求次数溢出"))?;
    record_count = record_count
      .checked_add(checkpoint_records)
      .ok_or_else(|| task_error("部分成功任务的记录数量溢出"))?;
    billed_checkpoint_count += 1;
  }
  if billed_checkpoint_count == 0 {
    return Err(task_error("已有采集结果但缺少已结算检查点成本证据"));
  }
  Ok(
    serde_json::json!({
      "currency": "USD",
      "billing_status": "quoted_not_final",
      "quoted_cost_micros": cost_micros,
      "amount_micros": cost_micros,
      "request_count": request_count,
      "record_count": record_count
    })
    .to_string(),
  )
}

fn run_has_request_evidence(connection: &Connection, run_id: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(
         SELECT 1 FROM collection_page_checkpoint AS checkpoint
         JOIN task_run_step AS run_step
           ON run_step.id = checkpoint.task_run_step_id
         WHERE run_step.task_run_id = ?1 AND (
           checkpoint.status IN ('requesting', 'response_received', 'completed', 'uncertain')
           OR checkpoint.request_attempt_count > 0
           OR checkpoint.requested_at IS NOT NULL
           OR checkpoint.provider_response_json IS NOT NULL
         )
       )",
      params![run_id],
      |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
    .map_err(database_error)
}

#[allow(clippy::too_many_arguments)]
fn settle_active_children(
  connection: &Connection,
  run_id: &str,
  step_status: &str,
  stop_reason: &str,
  uncertain_error_code: &str,
  terminal_error_code: &str,
  error_message: &str,
  now: &str,
) -> AppResult<()> {
  connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = CASE WHEN status = 'requesting' THEN 'uncertain' ELSE 'failed' END,
           retryable = 0,
           last_error_code = CASE WHEN status = 'requesting' THEN ?1 ELSE ?2 END,
           last_error_message = ?3, updated_at = ?4
       WHERE task_run_step_id IN (
         SELECT id FROM task_run_step WHERE task_run_id = ?5
       ) AND status IN ('prepared', 'requesting', 'response_received')",
      params![
        uncertain_error_code,
        terminal_error_code,
        error_message,
        now,
        run_id
      ],
    )
    .map_err(database_error)?;
  connection
    .execute(
      "UPDATE task_run_step
       SET status = ?1, stop_reason = ?2,
           completed_at = COALESCE(completed_at, ?3), updated_at = ?3
       WHERE task_run_id = ?4 AND status IN ('pending', 'running')",
      params![step_status, stop_reason, now, run_id],
    )
    .map_err(database_error)?;
  Ok(())
}

fn active_run_ids(connection: &Connection, task_id: &str) -> AppResult<Vec<String>> {
  let mut statement = connection
    .prepare("SELECT id FROM task_run WHERE task_id = ?1 AND status IN ('queued', 'running')")
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], |row| row.get::<_, String>(0))
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}
