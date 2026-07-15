use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;
use crate::tikhub::{build_collection_request, parse_collection_page, CollectionPage};

use super::{database_error, task_error, worker_error, Checkpoint, RunStep};

pub(super) fn ensure_record_limit(
  connection: &Connection,
  run_id: &str,
  record_limit: i64,
  page_count: usize,
) -> AppResult<()> {
  let persisted_before: i64 = connection
    .query_row(
      "SELECT COALESCE(SUM(checkpoint.record_count_persisted), 0)
       FROM collection_page_checkpoint AS checkpoint
       JOIN task_run_step AS run_step
         ON run_step.id = checkpoint.task_run_step_id
       WHERE run_step.task_run_id = ?1",
      params![run_id],
      |row| row.get(0),
    )
    .map_err(database_error)?;
  let page_count =
    i64::try_from(page_count).map_err(|_| task_error("TikHub 响应记录数超出数据库范围"))?;
  let persisted_after = persisted_before
    .checked_add(page_count)
    .ok_or_else(|| task_error("累计记录数溢出"))?;
  if persisted_after > record_limit {
    return Err(worker_error(
      "RECORD_LIMIT_REACHED",
      "TikHub 响应记录数将超过计划上限",
      false,
    ));
  }
  Ok(())
}

pub(super) fn parse_response_checkpoint(
  step: &RunStep,
  checkpoint: &Checkpoint,
) -> AppResult<CollectionPage> {
  let raw_response = checkpoint
    .provider_response_json
    .as_deref()
    .ok_or_else(|| {
      worker_error(
        "RESPONSE_CHECKPOINT_INVALID",
        "恢复检查点缺少响应内容",
        false,
      )
    })?;
  let expected_hash = checkpoint
    .provider_response_hash
    .as_deref()
    .ok_or_else(|| {
      worker_error(
        "RESPONSE_CHECKPOINT_INVALID",
        "恢复检查点缺少响应哈希",
        false,
      )
    })?;
  let expected_size = checkpoint.provider_response_size.ok_or_else(|| {
    worker_error(
      "RESPONSE_CHECKPOINT_INVALID",
      "恢复检查点缺少响应大小",
      false,
    )
  })?;
  let actual_hash = format!("{:x}", Sha256::digest(raw_response.as_bytes()));
  let actual_size = i64::try_from(raw_response.len()).map_err(|_| {
    worker_error(
      "RESPONSE_CHECKPOINT_INVALID",
      "响应大小超出数据库范围",
      false,
    )
  })?;
  if actual_hash != expected_hash || actual_size != expected_size {
    return Err(worker_error(
      "RESPONSE_CHECKPOINT_INVALID",
      "恢复检查点响应校验失败",
      false,
    ));
  }
  let response = serde_json::from_str(raw_response).map_err(|_| {
    worker_error(
      "RESPONSE_CHECKPOINT_INVALID",
      "恢复检查点响应不是合法 JSON",
      false,
    )
  })?;
  let request = build_collection_request(
    &step.platform,
    &step.data_type,
    &step.params,
    checkpoint.input_cursor.as_ref(),
  )?;
  let page = parse_collection_page(&request, response).map_err(|_| {
    worker_error(
      "RESPONSE_CHECKPOINT_INVALID",
      "恢复检查点响应不符合当前 endpoint 契约",
      false,
    )
  })?;
  let received = i64::try_from(page.records.len()).map_err(|_| {
    worker_error(
      "RESPONSE_CHECKPOINT_INVALID",
      "响应记录数超出数据库范围",
      false,
    )
  })?;
  if checkpoint.request_attempt_count <= 0
    || checkpoint.has_more != Some(page.has_more)
    || page.next_cursor != checkpoint.next_cursor
    || received != checkpoint.record_count_received
    || checkpoint.response_received_at.is_none()
  {
    return Err(worker_error(
      "RESPONSE_CHECKPOINT_INVALID",
      "恢复检查点的响应元数据与正文不一致",
      false,
    ));
  }
  Ok(page)
}

pub(super) fn mark_response_checkpoint_completed(
  connection: &Connection,
  checkpoint_id: &str,
  persisted_count: i64,
  committed_at: &str,
) -> AppResult<()> {
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'completed', record_count_persisted = ?1,
           committed_at = ?2, updated_at = ?2
       WHERE id = ?3 AND status = 'response_received'",
      params![persisted_count, committed_at, checkpoint_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("响应检查点无法进入 completed 状态"));
  }
  Ok(())
}
