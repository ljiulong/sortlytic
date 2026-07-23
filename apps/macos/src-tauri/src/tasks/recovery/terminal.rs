use chrono::Utc;
use rusqlite::{params, Connection};

use crate::domain::AppResult;

use super::{append_task_log, database_error, task_error, UNCERTAIN_REQUEST_CODE};

pub(super) fn stop_queued_run(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  stage: &str,
  code: &str,
  message: &str,
) -> AppResult<()> {
  stop_run_in_state(connection, run_id, task_id, stage, code, message, "queued")
}

pub(super) fn mark_requesting_uncertain(connection: &Connection, run_id: &str) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let message = "进程中断时请求处于 requesting，远端副作用无法确认";
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'uncertain', retryable = 0, last_error_code = ?1,
           last_error_message = ?2, updated_at = ?3
       WHERE status = 'requesting' AND task_run_step_id IN (
         SELECT id FROM task_run_step WHERE task_run_id = ?4
       )",
      params![UNCERTAIN_REQUEST_CODE, message, now, run_id],
    )
    .map_err(database_error)?;
  if changed == 0 {
    return Err(task_error(
      "requesting 检查点状态已变化，无法标记 uncertain",
    ));
  }
  Ok(())
}

pub(super) fn stop_run(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  stage: &str,
  code: &str,
  message: &str,
) -> AppResult<()> {
  stop_run_in_state(connection, run_id, task_id, stage, code, message, "running")
}

fn stop_run_in_state(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  stage: &str,
  code: &str,
  message: &str,
  expected_status: &str,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let changed = connection
    .execute(
      "UPDATE task_run
       SET status = 'failed', ended_at = ?1, current_stage = ?2,
           error_code = ?3, error_message = ?4, retryable = 0, claimed_at = NULL
       WHERE id = ?5 AND task_id = ?6 AND status = ?7",
      params![now, stage, code, message, run_id, task_id, expected_status],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("活动运行状态已变化，无法安全停止"));
  }
  connection
    .execute(
      "UPDATE task_run_step
       SET status = 'failed', stop_reason = ?1,
           completed_at = COALESCE(completed_at, ?2), updated_at = ?2
       WHERE task_run_id = ?3 AND status IN ('pending', 'running')",
      params![checkpoint_stop_reason(code), now, run_id],
    )
    .map_err(database_error)?;
  let task_changed = connection
    .execute(
      "UPDATE collection_task SET status = 'failed', updated_at = ?1
       WHERE id = ?2 AND status = ?3",
      params![now, task_id, expected_status],
    )
    .map_err(database_error)?;
  if task_changed != 1 {
    return Err(task_error("父任务状态已变化，无法原子停止活动运行"));
  }
  append_task_log(connection, run_id, stage, "error", message)
}

fn checkpoint_stop_reason(error_code: &str) -> &'static str {
  match error_code {
    UNCERTAIN_REQUEST_CODE => "uncertain_request",
    "REQUEST_LIMIT_REACHED" => "request_limit",
    "RECORD_LIMIT_REACHED" => "record_limit",
    "BUDGET_LIMIT_REACHED" => "budget_limit",
    _ => "terminal_error",
  }
}
