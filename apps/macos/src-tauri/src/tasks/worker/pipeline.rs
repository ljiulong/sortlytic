use std::collections::VecDeque;
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::targets::{materialize_targets, PipelineTarget, TargetStepInput};
use super::{
  database_error, insert_prepared_checkpoint, mark_checkpoint_completed,
  mark_checkpoint_requesting, mark_checkpoint_response_received, mark_checkpoint_uncertain,
  mark_step_running, mark_step_stopped, mark_step_success, open_workspace_connection,
  persist_step_accounts, task_error, worker_error, RunStep,
};
use crate::domain::AppResult;
use crate::records::{persist_collection_page, PersistCollectionPageInput};
use crate::tikhub::{build_collection_request, CollectionPage, TikHubCollectionRequest};

pub(super) fn execute_pipeline_step<F>(
  root_path: &Path,
  step: &RunStep,
  fetch_page: &F,
) -> AppResult<()>
where
  F: Fn(&TikHubCollectionRequest) -> AppResult<CollectionPage>,
{
  let connection = open_workspace_connection(root_path)?;
  let run_id = connection
    .query_row(
      "SELECT task_run_id FROM task_run_step WHERE id = ?1",
      params![step.id],
      |row| row.get::<_, String>(0),
    )
    .map_err(database_error)?;
  let targets = materialize_targets(
    &connection,
    &TargetStepInput {
      task_run_id: run_id.clone(),
      step_key: step.step_key.clone(),
      platform: step.platform.clone(),
      data_type: step.data_type.clone(),
      params: step.params.clone(),
      output_selected: step.output_selected,
      depends_on_step_key: step.depends_on_step_key.clone(),
    },
  )?;
  mark_step_running(&connection, &step.id)?;
  if targets.is_empty() {
    return mark_step_stopped(
      &connection,
      &step.id,
      "provider_exhausted",
      &Utc::now().to_rfc3339(),
    );
  }

  let mut page_index = checkpoint_count(&connection, &step.id)?;
  if page_index > 0 && targets.iter().any(|target| target.status == "running") {
    return Err(worker_error(
      "PIPELINE_RECOVERY_REQUIRES_REVIEW",
      "多目标步骤存在无法映射到具体目标的中断请求",
      false,
    ));
  }
  let mut queue = targets
    .into_iter()
    .filter(|target| matches!(target.status.as_str(), "pending" | "running"))
    .collect::<VecDeque<_>>();
  let mut request_limited = false;

  while let Some(mut target) = queue.pop_front() {
    if output_count(&connection, &run_id)? >= step.record_limit {
      stop_remaining_targets(&connection, &run_id, &step.step_key)?;
      return mark_step_stopped(
        &connection,
        &step.id,
        "record_limit",
        &Utc::now().to_rfc3339(),
      );
    }
    if target.request_count >= step.request_limit {
      let cursor = target.cursor.clone();
      update_target(&connection, &mut target, "exhausted", cursor)?;
      request_limited = true;
      continue;
    }
    execute_target_page(
      root_path,
      &connection,
      step,
      &run_id,
      page_index,
      &mut target,
      fetch_page,
    )?;
    page_index += 1;
    if target.status == "pending" {
      queue.push_back(target);
    }
  }

  let now = Utc::now().to_rfc3339();
  if request_limited {
    mark_step_stopped(&connection, &step.id, "request_limit", &now)
  } else {
    mark_step_success(&connection, &step.id, &now)
  }
}

#[allow(clippy::too_many_arguments)]
fn execute_target_page<F>(
  root_path: &Path,
  connection: &Connection,
  step: &RunStep,
  run_id: &str,
  page_index: i64,
  target: &mut PipelineTarget,
  fetch_page: &F,
) -> AppResult<()>
where
  F: Fn(&TikHubCollectionRequest) -> AppResult<CollectionPage>,
{
  let request = build_collection_request(
    &step.platform,
    &step.data_type,
    &target.params,
    target.cursor.as_ref(),
  )?;
  let (checkpoint_id, idempotency_key) =
    insert_prepared_checkpoint(connection, &step.id, page_index, target.cursor.as_ref())?;
  let request = request.with_idempotency_key(idempotency_key)?;
  let requested_at = Utc::now().to_rfc3339();
  mark_checkpoint_requesting(connection, &checkpoint_id, &requested_at)?;
  let page = match fetch_page(&request) {
    Ok(page) => page,
    Err(error) => {
      mark_checkpoint_uncertain(connection, &checkpoint_id, &error.message)?;
      return Err(worker_error(
        "UNCERTAIN_REQUEST_AFTER_FAILURE",
        "TikHub 目标请求已发出但响应状态不确定，已禁止自动重试",
        false,
      ));
    }
  };
  let response_received_at = Utc::now().to_rfc3339();
  let persisted = match persist_collection_page(
    root_path,
    PersistCollectionPageInput {
      task_id: step.task_id.clone(),
      task_run_id: run_id.to_string(),
      platform: step.platform.clone(),
      data_type: step.data_type.clone(),
      records: page.records.clone(),
      collected_at: Some(response_received_at.clone()),
    },
  ) {
    Ok(persisted) => persisted,
    Err(error) => {
      mark_checkpoint_uncertain(connection, &checkpoint_id, &error.message)?;
      return Err(worker_error(
        "RECORD_PERSISTENCE_FAILED",
        "TikHub 目标响应已返回但记录落库失败，已禁止自动重试",
        false,
      ));
    }
  };
  let persisted_count = persisted
    .inserted_count
    .checked_add(persisted.existing_count)
    .ok_or_else(|| task_error("已持久化记录数溢出"))?;
  if persisted_count != page.records.len() {
    return Err(worker_error(
      "RECORD_PERSISTENCE_INCOMPLETE",
      "TikHub 目标响应记录未能全部写入本地存储",
      false,
    ));
  }
  persist_step_accounts(
    connection,
    step,
    run_id,
    &page.records,
    Some(&response_received_at),
  )?;

  let raw_response = page.raw_response.to_string();
  let response_hash = format!("{:x}", Sha256::digest(raw_response.as_bytes()));
  let response_size =
    i64::try_from(raw_response.len()).map_err(|_| task_error("TikHub 响应体大小超出数据库范围"))?;
  let next_cursor_json = page.next_cursor.as_ref().map(Value::to_string);
  let input_cursor_json = target.cursor.as_ref().map(Value::to_string);
  mark_checkpoint_response_received(
    connection,
    &checkpoint_id,
    &raw_response,
    &response_hash,
    response_size,
    &request,
    input_cursor_json.as_deref(),
    &page,
    persisted_count,
    &serde_json::json!({ "currency": "USD", "amount_micros": 0 }).to_string(),
    &response_received_at,
    next_cursor_json.as_deref(),
  )?;
  mark_checkpoint_completed(connection, &checkpoint_id, &Utc::now().to_rfc3339())?;

  target.request_count += 1;
  if page.has_more && target.request_count < step.request_limit {
    update_target(connection, target, "pending", page.next_cursor)?;
  } else if page.has_more {
    update_target(connection, target, "exhausted", page.next_cursor)?;
  } else {
    update_target(connection, target, "success", None)?;
  }
  Ok(())
}

fn update_target(
  connection: &Connection,
  target: &mut PipelineTarget,
  status: &str,
  cursor: Option<Value>,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  connection
    .execute(
      "UPDATE collection_pipeline_target
       SET status = ?1, cursor_json = ?2, request_count = ?3, updated_at = ?4
       WHERE id = ?5",
      params![
        status,
        cursor.as_ref().map(Value::to_string),
        target.request_count,
        now,
        target.id
      ],
    )
    .map_err(database_error)?;
  target.status = status.to_string();
  target.cursor = cursor;
  Ok(())
}

fn checkpoint_count(connection: &Connection, run_step_id: &str) -> AppResult<i64> {
  connection
    .query_row(
      "SELECT COUNT(*) FROM collection_page_checkpoint WHERE task_run_step_id = ?1",
      params![run_step_id],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn output_count(connection: &Connection, run_id: &str) -> AppResult<i64> {
  connection
    .query_row(
      "SELECT COUNT(*) FROM collected_account
       WHERE task_run_id = ?1 AND output_included = 1",
      params![run_id],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn stop_remaining_targets(connection: &Connection, run_id: &str, step_key: &str) -> AppResult<()> {
  connection
    .execute(
      "UPDATE collection_pipeline_target
       SET status = 'exhausted', updated_at = ?1
       WHERE task_run_id = ?2 AND step_key = ?3 AND status IN ('pending', 'running')",
      params![Utc::now().to_rfc3339(), run_id, step_key],
    )
    .map_err(database_error)?;
  Ok(())
}
