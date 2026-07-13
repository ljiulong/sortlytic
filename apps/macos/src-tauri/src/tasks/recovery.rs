use std::path::Path;

use chrono::Utc;
use rusqlite::{params, TransactionBehavior};

use crate::domain::{AppErrorCode, AppResult};

use super::execution::{
  append_task_log, quarantine_run_for_reconfirmation, require_executable_plan,
};
use super::{database_error, open_workspace_connection, task_error};

pub fn recover_interrupted_runs(root_path: impl AsRef<Path>) -> AppResult<i64> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let interrupted = {
    let mut statement = transaction
      .prepare(
        "SELECT run.id, run.task_id, run.plan_id
         FROM task_run AS run
         WHERE run.status = 'running'
         ORDER BY run.started_at, run.id",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map([], |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      })
      .map_err(database_error)?;
    rows
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(database_error)?
  };
  let mut recoverable = Vec::new();
  for (run_id, task_id, plan_id) in interrupted {
    let plan_validation = plan_id.as_deref().map_or_else(
      || Err(task_error("运行中的任务缺少采集计划，不能恢复")),
      |plan_id| require_executable_plan(&transaction, &task_id, plan_id),
    );
    match plan_validation {
      Ok(()) => recoverable.push((run_id, task_id, plan_id)),
      Err(error) if matches!(&error.code, AppErrorCode::ValidationError) => {
        quarantine_run_for_reconfirmation(
          &transaction,
          &run_id,
          &task_id,
          plan_id.as_deref(),
          &error.message,
        )?;
      }
      Err(error) => return Err(error),
    }
  }
  let now = Utc::now().to_rfc3339();

  for (run_id, task_id, _) in &recoverable {
    transaction
      .execute(
        "UPDATE task_run
         SET status = 'queued', current_stage = '恢复等待', ended_at = NULL,
             error_code = NULL, error_message = NULL, retryable = 1, claimed_at = NULL
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
  let recovered = recoverable.len() as i64;
  transaction.commit().map_err(database_error)?;
  Ok(recovered)
}
