use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{redact_sensitive_text, AppResult};

use super::{
  database_error, get_task_by_id, get_task_run, map_task_run, normalize_required,
  open_workspace_connection, task_error, CollectionTaskView, TaskRunView,
};

pub fn enqueue_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let task = get_task_by_id(&transaction, task_id)?;

  if task.confirmed_at.is_none() {
    return Err(task_error("任务必须先确认采集计划才能入队"));
  }
  if !["waiting_confirmation", "failed"].contains(&task.status.as_str()) {
    return Err(task_error("只有已确认或失败任务可以入队"));
  }

  let run_id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, current_stage, retryable)
       VALUES (?1, ?2, 'queued', ?3, '等待执行', 0)",
      params![run_id, task_id, now],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE collection_task
       SET status = 'queued', completed_at = NULL, cancelled_at = NULL, updated_at = ?1
       WHERE id = ?2",
      params![now, task_id],
    )
    .map_err(database_error)?;
  append_task_log(
    &transaction,
    &run_id,
    "等待执行",
    "info",
    "任务已加入本地队列",
  )?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, &run_id)
}

pub fn claim_next_task(root_path: impl AsRef<Path>) -> AppResult<Option<TaskRunView>> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let queued = transaction
    .query_row(
      "SELECT id, task_id, status, started_at, ended_at, current_stage, error_code,
              error_message, retryable, cost_actual_json
       FROM task_run
       WHERE status = 'queued'
       ORDER BY started_at ASC, id ASC
       LIMIT 1",
      [],
      map_task_run,
    )
    .optional()
    .map_err(database_error)?;
  let Some(queued) = queued else {
    transaction.commit().map_err(database_error)?;
    return Ok(None);
  };
  let now = Utc::now().to_rfc3339();
  let changed = transaction
    .execute(
      "UPDATE task_run
       SET status = 'running', current_stage = '执行采集', error_code = NULL,
           error_message = NULL, retryable = 0
       WHERE id = ?1 AND status = 'queued'",
      params![queued.id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("队列任务状态已变化，无法领取"));
  }
  transaction
    .execute(
      "UPDATE collection_task SET status = 'running', updated_at = ?1 WHERE id = ?2",
      params![now, queued.task_id],
    )
    .map_err(database_error)?;
  append_task_log(
    &transaction,
    &queued.id,
    "执行采集",
    "info",
    "本地执行器已领取任务",
  )?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, &queued.id).map(Some)
}

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

pub fn recover_interrupted_runs(root_path: impl AsRef<Path>) -> AppResult<i64> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let interrupted = {
    let mut statement = transaction
      .prepare("SELECT id, task_id FROM task_run WHERE status = 'running' ORDER BY started_at")
      .map_err(database_error)?;
    let rows = statement
      .query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
      })
      .map_err(database_error)?;
    rows
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(database_error)?
  };
  let now = Utc::now().to_rfc3339();

  for (run_id, task_id) in &interrupted {
    transaction
      .execute(
        "UPDATE task_run
         SET status = 'queued', current_stage = '恢复等待', ended_at = NULL,
             error_code = NULL, error_message = NULL, retryable = 1
         WHERE id = ?1 AND status = 'running'",
        params![run_id],
      )
      .map_err(database_error)?;
    transaction
      .execute(
        "UPDATE collection_task SET status = 'queued', updated_at = ?1
         WHERE id = ?2 AND status = 'running'",
        params![now, task_id],
      )
      .map_err(database_error)?;
    append_task_log(
      &transaction,
      run_id,
      "恢复等待",
      "warning",
      "检测到上次进程中断，任务已重新排队",
    )?;
  }
  let recovered = interrupted.len() as i64;
  transaction.commit().map_err(database_error)?;
  Ok(recovered)
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

pub fn retry_task(
  root_path: impl AsRef<Path>,
  task_id: &str,
  stage: Option<String>,
) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let task = get_task_by_id(&transaction, task_id)?;
  if task.status != "failed" {
    return Err(task_error("只有失败任务可以重试"));
  }
  let run_id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  let stage = stage
    .as_deref()
    .map(|value| normalize_required("重试阶段", value))
    .transpose()?
    .unwrap_or_else(|| "等待执行".to_string());
  transaction
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, current_stage, retryable)
       VALUES (?1, ?2, 'queued', ?3, ?4, 0)",
      params![run_id, task_id, now, stage],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE collection_task SET status = 'queued', updated_at = ?1 WHERE id = ?2",
      params![now, task_id],
    )
    .map_err(database_error)?;
  append_task_log(&transaction, &run_id, &stage, "info", "失败任务已重新排队")?;
  transaction.commit().map_err(database_error)?;
  get_task_run(&connection, &run_id)
}

fn immediate_transaction(connection: &mut Connection) -> AppResult<Transaction<'_>> {
  Transaction::new(connection, TransactionBehavior::Immediate).map_err(database_error)
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

fn append_task_log(
  connection: &Connection,
  run_id: &str,
  stage: &str,
  level: &str,
  message: &str,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO task_log (id, task_run_id, stage, level, message, safe_details_json, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, '{}', ?6)",
      params![
        Uuid::new_v4().to_string(),
        run_id,
        stage,
        level,
        message,
        Utc::now().to_rfc3339()
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}
