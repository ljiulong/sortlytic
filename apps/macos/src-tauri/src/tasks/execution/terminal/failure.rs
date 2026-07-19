use super::*;

pub fn fail_task_run(
  root_path: impl AsRef<Path>,
  run_id: &str,
  error_code: &str,
  error_message: &str,
  retryable: bool,
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

  let run_changed = transaction
    .execute(
      "UPDATE task_run
       SET status = ?1, ended_at = ?2, current_stage = ?3, error_code = ?4,
           error_message = ?5, retryable = ?6
       WHERE id = ?7 AND status = 'running'",
      params![
        terminal_status,
        now,
        current_stage,
        code,
        message,
        i64::from(!budget_stopped_with_results && retryable && !has_request_evidence),
        run_id
      ],
    )
    .map_err(database_error)?;
  if run_changed != 1 {
    return Err(task_error("运行记录状态已变化，无法标记终态"));
  }
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
  let task_changed = transaction
    .execute(
      "UPDATE collection_task
       SET status = ?1,
           completed_at = CASE WHEN ?1 = 'partial_success' THEN ?2 ELSE completed_at END,
           updated_at = ?2
       WHERE id = ?3 AND status = 'running'",
      params![terminal_status, now, run.task_id],
    )
    .map_err(database_error)?;
  if task_changed != 1 {
    return Err(task_error("父任务状态已变化，无法标记终态"));
  }
  append_task_log(
    &transaction,
    run_id,
    current_stage,
    if budget_stopped_with_results {
      "warning"
    } else {
      "error"
    },
    &message,
  )?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, run_id)
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
