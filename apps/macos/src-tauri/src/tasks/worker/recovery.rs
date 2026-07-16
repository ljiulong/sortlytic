use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::domain::AppResult;
use crate::tikhub::{build_collection_request, parse_collection_page, CollectionPage};

use super::{database_error, task_error, worker_error, RunStep};

#[derive(Clone)]
pub(super) struct Checkpoint {
  pub(super) id: String,
  pub(super) page_index: i64,
  pub(super) status: String,
  pub(super) request_attempt_count: i64,
  pub(super) idempotency_key: String,
  pub(super) input_cursor: Option<Value>,
  pub(super) next_cursor: Option<Value>,
  pub(super) has_more: Option<bool>,
  pub(super) provider_response_json: Option<String>,
  pub(super) provider_response_hash: Option<String>,
  pub(super) provider_response_size: Option<i64>,
  pub(super) record_count_received: i64,
  pub(super) response_received_at: Option<String>,
}

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

pub(super) fn resume_position(checkpoints: &[Checkpoint]) -> AppResult<(i64, Option<Value>)> {
  for (index, checkpoint) in checkpoints.iter().enumerate() {
    if checkpoint.page_index != index as i64 {
      return Err(worker_error(
        "CHECKPOINT_CHAIN_INVALID",
        "运行步骤检查点链不连续，已停止执行",
        false,
      ));
    }
    if index == 0 && checkpoint.input_cursor.is_some() {
      return Err(worker_error(
        "CHECKPOINT_CHAIN_INVALID",
        "首个检查点不能携带续页游标",
        false,
      ));
    }
    if index > 0 {
      let previous = &checkpoints[index - 1];
      if previous.has_more != Some(true) || previous.next_cursor != checkpoint.input_cursor {
        return Err(worker_error(
          "CHECKPOINT_CHAIN_INVALID",
          "检查点之间的续页游标不一致，已停止执行",
          false,
        ));
      }
    }
    if index + 1 < checkpoints.len()
      && (checkpoint.status != "completed" || checkpoint.has_more != Some(true))
    {
      return Err(worker_error(
        "CHECKPOINT_CHAIN_INVALID",
        "非末页检查点不能声明采集结束",
        false,
      ));
    }
  }
  let Some(last) = checkpoints.last() else {
    return Ok((0, None));
  };
  match last.status.as_str() {
    "prepared" => Ok((last.page_index, last.input_cursor.clone())),
    "response_received" => Ok((last.page_index, last.input_cursor.clone())),
    "completed" if last.has_more == Some(false) => Ok((last.page_index + 1, None)),
    "completed" if last.has_more == Some(true) && last.next_cursor.is_some() => {
      Ok((last.page_index + 1, last.next_cursor.clone()))
    }
    "completed" => Err(worker_error(
      "CHECKPOINT_CHAIN_INVALID",
      "续页检查点缺少有效游标",
      false,
    )),
    _ => Err(worker_error(
      "CHECKPOINT_STATE_UNSUPPORTED",
      "运行步骤存在无法安全恢复的未完成检查点",
      false,
    )),
  }
}

pub(super) fn load_checkpoints(
  connection: &Connection,
  run_step_id: &str,
) -> AppResult<Vec<Checkpoint>> {
  let mut statement = connection
    .prepare(
      "SELECT id, page_index, status, request_attempt_count, idempotency_key, input_cursor_json,
              next_cursor_json, has_more, provider_response_json,
              provider_response_hash, provider_response_size,
              record_count_received, response_received_at
       FROM collection_page_checkpoint
       WHERE task_run_step_id = ?1
       ORDER BY page_index, id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_step_id], |row| {
      let input_cursor: Option<String> = row.get(5)?;
      let next_cursor: Option<String> = row.get(6)?;
      Ok(Checkpoint {
        id: row.get(0)?,
        page_index: row.get(1)?,
        status: row.get(2)?,
        request_attempt_count: row.get(3)?,
        idempotency_key: row.get(4)?,
        input_cursor: input_cursor
          .map(|value| serde_json::from_str(&value))
          .transpose()
          .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
              5,
              rusqlite::types::Type::Text,
              Box::new(error),
            )
          })?,
        next_cursor: next_cursor
          .map(|value| serde_json::from_str(&value))
          .transpose()
          .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
              6,
              rusqlite::types::Type::Text,
              Box::new(error),
            )
          })?,
        has_more: row.get::<_, Option<i64>>(7)?.map(|value| value != 0),
        provider_response_json: row.get(8)?,
        provider_response_hash: row.get(9)?,
        provider_response_size: row.get(10)?,
        record_count_received: row.get(11)?,
        response_received_at: row.get(12)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}
