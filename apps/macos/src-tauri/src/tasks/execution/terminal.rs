use super::*;

pub fn complete_task_run(
  root_path: impl AsRef<Path>,
  run_id: &str,
  actual_cost_json: Value,
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
  transaction
    .execute(
      "UPDATE task_run
       SET status = 'success', ended_at = ?1, current_stage = '已完成',
           cost_actual_json = ?2, retryable = 0
       WHERE id = ?3 AND status = 'running'",
      params![now, actual_cost_json.to_string(), run_id],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE collection_task
       SET status = 'success', completed_at = ?1, actual_cost_json = ?2, updated_at = ?1
       WHERE id = ?3 AND status = 'running'",
      params![now, actual_cost_json.to_string(), run.task_id],
    )
    .map_err(database_error)?;
  append_task_log(&transaction, run_id, "已完成", "info", "任务执行成功")?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, run_id)
}

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

  transaction
    .execute(
      "UPDATE task_run
       SET status = 'failed', ended_at = ?1, current_stage = '执行失败', error_code = ?2,
           error_message = ?3, retryable = ?4
       WHERE id = ?5 AND status = 'running'",
      params![now, code, message, i64::from(retryable), run_id],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE collection_task SET status = 'failed', updated_at = ?1
       WHERE id = ?2 AND status = 'running'",
      params![now, run.task_id],
    )
    .map_err(database_error)?;
  append_task_log(&transaction, run_id, "执行失败", "error", &message)?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, run_id)
}

pub fn cancel_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<CollectionTaskView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let task = get_task_by_id(&transaction, task_id)?;
  if matches!(task.status.as_str(), "success" | "cancelled") {
    return Err(task_error("成功或已取消任务不能再次取消"));
  }
  let active_runs = active_run_ids(&transaction, task_id)?;
  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "UPDATE collection_task SET status = 'cancelled', cancelled_at = ?1, updated_at = ?1
       WHERE id = ?2",
      params![now, task_id],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE task_run SET status = 'cancelled', ended_at = ?1, current_stage = '用户取消'
       WHERE task_id = ?2 AND status IN ('queued', 'running')",
      params![now, task_id],
    )
    .map_err(database_error)?;
  for run_id in active_runs {
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

fn require_running(run: &TaskRunView) -> AppResult<()> {
  if run.status == "running" {
    Ok(())
  } else {
    Err(task_error("只有运行中的任务记录可以结束"))
  }
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
