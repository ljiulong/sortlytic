use std::collections::VecDeque;
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::pricing::checkpoint_quote_json;
use super::targets::{materialize_targets, PipelineTarget, TargetStepInput};
use super::{
  database_error, insert_prepared_checkpoint, mark_checkpoint_completed,
  mark_checkpoint_requesting, mark_checkpoint_response_received, mark_checkpoint_uncertain,
  mark_step_running, mark_step_stopped, mark_step_success, open_workspace_connection,
  persist_step_accounts, task_error, worker_error, RunStep,
};
use crate::domain::AppResult;
use crate::domain::{AppError, AppErrorCode};
use crate::records::{persist_collection_page, PersistCollectionPageInput};
use crate::tikhub::{build_collection_request, CollectionPage, TikHubCollectionRequest};

pub(super) fn execute_pipeline_step<G, F>(
  root_path: &Path,
  step: &RunStep,
  guard_request: &G,
  fetch_page: &F,
) -> AppResult<()>
where
  G: Fn(&TikHubCollectionRequest) -> AppResult<()>,
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
      guard_request,
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
fn execute_target_page<G, F>(
  root_path: &Path,
  connection: &Connection,
  step: &RunStep,
  run_id: &str,
  page_index: i64,
  target: &mut PipelineTarget,
  guard_request: &G,
  fetch_page: &F,
) -> AppResult<()>
where
  G: Fn(&TikHubCollectionRequest) -> AppResult<()>,
  F: Fn(&TikHubCollectionRequest) -> AppResult<CollectionPage>,
{
  let request = build_collection_request(
    &step.platform,
    &step.data_type,
    &target.params,
    target.cursor.as_ref(),
  )?;
  guard_request(&request)?;
  let current_cursor = target.cursor.clone();
  update_target(connection, target, "running", current_cursor)?;
  let (checkpoint_id, idempotency_key) =
    insert_prepared_checkpoint(connection, &step.id, page_index, target.cursor.as_ref())?;
  let request = request.with_idempotency_key(idempotency_key)?;
  let requested_at = Utc::now().to_rfc3339();
  mark_checkpoint_requesting(connection, &checkpoint_id, &requested_at)?;
  let page_result = fetch_page(&request);
  super::ensure_run_accepts_response(root_path, run_id)?;
  let page = match page_result {
    Ok(page) => page,
    Err(error) if is_isolated_target_failure(&error) => {
      persist_target_failure(
        connection,
        step,
        run_id,
        &checkpoint_id,
        target,
        &request,
        &error,
      )?;
      return Ok(());
    }
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
    &checkpoint_quote_json(connection, run_id, &request)?,
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

fn is_isolated_target_failure(error: &AppError) -> bool {
  error.code == AppErrorCode::TikhubRequestError && !error.retryable
}

fn persist_target_failure(
  connection: &Connection,
  step: &RunStep,
  run_id: &str,
  checkpoint_id: &str,
  target: &mut PipelineTarget,
  request: &TikHubCollectionRequest,
  error: &AppError,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let error_code = serde_json::to_string(&error.code)
    .unwrap_or_else(|_| "\"TIKHUB_REQUEST_ERROR\"".to_string())
    .trim_matches('"')
    .to_string();
  let quote_json = checkpoint_quote_json(connection, run_id, request)?;
  let transaction = connection.unchecked_transaction().map_err(database_error)?;
  let checkpoint_changed = transaction
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'failed', retryable = 0, last_error_code = ?1,
           last_error_message = ?2, cost_actual_json = ?3,
           committed_at = ?4, updated_at = ?4
       WHERE id = ?5 AND status = 'requesting'",
      params![error_code, error.message, quote_json, now, checkpoint_id],
    )
    .map_err(database_error)?;
  if checkpoint_changed != 1 {
    return Err(task_error("逐目标失败检查点无法形成确定终态"));
  }
  target.request_count += 1;
  transaction
    .execute(
      "UPDATE collection_pipeline_target
       SET status = 'failed', request_count = ?1, updated_at = ?2
       WHERE id = ?3 AND status IN ('pending', 'running')",
      params![target.request_count, now, target.id],
    )
    .map_err(database_error)?;
  let endpoint_key = format!("{}.{}", step.platform, step.data_type);
  transaction
    .execute(
      "INSERT INTO collection_failure_evidence (
         id, task_run_id, target_id, step_key, endpoint_key, target_key,
         error_code, error_message, retryable, evidence_json, created_at
       ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)",
      params![
        Uuid::new_v4().to_string(),
        run_id,
        target.id,
        step.step_key,
        endpoint_key,
        target.target_key,
        error_code,
        error.message,
        serde_json::json!({
          "checkpoint_id": checkpoint_id,
          "candidate_paths": request.paths(),
          "source_params": request.source_params(),
          "billing": serde_json::from_str::<Value>(&quote_json).unwrap_or(Value::Null)
        })
        .to_string(),
        now
      ],
    )
    .map_err(database_error)?;
  transaction.commit().map_err(database_error)?;
  target.status = "failed".to_string();
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
