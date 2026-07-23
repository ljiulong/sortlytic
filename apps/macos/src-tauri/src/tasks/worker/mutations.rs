use chrono::Utc;
use rusqlite::{params, Connection, Transaction, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::AppResult;
use crate::tikhub::{CollectionPage, TikHubCollectionRequest};

use super::{database_error, task_error, WorkerFence};

pub(super) fn mark_step_running(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  step_id: &str,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
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
  })
}

pub(super) fn mark_step_success(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  step_id: &str,
  now: &str,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
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
  })
}

pub(super) fn mark_step_stopped(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  step_id: &str,
  stop_reason: &str,
  now: &str,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
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
  })
}

pub(super) fn insert_prepared_checkpoint(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  run_step_id: &str,
  page_index: i64,
  cursor: Option<&Value>,
) -> AppResult<(String, String)> {
  with_fenced_write(connection, fence, |connection| {
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
  })
}

pub(super) fn mark_checkpoint_requesting(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  checkpoint_id: &str,
  requested_at: &str,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
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
  })
}

pub(super) fn mark_checkpoint_uncertain(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  checkpoint_id: &str,
  detail: &str,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
    let now = Utc::now().to_rfc3339();
    let changed = connection
      .execute(
        "UPDATE collection_page_checkpoint
         SET status = 'uncertain', retryable = 0,
             last_error_code = 'UNCERTAIN_REQUEST_AFTER_FAILURE',
             last_error_message = ?1, updated_at = ?2
         WHERE id = ?3 AND status = 'requesting'",
        params![detail, now, checkpoint_id],
      )
      .map_err(database_error)?;
    if changed != 1 {
      return Err(task_error("检查点无法标记为 uncertain"));
    }
    Ok(())
  })
}

pub(super) fn mark_checkpoint_failed(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  checkpoint_id: &str,
  error_code: &str,
  error_message: &str,
) -> AppResult<()> {
  mark_checkpoint_failed_with_retryable(
    connection,
    fence,
    checkpoint_id,
    error_code,
    error_message,
    false,
  )
}

pub(super) fn mark_checkpoint_failed_with_retryable(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  checkpoint_id: &str,
  error_code: &str,
  error_message: &str,
  retryable: bool,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
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
  })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn mark_checkpoint_response_received(
  connection: &Connection,
  fence: Option<&WorkerFence>,
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
  with_fenced_write(connection, fence, |connection| {
    let received_count = i64::try_from(page.records.len())
      .map_err(|_| task_error("TikHub 响应记录数超出数据库范围"))?;
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
  })
}

pub(super) fn mark_checkpoint_completed(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  checkpoint_id: &str,
  committed_at: &str,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
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
  })
}

fn with_fenced_write<T>(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  write: impl FnOnce(&Connection) -> AppResult<T>,
) -> AppResult<T> {
  let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if let Some(fence) = fence {
    fence.ensure_current(&transaction)?;
  }
  let result = write(&transaction)?;
  transaction.commit().map_err(database_error)?;
  Ok(result)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::tasks::WorkerFence;

  #[test]
  fn stale_worker_fence_rejects_step_and_checkpoint_mutations() {
    let connection = Connection::open_in_memory().expect("database should open");
    connection
      .execute_batch(
        "CREATE TABLE task_worker_lease (
           id TEXT PRIMARY KEY,
           owner_id TEXT NOT NULL,
           lease_expires_at INTEGER NOT NULL,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           generation INTEGER NOT NULL
         );
         CREATE TABLE task_run_step (
           id TEXT PRIMARY KEY,
           status TEXT NOT NULL,
           stop_reason TEXT,
           started_at TEXT,
           completed_at TEXT,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE collection_page_checkpoint (
           id TEXT PRIMARY KEY,
           status TEXT NOT NULL,
           request_attempt_count INTEGER NOT NULL DEFAULT 0,
           requested_at TEXT,
           updated_at TEXT NOT NULL
         );
         INSERT INTO task_worker_lease (
           id, owner_id, lease_expires_at, created_at, updated_at, generation
         ) VALUES (
           'task_worker', 'replacement-owner', 9223372036854775807, 'now', 'now', 2
         );
         INSERT INTO task_run_step (id, status, updated_at)
         VALUES ('step-1', 'pending', 'now');
         INSERT INTO collection_page_checkpoint (id, status, updated_at)
         VALUES ('checkpoint-1', 'prepared', 'now');",
      )
      .expect("mutation fixture should install");
    let stale =
      WorkerFence::new("stale-owner".to_string(), 1).expect("stale fence should be valid");

    mark_step_running(&connection, Some(&stale), "step-1")
      .expect_err("a stale generation must not start a step");
    mark_checkpoint_requesting(&connection, Some(&stale), "checkpoint-1", "later")
      .expect_err("a stale generation must not dispatch a checkpoint");

    assert_eq!(
      connection
        .query_row(
          "SELECT status FROM task_run_step WHERE id = 'step-1'",
          [],
          |row| row.get::<_, String>(0),
        )
        .unwrap(),
      "pending"
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT status FROM collection_page_checkpoint WHERE id = 'checkpoint-1'",
          [],
          |row| row.get::<_, String>(0),
        )
        .unwrap(),
      "prepared"
    );
  }
}
