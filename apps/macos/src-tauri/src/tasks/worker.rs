use std::path::Path;

use chrono::Utc;
use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::accounts::{
  persist_account_observations, persist_account_observations_with_fence, AccountObservationInput,
  AccountPersistenceResult, AgeRange,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::records::{
  persist_collection_page, persist_collection_page_with_fence, PersistCollectionPageInput,
  PersistCollectionPageResult,
};
use crate::secrets::read_secret_for_snapshot;
use crate::tikhub::{
  build_collection_request, send_collection_request, CollectionPage, TikHubCollectionRequest,
};

#[cfg(test)]
use super::claim_next_task;
use super::execution::{
  complete_task_run_with_fence, fail_task_run_with_safe_details,
  fail_task_run_with_safe_details_with_fence,
};
use super::{
  claim_next_task_with_fence, complete_task_run, database_error, get_task_run,
  open_workspace_connection, task_error, TaskRunView, WorkerFence,
};

mod mutations;
mod pipeline;
mod pricing;
mod recovery;
mod runtime;
mod targets;
use mutations::{
  insert_prepared_checkpoint, mark_checkpoint_completed, mark_checkpoint_failed,
  mark_checkpoint_failed_with_retryable, mark_checkpoint_requesting,
  mark_checkpoint_response_received, mark_checkpoint_uncertain, mark_response_checkpoint_completed,
  mark_step_running, mark_step_stopped, mark_step_success,
};
use recovery::{ensure_record_limit, load_checkpoints, parse_response_checkpoint, resume_position};
use runtime::load_runtime_snapshot;

pub(super) struct RunStep {
  pub(super) id: String,
  pub(super) task_id: String,
  pub(super) platform: String,
  pub(super) data_type: String,
  pub(super) params: Value,
  pub(super) request_limit: i64,
  pub(super) record_limit: i64,
  pub(super) status: String,
  pub(super) schema_version: i64,
  pub(super) output_selected: bool,
  pub(super) age_range: Option<AgeRange>,
  pub(super) step_key: String,
  pub(super) depends_on_step_key: Option<String>,
  pub(super) input_binding: Option<String>,
  pub(super) dependency_data_type: Option<String>,
}

#[cfg(test)]
pub fn execute_next_task(root_path: impl AsRef<Path>) -> AppResult<Option<TaskRunView>> {
  let root_path = root_path.as_ref();
  let Some(run) = claim_next_task(root_path)? else {
    return Ok(None);
  };
  execute_and_finalize_claimed_run(root_path, run, None, || Ok(()))
}

pub(super) fn execute_next_task_with_owner<G>(
  root_path: impl AsRef<Path>,
  fence: &WorkerFence,
  ensure_owner: G,
) -> AppResult<Option<TaskRunView>>
where
  G: Fn() -> AppResult<()>,
{
  let root_path = root_path.as_ref();
  ensure_owner()?;
  let Some(run) = claim_next_task_with_fence(root_path, fence)? else {
    return Ok(None);
  };
  execute_and_finalize_claimed_run(root_path, run, Some(fence), ensure_owner)
}

fn execute_and_finalize_claimed_run<G>(
  root_path: &Path,
  run: TaskRunView,
  fence: Option<&WorkerFence>,
  ensure_owner: G,
) -> AppResult<Option<TaskRunView>>
where
  G: Fn() -> AppResult<()>,
{
  let result = execute_claimed_run(root_path, &run, fence, &ensure_owner);
  ensure_owner()?;
  finalize_claimed_run(root_path, &run, fence, result).map(Some)
}

fn finalize_claimed_run(
  root_path: &Path,
  run: &TaskRunView,
  fence: Option<&WorkerFence>,
  result: AppResult<()>,
) -> AppResult<TaskRunView> {
  match result {
    Ok(()) => match fence {
      Some(fence) => complete_task_run_with_fence(root_path, &run.id, Value::Null, fence),
      None => complete_task_run(root_path, &run.id, Value::Null),
    },
    Err(error) if error.code == AppErrorCode::Cancelled => {
      let connection = open_workspace_connection(root_path)?;
      get_task_run(&connection, &run.id)
    }
    Err(error) => {
      let error_code = error
        .safe_details
        .get("worker_code")
        .cloned()
        .unwrap_or_else(|| serialized_error_code(&error.code));
      match fence {
        Some(fence) => fail_task_run_with_safe_details_with_fence(
          root_path,
          &run.id,
          &error_code,
          &error.message,
          error.retryable,
          &error.safe_details,
          fence,
        ),
        None => fail_task_run_with_safe_details(
          root_path,
          &run.id,
          &error_code,
          &error.message,
          error.retryable,
          &error.safe_details,
        ),
      }
    }
  }
}

fn execute_claimed_run<G>(
  root_path: &Path,
  run: &TaskRunView,
  fence: Option<&WorkerFence>,
  ensure_owner: &G,
) -> AppResult<()>
where
  G: Fn() -> AppResult<()>,
{
  let snapshot = load_runtime_snapshot(root_path, &run.id)?;
  let token = load_runtime_token(root_path, &snapshot)?;
  execute_claimed_run_with_guard(
    root_path,
    run,
    fence,
    |request| {
      ensure_owner()?;
      let result = pricing::guard_request(root_path, &run.id, request, fence).map(|_| ());
      ensure_owner()?;
      result
    },
    |request| {
      ensure_owner()?;
      let result = send_collection_request(Some(snapshot.base_url.clone()), &token, request);
      ensure_owner()?;
      result
    },
  )
}

fn load_runtime_token(root_path: &Path, snapshot: &runtime::RuntimeSnapshot) -> AppResult<String> {
  read_secret_for_snapshot(
    root_path,
    &snapshot.secret_ref_id,
    "tikhub",
    &snapshot.secret_provider_id,
    snapshot.secret_revision,
  )
}

#[cfg(test)]
fn execute_claimed_run_with_fetcher<F>(
  root_path: &Path,
  run: &TaskRunView,
  fetch_page: F,
) -> AppResult<()>
where
  F: Fn(&TikHubCollectionRequest) -> AppResult<CollectionPage>,
{
  execute_claimed_run_with_guard(root_path, run, None, |_| Ok(()), fetch_page)
}

fn execute_claimed_run_with_guard<G, F>(
  root_path: &Path,
  run: &TaskRunView,
  fence: Option<&WorkerFence>,
  guard_request: G,
  fetch_page: F,
) -> AppResult<()>
where
  G: Fn(&TikHubCollectionRequest) -> AppResult<()>,
  F: Fn(&TikHubCollectionRequest) -> AppResult<CollectionPage>,
{
  let connection = open_workspace_connection(root_path)?;
  let steps = load_run_steps(&connection, run)?;
  if steps.is_empty() {
    return Err(task_error("运行记录没有可执行步骤"));
  }

  for step in steps {
    execute_step(root_path, &step, fence, &guard_request, &fetch_page)?;
  }
  Ok(())
}

fn load_run_steps(connection: &rusqlite::Connection, run: &TaskRunView) -> AppResult<Vec<RunStep>> {
  let mut statement = connection
    .prepare(
      "SELECT run_step.id, task_run.task_id, api_step.platform, api_step.data_type,
              api_step.params_json, api_step.request_count_estimate,
              json_extract(plan.plan_json, '$.record_limit'), run_step.status,
              plan.schema_version,
              COALESCE(
                json_extract(
                  plan.plan_json,
                  '$.steps[' || api_step.step_order || '].output_selected'
                ),
                1
              ),
              json_extract(plan.plan_json, '$.age_range.min'),
              json_extract(plan.plan_json, '$.age_range.max'),
              COALESCE(
                json_extract(plan.plan_json, '$.steps[' || api_step.step_order || '].step_key'),
                api_step.data_type
              ),
              json_extract(
                plan.plan_json,
                '$.steps[' || api_step.step_order || '].depends_on_step_key'
              ),
              json_extract(
                plan.plan_json,
                '$.steps[' || api_step.step_order || '].input_binding.account_id'
              ),
              (
                SELECT json_extract(dependency.value, '$.data_type')
                FROM json_each(plan.plan_json, '$.steps') AS dependency
                WHERE json_extract(dependency.value, '$.step_key') = json_extract(
                  plan.plan_json,
                  '$.steps[' || api_step.step_order || '].depends_on_step_key'
                )
                LIMIT 1
              )
       FROM task_run_step AS run_step
       JOIN task_run ON task_run.id = run_step.task_run_id
       JOIN collection_plan AS plan ON plan.id = task_run.plan_id
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1
       ORDER BY api_step.step_order, api_step.id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run.id], |row| {
      let params_json: String = row.get(4)?;
      Ok(RunStep {
        id: row.get(0)?,
        task_id: row.get(1)?,
        platform: row.get(2)?,
        data_type: row.get(3)?,
        params: serde_json::from_str(&params_json).map_err(|error| {
          rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
        })?,
        request_limit: row.get(5)?,
        record_limit: row.get(6)?,
        status: row.get(7)?,
        schema_version: row.get(8)?,
        output_selected: row.get::<_, i64>(9)? != 0,
        age_range: row
          .get::<_, Option<i64>>(10)?
          .zip(row.get::<_, Option<i64>>(11)?)
          .map(|(min, max)| AgeRange { min, max }),
        step_key: row.get(12)?,
        depends_on_step_key: row.get(13)?,
        input_binding: row.get(14)?,
        dependency_data_type: row.get(15)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn execute_step<G, F>(
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
  if step.schema_version >= 3 {
    return pipeline::execute_pipeline_step(root_path, step, fence, guard_request, fetch_page);
  }
  let connection = open_workspace_connection(root_path)?;
  let existing = load_checkpoints(&connection, &step.id)?;
  let (mut page_index, mut cursor) = resume_position(&existing)?;
  let mut prepared_checkpoint = existing
    .last()
    .filter(|checkpoint| checkpoint.status == "prepared")
    .cloned();
  let mut response_checkpoint = existing
    .last()
    .filter(|checkpoint| checkpoint.status == "response_received")
    .cloned();
  if step.status == "success" {
    if existing
      .last()
      .is_some_and(|checkpoint| checkpoint.has_more == Some(false))
    {
      return Ok(());
    }
    return Err(task_error("已成功步骤缺少完整的结束检查点"));
  }

  let task_id = step.task_id.clone();
  let run_id = connection
    .query_row(
      "SELECT task_run_id FROM task_run_step WHERE id = ?1",
      params![step.id],
      |row| row.get::<_, String>(0),
    )
    .map_err(database_error)?;
  mark_step_running(&connection, fence, &step.id)?;

  loop {
    if page_index >= step.request_limit {
      return Err(worker_error(
        "REQUEST_LIMIT_REACHED",
        "TikHub 续页请求次数达到计划上限",
        false,
      ));
    }
    if let Some(checkpoint) = response_checkpoint.take() {
      let page = parse_response_checkpoint(step, &checkpoint)?;
      if step.schema_version == 2 {
        ensure_record_limit(&connection, &run_id, step.record_limit, page.records.len())?;
      }
      let persisted = persist_worker_page(
        root_path,
        PersistCollectionPageInput {
          task_id: task_id.clone(),
          task_run_id: run_id.clone(),
          platform: step.platform.clone(),
          data_type: step.data_type.clone(),
          records: page.records.clone(),
          collected_at: checkpoint.response_received_at.clone(),
        },
        fence,
      )?;
      let persisted_count = persisted
        .inserted_count
        .checked_add(persisted.existing_count)
        .ok_or_else(|| task_error("已持久化记录数溢出"))?;
      if persisted_count != page.records.len() {
        return Err(worker_error(
          "RECORD_PERSISTENCE_INCOMPLETE",
          "TikHub 响应未能全部写入本地存储",
          false,
        ));
      }
      persist_step_accounts(
        &connection,
        step,
        &run_id,
        fence,
        &page.records,
        checkpoint.response_received_at.as_deref(),
      )?;
      let committed_at = Utc::now().to_rfc3339();
      mark_response_checkpoint_completed(
        &connection,
        fence,
        &checkpoint.id,
        i64::try_from(persisted_count).map_err(|_| task_error("持久化记录数超出数据库范围"))?,
        &committed_at,
      )?;
      if !page.has_more {
        mark_step_success(&connection, fence, &step.id, &committed_at)?;
        return Ok(());
      }
      cursor = page.next_cursor;
      page_index += 1;
      continue;
    }
    let request = build_collection_request(
      &step.platform,
      &step.data_type,
      &step.params,
      cursor.as_ref(),
    )?;
    guard_request(&request)?;
    let (checkpoint_id, idempotency_key) = if let Some(checkpoint) = prepared_checkpoint.take() {
      if checkpoint.page_index != page_index || checkpoint.input_cursor != cursor {
        return Err(worker_error(
          "CHECKPOINT_CHAIN_INVALID",
          "恢复检查点的页码或游标与当前执行位置不一致",
          false,
        ));
      }
      (checkpoint.id, checkpoint.idempotency_key)
    } else {
      insert_prepared_checkpoint(&connection, fence, &step.id, page_index, cursor.as_ref())?
    };
    let request = request.with_idempotency_key(idempotency_key)?;
    let requested_at = Utc::now().to_rfc3339();
    mark_checkpoint_requesting(&connection, fence, &checkpoint_id, &requested_at)?;

    let page_result = fetch_page(&request);
    ensure_run_accepts_response(root_path, &run_id)?;
    let page = match page_result {
      Ok(page) => page,
      Err(error) if response_status_is_uncertain(&error) => {
        mark_checkpoint_uncertain(&connection, fence, &checkpoint_id, &error.message)?;
        return Err(worker_error(
          "UNCERTAIN_REQUEST_AFTER_FAILURE",
          "TikHub 请求已发出但响应状态不确定，已禁止自动重试",
          false,
        ));
      }
      Err(error) => {
        mark_checkpoint_failed_with_retryable(
          &connection,
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
    if let Err(error) = (step.schema_version == 2)
      .then(|| ensure_record_limit(&connection, &run_id, step.record_limit, page.records.len()))
      .transpose()
    {
      if error.safe_details.get("worker_code").map(String::as_str) == Some("RECORD_LIMIT_REACHED") {
        mark_checkpoint_failed(
          &connection,
          fence,
          &checkpoint_id,
          "RECORD_LIMIT_REACHED",
          "响应记录数将超过已确认的记录上限",
        )?;
      }
      return Err(error);
    }
    let persisted = match persist_worker_page(
      root_path,
      PersistCollectionPageInput {
        task_id: task_id.clone(),
        task_run_id: run_id.clone(),
        platform: step.platform.clone(),
        data_type: step.data_type.clone(),
        records: page.records.clone(),
        collected_at: Some(response_received_at.clone()),
      },
      fence,
    ) {
      Ok(persisted) => persisted,
      Err(error) => {
        mark_checkpoint_uncertain(&connection, fence, &checkpoint_id, &error.message)?;
        return Err(worker_error(
          "RECORD_PERSISTENCE_FAILED",
          "TikHub 响应已返回但记录落库失败，已禁止自动重试",
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
        "TikHub 响应记录未全部写入本地存储",
        false,
      ));
    }
    if let Err(error) = persist_step_accounts(
      &connection,
      step,
      &run_id,
      fence,
      &page.records,
      Some(&response_received_at),
    ) {
      mark_checkpoint_uncertain(&connection, fence, &checkpoint_id, &error.message)?;
      return Err(worker_error(
        "ACCOUNT_PERSISTENCE_FAILED",
        "TikHub 响应已返回但账号合并落库失败，已禁止自动重试",
        false,
      ));
    }

    let raw_response = page.raw_response.to_string();
    let response_hash = format!("{:x}", Sha256::digest(raw_response.as_bytes()));
    let response_size = i64::try_from(raw_response.len())
      .map_err(|_| task_error("TikHub 响应体大小超出数据库范围"))?;
    let next_cursor_json = page.next_cursor.as_ref().map(Value::to_string);
    let input_cursor_json = cursor.as_ref().map(Value::to_string);
    let cost_actual_json = pricing::checkpoint_quote_json(&connection, &run_id, &request)?;
    mark_checkpoint_response_received(
      &connection,
      fence,
      &checkpoint_id,
      &raw_response,
      &response_hash,
      response_size,
      &request,
      input_cursor_json.as_deref(),
      &page,
      persisted_count,
      &cost_actual_json,
      &response_received_at,
      next_cursor_json.as_deref(),
    )?;
    let committed_at = Utc::now().to_rfc3339();
    mark_checkpoint_completed(&connection, fence, &checkpoint_id, &committed_at)?;

    if !page.has_more {
      mark_step_success(&connection, fence, &step.id, &committed_at)?;
      return Ok(());
    }
    cursor = page.next_cursor;
    page_index += 1;
  }
}

pub(super) fn persist_worker_page(
  root_path: &Path,
  input: PersistCollectionPageInput,
  fence: Option<&WorkerFence>,
) -> AppResult<PersistCollectionPageResult> {
  match fence {
    Some(fence) => persist_collection_page_with_fence(root_path, input, fence),
    None => persist_collection_page(root_path, input),
  }
}

pub(super) fn ensure_run_accepts_response(root_path: &Path, run_id: &str) -> AppResult<()> {
  let connection = open_workspace_connection(root_path)?;
  let current = get_task_run(&connection, run_id)?;
  if current.status == "running" {
    return Ok(());
  }
  if current.status == "cancelled" {
    return Err(AppError::new(
      AppErrorCode::Cancelled,
      "任务已取消；已发出的远端请求可能仍已完成并产生费用，返回数据不会写入本地",
      AppErrorStage::Collection,
      false,
    ));
  }
  Err(task_error("任务状态已变化，远端响应不会写入本地"))
}

pub(super) fn persist_step_accounts(
  connection: &rusqlite::Connection,
  step: &RunStep,
  run_id: &str,
  fence: Option<&WorkerFence>,
  records: &[Value],
  collected_at: Option<&str>,
) -> AppResult<AccountPersistenceResult> {
  if step.schema_version < 3 {
    return Ok(AccountPersistenceResult {
      observed_count: 0,
      skipped_count: 0,
      output_count: 0,
    });
  }
  let record_limit =
    usize::try_from(step.record_limit).map_err(|_| task_error("账号输出上限超出运行平台范围"))?;
  let input = AccountObservationInput {
    task_run_id: run_id.to_string(),
    platform: step.platform.clone(),
    data_type: step.data_type.clone(),
    records: records.to_vec(),
    output_selected: step.output_selected,
    age_range: step.age_range,
    record_limit,
    collected_at: collected_at
      .map(ToString::to_string)
      .unwrap_or_else(|| Utc::now().to_rfc3339()),
  };
  match fence {
    Some(fence) => persist_account_observations_with_fence(connection, input, fence),
    None => persist_account_observations(connection, input),
  }
}

pub(super) fn worker_error(code: &str, message: &str, retryable: bool) -> AppError {
  AppError::new(
    AppErrorCode::TikhubRequestError,
    format!("{code}: {message}"),
    AppErrorStage::Collection,
    retryable,
  )
  .with_safe_detail("worker_code", code)
}

fn serialized_error_code(code: &AppErrorCode) -> String {
  serde_json::to_string(code)
    .unwrap_or_else(|_| "WORKER_ERROR".to_string())
    .trim_matches('"')
    .to_string()
}

fn response_status_is_uncertain(error: &AppError) -> bool {
  error
    .safe_details
    .get("response_state")
    .is_some_and(|state| state == "uncertain")
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "worker_v4_tests.rs"]
mod v4_tests;

#[cfg(test)]
#[path = "worker_snapshot_tests.rs"]
mod snapshot_tests;
