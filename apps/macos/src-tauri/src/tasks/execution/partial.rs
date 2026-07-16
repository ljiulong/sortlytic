use rusqlite::{params, Connection};
use uuid::Uuid;

use super::{
  checkpoint_cost_micros, database_error, task_error, valid_timestamp, CompletionCheckpoint,
};
use crate::domain::AppResult;

pub(super) fn append_completion_log(
  connection: &Connection,
  run_id: &str,
  terminal_status: &str,
  created_at: &str,
) -> AppResult<()> {
  let (stage, level, message) = match terminal_status {
    "partial_success" => ("部分成功", "warning", "任务部分目标失败，合格数据已保留"),
    "failed" => ("执行失败", "error", "全部采集目标失败"),
    _ => ("已完成", "info", "任务执行成功"),
  };
  let changed = connection
    .execute(
      "INSERT INTO task_log (
         id, task_run_id, stage, level, message, safe_details_json, created_at
       ) VALUES (?1, ?2, ?3, ?4, ?5, '{}', ?6)",
      params![
        Uuid::new_v4().to_string(),
        run_id,
        stage,
        level,
        message,
        created_at
      ],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("任务终态日志未写入，已回滚终态更新"));
  }
  Ok(())
}

pub(super) fn failed_checkpoint_is_complete(checkpoint: &CompletionCheckpoint) -> bool {
  let (Some(requested_at), Some(committed_at)) = (
    valid_timestamp(checkpoint.requested_at.as_deref()),
    valid_timestamp(checkpoint.committed_at.as_deref()),
  ) else {
    return false;
  };
  checkpoint.status == "failed"
    && checkpoint.request_attempt_count > 0
    && !checkpoint.retryable
    && checkpoint
      .last_error_code
      .as_deref()
      .is_some_and(|value| !value.trim().is_empty())
    && checkpoint
      .last_error_message
      .as_deref()
      .is_some_and(|value| !value.trim().is_empty())
    && checkpoint.provider_response_json.is_none()
    && checkpoint.provider_response_hash.is_none()
    && checkpoint.provider_response_size.is_none()
    && checkpoint.response_received_at.is_none()
    && checkpoint.has_more.is_none()
    && checkpoint.next_cursor_json.is_none()
    && checkpoint.record_count_received == 0
    && checkpoint.record_count_persisted == 0
    && requested_at <= committed_at
    && checkpoint_cost_micros(&checkpoint.cost_actual_json).is_some()
}

pub(super) fn failure_evidence_matches(
  connection: &Connection,
  run_id: &str,
  run_step_id: &str,
  step_key: &str,
  expected_failures: i64,
) -> AppResult<bool> {
  let matched = connection
    .query_row(
      "SELECT COUNT(*)
       FROM collection_failure_evidence AS failure
       JOIN collection_pipeline_target AS target ON target.id = failure.target_id
       JOIN collection_page_checkpoint AS checkpoint
         ON checkpoint.id = json_extract(failure.evidence_json, '$.checkpoint_id')
       WHERE failure.task_run_id = ?1 AND failure.step_key = ?2
         AND target.task_run_id = ?1 AND target.step_key = ?2 AND target.status = 'failed'
         AND checkpoint.task_run_step_id = ?3 AND checkpoint.status = 'failed'
         AND failure.retryable = 0
         AND failure.error_code = checkpoint.last_error_code
         AND failure.error_message = checkpoint.last_error_message",
      params![run_id, step_key, run_step_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  Ok(matched == expected_failures)
}
