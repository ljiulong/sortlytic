use std::collections::VecDeque;
use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::mutations::with_fenced_write;
use super::pricing::checkpoint_quote_json;
use super::targets::{materialize_targets, PipelineTarget, TargetStepInput};
use super::{
  database_error, ensure_run_accepts_dispatch, insert_prepared_checkpoint,
  mark_checkpoint_completed, mark_checkpoint_failed, mark_checkpoint_failed_with_retryable,
  mark_checkpoint_requesting, mark_checkpoint_response_received, mark_checkpoint_uncertain,
  mark_step_running, mark_step_stopped, mark_step_success, open_workspace_connection,
  persist_step_accounts, persist_worker_page, response_status_is_uncertain, serialized_error_code,
  task_error, with_task_dispatch_gate, worker_error, RunStep, WorkerFence,
};
use crate::domain::AppResult;
use crate::domain::{AppError, AppErrorCode};
use crate::records::PersistCollectionPageInput;
use crate::tikhub::{build_collection_request, CollectionPage, TikHubCollectionRequest};

pub(super) fn execute_pipeline_step<G, F>(
  root_path: &Path,
  step: &RunStep,
  fence: Option<&WorkerFence>,
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
      target_limit: step.record_limit,
      output_selected: step.output_selected,
      depends_on_step_key: step.depends_on_step_key.clone(),
      input_binding: step.input_binding.clone(),
      dependency_data_type: step.dependency_data_type.clone(),
    },
    fence,
  )?;
  mark_step_running(&connection, fence, &step.id)?;
  if targets.is_empty() {
    return mark_step_stopped(
      &connection,
      fence,
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
    if step.depends_on_step_key.is_none()
      && output_count(&connection, &run_id)? >= step.record_limit
    {
      stop_remaining_targets(&connection, fence, &run_id, &step.step_key)?;
      return mark_step_stopped(
        &connection,
        fence,
        &step.id,
        "record_limit",
        &Utc::now().to_rfc3339(),
      );
    }
    if target.request_count >= step.request_limit {
      let cursor = target.cursor.clone();
      update_target(&connection, fence, &mut target, "exhausted", cursor)?;
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
      fence,
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
    mark_step_stopped(&connection, fence, &step.id, "request_limit", &now)
  } else {
    mark_step_success(&connection, fence, &step.id, &now)
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
  fence: Option<&WorkerFence>,
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
  let (checkpoint_id, request, page_result) =
    with_task_dispatch_gate(root_path, &step.task_id, fence.is_some(), || {
      ensure_run_accepts_dispatch(connection, &step.task_id, run_id, &step.id)?;
      let current_cursor = target.cursor.clone();
      update_target(connection, fence, target, "running", current_cursor)?;
      let (checkpoint_id, idempotency_key) = insert_prepared_checkpoint(
        connection,
        fence,
        &step.id,
        page_index,
        target.cursor.as_ref(),
      )?;
      let request = request.with_idempotency_key(idempotency_key)?;
      let requested_at = Utc::now().to_rfc3339();
      mark_checkpoint_requesting(connection, fence, &checkpoint_id, &requested_at)?;
      let page_result = fetch_page(&request);
      Ok((checkpoint_id, request, page_result))
    })?;
  super::ensure_run_accepts_response(root_path, run_id)?;
  let page = match page_result {
    Ok(page) => page,
    Err(error) if is_isolated_target_failure(&error) => {
      persist_target_failure(
        connection,
        fence,
        target,
        TargetFailureContext {
          step,
          run_id,
          checkpoint_id: &checkpoint_id,
          request: &request,
          error: &error,
        },
      )?;
      return Ok(());
    }
    Err(error) if response_status_is_uncertain(&error) => {
      mark_checkpoint_uncertain(connection, fence, &checkpoint_id, &error.message)?;
      return Err(worker_error(
        "UNCERTAIN_REQUEST_AFTER_FAILURE",
        "TikHub 目标请求已发出但响应状态不确定，已禁止自动重试",
        false,
      ));
    }
    Err(error) => {
      mark_checkpoint_failed_with_retryable(
        connection,
        fence,
        &checkpoint_id,
        &serialized_error_code(&error.code),
        &error.message,
        error.retryable,
      )?;
      return Err(error);
    }
  };
  let response_received_at = Utc::now().to_rfc3339();
  let persisted = match persist_worker_page(
    root_path,
    PersistCollectionPageInput {
      task_id: step.task_id.clone(),
      task_run_id: run_id.to_string(),
      platform: step.platform.clone(),
      data_type: step.data_type.clone(),
      records: page.records.clone(),
      collected_at: Some(response_received_at.clone()),
    },
    fence,
  ) {
    Ok(persisted) => persisted,
    Err(error) => {
      mark_checkpoint_uncertain(connection, fence, &checkpoint_id, &error.message)?;
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
  let account_result = persist_step_accounts(
    connection,
    step,
    run_id,
    fence,
    &page.records,
    Some(&response_received_at),
  )?;
  if step.schema_version >= 4
    && step.depends_on_step_key.is_none()
    && !page.records.is_empty()
    && account_result.observed_count == 0
    && account_result.skipped_count == page.records.len()
  {
    const ERROR_CODE: &str = "ACCOUNT_IDENTITY_CONTRACT_FAILED";
    const ERROR_MESSAGE: &str =
      "TikHub 主来源返回了记录，但所有记录都缺少平台用户 ID 和可用账号标识";
    mark_checkpoint_failed(connection, fence, &checkpoint_id, ERROR_CODE, ERROR_MESSAGE)?;
    target.request_count += 1;
    let cursor = target.cursor.clone();
    update_target(connection, fence, target, "failed", cursor)?;
    return Err(worker_error(ERROR_CODE, ERROR_MESSAGE, false));
  }

  let raw_response = page.raw_response.to_string();
  let response_hash = format!("{:x}", Sha256::digest(raw_response.as_bytes()));
  let response_size =
    i64::try_from(raw_response.len()).map_err(|_| task_error("TikHub 响应体大小超出数据库范围"))?;
  let next_cursor_json = page.next_cursor.as_ref().map(Value::to_string);
  let input_cursor_json = target.cursor.as_ref().map(Value::to_string);
  mark_checkpoint_response_received(
    connection,
    fence,
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
  mark_checkpoint_completed(connection, fence, &checkpoint_id, &Utc::now().to_rfc3339())?;

  target.request_count += 1;
  if page.has_more && target.request_count < step.request_limit {
    update_target(connection, fence, target, "pending", page.next_cursor)?;
  } else if page.has_more {
    update_target(connection, fence, target, "exhausted", page.next_cursor)?;
  } else {
    update_target(connection, fence, target, "success", None)?;
  }
  Ok(())
}

fn is_isolated_target_failure(error: &AppError) -> bool {
  error.code == AppErrorCode::TikhubRequestError && !error.retryable
}

struct TargetFailureContext<'a> {
  step: &'a RunStep,
  run_id: &'a str,
  checkpoint_id: &'a str,
  request: &'a TikHubCollectionRequest,
  error: &'a AppError,
}

fn persist_target_failure(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  target: &mut PipelineTarget,
  context: TargetFailureContext<'_>,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let error_code = serde_json::to_string(&context.error.code)
    .unwrap_or_else(|_| "\"TIKHUB_REQUEST_ERROR\"".to_string())
    .trim_matches('"')
    .to_string();
  let quote_json = checkpoint_quote_json(connection, context.run_id, context.request)?;
  let request_count = target
    .request_count
    .checked_add(1)
    .ok_or_else(|| task_error("逐目标请求次数溢出"))?;
  let endpoint_key = format!("{}.{}", context.step.platform, context.step.data_type);
  with_fenced_write(connection, fence, |connection| {
    let checkpoint_changed = connection
      .execute(
        "UPDATE collection_page_checkpoint
         SET status = 'failed', retryable = 0, last_error_code = ?1,
             last_error_message = ?2, cost_actual_json = ?3,
             committed_at = ?4, updated_at = ?4
         WHERE id = ?5 AND status = 'requesting'",
        params![
          error_code,
          context.error.message,
          quote_json,
          now,
          context.checkpoint_id
        ],
      )
      .map_err(database_error)?;
    if checkpoint_changed != 1 {
      return Err(task_error("逐目标失败检查点无法形成确定终态"));
    }
    let target_changed = connection
      .execute(
        "UPDATE collection_pipeline_target
         SET status = 'failed', request_count = ?1, updated_at = ?2
         WHERE id = ?3 AND status IN ('pending', 'running')",
        params![request_count, now, target.id],
      )
      .map_err(database_error)?;
    if target_changed != 1 {
      return Err(task_error("逐目标失败状态无法形成确定终态"));
    }
    connection
      .execute(
        "INSERT INTO collection_failure_evidence (
           id, task_run_id, target_id, step_key, endpoint_key, target_key,
           error_code, error_message, retryable, evidence_json, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)",
        params![
          Uuid::new_v4().to_string(),
          context.run_id,
          target.id,
          context.step.step_key,
          endpoint_key,
          target.target_key,
          error_code,
          context.error.message,
          serde_json::json!({
            "checkpoint_id": context.checkpoint_id,
            "candidate_paths": context.request.paths(),
            "source_params": context.request.source_params(),
            "billing": serde_json::from_str::<Value>(&quote_json).unwrap_or(Value::Null)
          })
          .to_string(),
          now
        ],
      )
      .map_err(database_error)?;
    Ok(())
  })?;
  target.request_count = request_count;
  target.status = "failed".to_string();
  Ok(())
}

fn update_target(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  target: &mut PipelineTarget,
  status: &str,
  cursor: Option<Value>,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  with_fenced_write(connection, fence, |connection| {
    let changed = connection
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
    if changed != 1 {
      return Err(task_error("采集流水线目标状态已变化"));
    }
    Ok(())
  })?;
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

fn stop_remaining_targets(
  connection: &Connection,
  fence: Option<&WorkerFence>,
  run_id: &str,
  step_key: &str,
) -> AppResult<()> {
  with_fenced_write(connection, fence, |connection| {
    connection
      .execute(
        "UPDATE collection_pipeline_target
         SET status = 'exhausted', updated_at = ?1
         WHERE task_run_id = ?2 AND step_key = ?3 AND status IN ('pending', 'running')",
        params![Utc::now().to_rfc3339(), run_id, step_key],
      )
      .map_err(database_error)?;
    Ok(())
  })
}

#[cfg(test)]
mod tests {
  use std::cell::Cell;
  use std::os::unix::fs::{symlink, PermissionsExt};

  use super::*;
  use crate::tasks::test_support::install_successful_tikhub_profile;
  use crate::tasks::{
    cancel_task, claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task,
    save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
  };
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

  #[test]
  fn pipeline_cancellation_committed_by_request_guard_prevents_dispatch() {
    assert_guard_cancellation_prevents_dispatch(true);
  }

  #[test]
  fn legacy_cancellation_committed_by_request_guard_prevents_dispatch() {
    assert_guard_cancellation_prevents_dispatch(false);
  }

  #[test]
  fn dispatch_gate_uses_a_private_hashed_regular_file() {
    let root = std::env::temp_dir().join(format!("worker-dispatch-lock-{}", uuid::Uuid::new_v4()));
    create_workspace("请求分发锁文件测试", &root).expect("workspace should be created");

    super::super::with_task_dispatch_gate(&root, "sensitive-task-id", true, || Ok(()))
      .expect("dispatch gate should lock");

    let entries = std::fs::read_dir(root.join("temp"))
      .expect("temp directory should read")
      .collect::<Result<Vec<_>, _>>()
      .expect("lock entries should read");
    assert_eq!(entries.len(), 1);
    let file_name = entries[0].file_name().to_string_lossy().into_owned();
    assert!(file_name.starts_with("task-dispatch-"));
    assert!(file_name.ends_with(".lock"));
    assert!(!file_name.contains("sensitive-task-id"));
    assert_eq!(
      entries[0]
        .metadata()
        .expect("lock metadata should read")
        .permissions()
        .mode()
        & 0o7777,
      0o600
    );

    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn dispatch_gate_rejects_a_preexisting_symlink() {
    let root =
      std::env::temp_dir().join(format!("worker-dispatch-symlink-{}", uuid::Uuid::new_v4()));
    create_workspace("请求分发锁符号链接测试", &root).expect("workspace should be created");
    let task_id = "symlink-task";
    let task_hash = format!("{:x}", Sha256::digest(task_id.as_bytes()));
    let sentinel = root.join("sentinel");
    std::fs::write(&sentinel, b"do-not-follow").expect("sentinel should write");
    symlink(
      &sentinel,
      root
        .join("temp")
        .join(format!("task-dispatch-{task_hash}.lock")),
    )
    .expect("symlink fixture should create");

    let error = super::super::with_task_dispatch_gate(&root, task_id, true, || Ok(()))
      .expect_err("dispatch gate must reject symlink lock paths");

    assert_eq!(error.code, AppErrorCode::PermissionError);
    assert_eq!(
      std::fs::read(&sentinel).expect("sentinel should remain readable"),
      b"do-not-follow"
    );
    std::fs::remove_dir_all(root).ok();
  }

  fn assert_guard_cancellation_prevents_dispatch(pipeline: bool) {
    let root = std::env::temp_dir().join(format!(
      "worker-guard-cancel-{}-{}",
      if pipeline { "pipeline" } else { "legacy" },
      uuid::Uuid::new_v4()
    ));
    create_workspace("请求分发取消栅栏测试", &root).expect("workspace should be created");
    install_successful_tikhub_profile(&root).expect("TikHub profile should install");
    let task = create_collection_task(
      &root,
      CreateCollectionTaskInput {
        name: "取消后不得发送新请求".to_string(),
        source_type: "form".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["comments".to_string()],
      },
    )
    .expect("task should create");
    let plan_input = if pipeline {
      let draft = crate::collection::generate_form_collection_plan(
        crate::collection::FormCollectionPlanRequest {
          platform: "tiktok".to_string(),
          data_type: None,
          data_types: vec!["comments".to_string()],
          params: serde_json::json!({ "item_id": "video-cancel-before-dispatch" }),
          age_range: None,
          request_limit: Some(1),
          record_limit: Some(1),
          budget_limit_micros: Some(1_000_000),
        },
      )
      .expect("pipeline plan should generate");
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: draft.source,
        plan_json: draft.plan_json,
        validation_status: draft.validation_status,
        validation_errors_json: Some(draft.validation_errors_json),
        cost_estimate_json: Some(draft.cost_estimate_json),
      }
    } else {
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: "form_generated".to_string(),
        plan_json: serde_json::json!({
          "platforms": ["tiktok"],
          "data_types": ["comments"],
          "region": null,
          "time_range": null,
          "steps": [{
            "endpoint_key": "tiktok.comments",
            "platform": "tiktok",
            "data_type": "comments",
            "params": {"item_id": "video-cancel-before-dispatch"}
          }],
          "record_limit": 1,
          "request_limit": 1,
          "budget_limit": {"currency": "USD", "amount_micros": 1_000_000},
          "missing_fields": [],
          "requires_user_confirmation": true
        }),
        validation_status: "valid".to_string(),
        validation_errors_json: None,
        cost_estimate_json: None,
      }
    };
    let plan = save_collection_plan(&root, plan_input).expect("plan should save");
    assert_eq!(plan.schema_version >= 3, pipeline);
    confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should confirm");
    enqueue_task(&root, &task.id).expect("task should enqueue");
    let run = claim_next_task(&root)
      .expect("task claim should succeed")
      .expect("queued task should exist");
    let lease_connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let now = chrono::Utc::now();
    lease_connection
      .execute(
        "INSERT INTO task_worker_lease (
           id, owner_id, lease_expires_at, created_at, updated_at, generation
         ) VALUES ('task_worker', 'current-owner', ?1, ?2, ?2, 1)",
        rusqlite::params![now.timestamp_millis() + 120_000, now.to_rfc3339()],
      )
      .expect("current lease should install");
    drop(lease_connection);
    let current =
      WorkerFence::new("current-owner".to_string(), 1).expect("current fence should construct");
    let fetch_calls = Cell::new(0);

    let error = super::super::execute_claimed_run_with_guard(
      &root,
      &run,
      Some(&current),
      |_| {
        cancel_task(&root, &task.id).expect("request guard should commit cancellation");
        Ok(())
      },
      |_request| {
        fetch_calls.set(fetch_calls.get() + 1);
        Ok(CollectionPage {
          records: Vec::new(),
          next_cursor: None,
          has_more: false,
          raw_response: serde_json::json!({ "code": 200, "data": [] }),
        })
      },
    )
    .expect_err("a committed cancellation must stop the request before dispatch");

    assert_eq!(error.code, AppErrorCode::Cancelled);
    assert_eq!(
      fetch_calls.get(),
      0,
      "no provider request may be dispatched"
    );
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let states = connection
      .query_row(
        "SELECT
           (SELECT COUNT(*) FROM collection_page_checkpoint AS checkpoint
            JOIN task_run_step AS run_step
              ON run_step.id = checkpoint.task_run_step_id
            WHERE run_step.task_run_id = ?1 AND checkpoint.status = 'requesting'),
           (SELECT COUNT(*) FROM collection_pipeline_target
            WHERE task_run_id = ?1 AND status = 'running')",
        [&run.id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
      )
      .expect("dispatch side-effect states should query");
    assert_eq!(states, (0, 0));

    drop(connection);
    std::fs::remove_dir_all(root).ok();
  }
}
