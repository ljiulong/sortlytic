use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::AppResult;
use crate::tikhub::{CollectionPage, TikHubCollectionRequest};

use super::{database_error, task_error};

pub(super) fn mark_step_running(connection: &Connection, step_id: &str) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let changed = connection
    .execute(
      "UPDATE task_run_step
       SET status = 'running', started_at = COALESCE(started_at, ?1), updated_at = ?1
       WHERE id = ?2 AND status IN ('pending', 'running') AND stop_reason IS NULL",
      params![now, step_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("运行步骤状态已变化，无法开始执行"));
  }
  Ok(())
}

pub(super) fn mark_step_success(
  connection: &Connection,
  step_id: &str,
  now: &str,
) -> AppResult<()> {
  let changed = connection
    .execute(
      "UPDATE task_run_step
       SET status = 'success', completed_at = ?1, updated_at = ?1
       WHERE id = ?2 AND status = 'running' AND started_at IS NOT NULL AND stop_reason IS NULL",
      params![now, step_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("运行步骤无法进入成功终态"));
  }
  Ok(())
}

pub(super) fn mark_step_stopped(
  connection: &Connection,
  step_id: &str,
  stop_reason: &str,
  now: &str,
) -> AppResult<()> {
  let changed = connection
    .execute(
      "UPDATE task_run_step
       SET status = 'success', stop_reason = ?1, completed_at = ?2, updated_at = ?2
       WHERE id = ?3 AND status = 'running' AND started_at IS NOT NULL AND stop_reason IS NULL",
      params![stop_reason, now, step_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("运行步骤无法记录正常停止原因"));
  }
  Ok(())
}

pub(super) fn insert_prepared_checkpoint(
  connection: &Connection,
  run_step_id: &str,
  page_index: i64,
  cursor: Option<&Value>,
) -> AppResult<(String, String)> {
  let checkpoint_id = Uuid::new_v4().to_string();
  let idempotency_key = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, input_cursor_json,
         status, created_at, updated_at
       ) VALUES (?1, ?2, ?3, ?4, ?5, 'prepared', ?6, ?6)",
      params![
        checkpoint_id,
        run_step_id,
        page_index,
        idempotency_key.clone(),
        cursor.map(Value::to_string),
        now
      ],
    )
    .map_err(database_error)?;
  Ok((checkpoint_id, idempotency_key))
}

pub(super) fn mark_checkpoint_requesting(
  connection: &Connection,
  checkpoint_id: &str,
  requested_at: &str,
) -> AppResult<()> {
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'requesting', request_attempt_count = request_attempt_count + 1,
           requested_at = ?1, updated_at = ?1
       WHERE id = ?2 AND status = 'prepared'",
      params![requested_at, checkpoint_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("检查点无法进入 requesting 状态"));
  }
  Ok(())
}

pub(super) fn mark_checkpoint_uncertain(
  connection: &Connection,
  checkpoint_id: &str,
  detail: &str,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'uncertain', retryable = 0, last_error_code = 'UNCERTAIN_REQUEST_AFTER_FAILURE',
           last_error_message = ?1, updated_at = ?2
       WHERE id = ?3 AND status = 'requesting'",
      params![detail, now, checkpoint_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("检查点无法标记为 uncertain"));
  }
  Ok(())
}

pub(super) fn mark_checkpoint_failed(
  connection: &Connection,
  checkpoint_id: &str,
  error_code: &str,
  error_message: &str,
) -> AppResult<()> {
  mark_checkpoint_failed_with_retryable(connection, checkpoint_id, error_code, error_message, false)
}

pub(super) fn mark_checkpoint_failed_with_retryable(
  connection: &Connection,
  checkpoint_id: &str,
  error_code: &str,
  error_message: &str,
  retryable: bool,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'failed', retryable = ?1, last_error_code = ?2,
           last_error_message = ?3, updated_at = ?4
       WHERE id = ?5 AND status = 'requesting'",
      params![
        i64::from(retryable),
        error_code,
        error_message,
        now,
        checkpoint_id
      ],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("检查点无法标记为失败"));
  }
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn mark_checkpoint_response_received(
  connection: &Connection,
  checkpoint_id: &str,
  raw_response: &str,
  response_hash: &str,
  response_size: i64,
  request: &TikHubCollectionRequest,
  input_cursor_json: Option<&str>,
  page: &CollectionPage,
  persisted_count: usize,
  cost_actual_json: &str,
  response_received_at: &str,
  next_cursor_json: Option<&str>,
) -> AppResult<()> {
  let received_count =
    i64::try_from(page.records.len()).map_err(|_| task_error("TikHub 响应记录数超出数据库范围"))?;
  let persisted_count =
    i64::try_from(persisted_count).map_err(|_| task_error("已持久化记录数超出数据库范围"))?;
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'response_received', input_cursor_json = ?1,
           final_endpoint_key = ?2, provider_response_json = ?3,
           provider_response_hash = ?4, provider_response_size = ?5,
           has_more = ?6, next_cursor_json = ?7, record_count_received = ?8,
           record_count_persisted = ?9, cost_actual_json = ?10,
           response_received_at = ?11, updated_at = ?11
       WHERE id = ?12 AND status = 'requesting'",
      params![
        input_cursor_json,
        request.paths().last(),
        raw_response,
        response_hash,
        response_size,
        i64::from(page.has_more),
        next_cursor_json,
        received_count,
        persisted_count,
        cost_actual_json,
        response_received_at,
        checkpoint_id
      ],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("检查点无法记录 TikHub 响应"));
  }
  Ok(())
}

pub(super) fn mark_checkpoint_completed(
  connection: &Connection,
  checkpoint_id: &str,
  committed_at: &str,
) -> AppResult<()> {
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'completed', committed_at = ?1, updated_at = ?1
       WHERE id = ?2 AND status = 'response_received'",
      params![committed_at, checkpoint_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("检查点无法进入 completed 状态"));
  }
  Ok(())
}
