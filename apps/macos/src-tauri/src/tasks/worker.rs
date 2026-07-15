use std::path::Path;

use chrono::Utc;
use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::records::{persist_collection_page, PersistCollectionPageInput};
use crate::secrets::read_secret_for_backend;
use crate::tikhub::{
  build_collection_request, send_collection_request, CollectionPage, TikHubCollectionRequest,
};

use super::{
  claim_next_task, complete_task_run, database_error, fail_task_run, open_workspace_connection,
  task_error, TaskRunView,
};

struct RunStep {
  id: String,
  task_id: String,
  platform: String,
  data_type: String,
  params: Value,
  request_limit: i64,
  status: String,
}

struct Checkpoint {
  page_index: i64,
  status: String,
  next_cursor: Option<Value>,
  has_more: Option<bool>,
}

pub fn execute_next_task(root_path: impl AsRef<Path>) -> AppResult<Option<TaskRunView>> {
  let root_path = root_path.as_ref();
  let Some(run) = claim_next_task(root_path)? else {
    return Ok(None);
  };

  let result = execute_claimed_run(root_path, &run);
  match result {
    Ok(()) => complete_task_run(root_path, &run.id, Value::Null).map(Some),
    Err(error) => {
      let error_code = error
        .safe_details
        .get("worker_code")
        .cloned()
        .unwrap_or_else(|| serialized_error_code(&error.code));
      fail_task_run(
        root_path,
        &run.id,
        &error_code,
        &error.message,
        error.retryable,
      )
      .map(Some)
    }
  }
}

fn execute_claimed_run(root_path: &Path, run: &TaskRunView) -> AppResult<()> {
  let connector = crate::tikhub::get_tikhub_connector(root_path)?.ok_or_else(|| {
    worker_error(
      "TIKHUB_CONNECTOR_NOT_READY",
      "尚未配置 TikHub 连接器",
      false,
    )
  })?;
  if !connector.enabled {
    return Err(worker_error(
      "TIKHUB_CONNECTOR_NOT_READY",
      "TikHub 连接器尚未启用",
      false,
    ));
  }
  let Some(secret_ref_id) = connector.secret_ref_id.as_deref() else {
    return Err(worker_error(
      "TIKHUB_CONNECTOR_NOT_READY",
      "TikHub 连接器缺少密钥引用",
      false,
    ));
  };
  if connector.last_test_status.as_deref() != Some("success") {
    return Err(worker_error(
      "TIKHUB_CONNECTOR_NOT_READY",
      "TikHub 连接器尚未通过最新连通性测试",
      false,
    ));
  }
  let token = read_secret_for_backend(root_path, secret_ref_id, "tikhub")?;
  execute_claimed_run_with_fetcher(root_path, run, |request| {
    send_collection_request(Some(connector.base_url.clone()), &token, request)
  })
}

fn execute_claimed_run_with_fetcher<F>(
  root_path: &Path,
  run: &TaskRunView,
  fetch_page: F,
) -> AppResult<()>
where
  F: Fn(&TikHubCollectionRequest) -> AppResult<CollectionPage>,
{
  let connection = open_workspace_connection(root_path)?;
  let steps = load_run_steps(&connection, run)?;
  if steps.is_empty() {
    return Err(task_error("运行记录没有可执行步骤"));
  }

  for step in steps {
    execute_step(root_path, &step, &fetch_page)?;
  }
  Ok(())
}

fn load_run_steps(connection: &rusqlite::Connection, run: &TaskRunView) -> AppResult<Vec<RunStep>> {
  let mut statement = connection
    .prepare(
      "SELECT run_step.id, task_run.task_id, api_step.platform, api_step.data_type,
              api_step.params_json, api_step.request_count_estimate, run_step.status
       FROM task_run_step AS run_step
       JOIN task_run ON task_run.id = run_step.task_run_id
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
        status: row.get(6)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn execute_step<F>(root_path: &Path, step: &RunStep, fetch_page: &F) -> AppResult<()>
where
  F: Fn(&TikHubCollectionRequest) -> AppResult<CollectionPage>,
{
  let connection = open_workspace_connection(root_path)?;
  let existing = load_checkpoints(&connection, &step.id)?;
  let (mut page_index, mut cursor) = resume_position(&existing)?;
  if step.status == "success" {
    if existing
      .last()
      .is_some_and(|checkpoint| checkpoint.has_more == Some(false))
    {
      return Ok(());
    }
    return Err(task_error("已成功步骤缺少完整的结束检查点"));
  }
  if existing
    .iter()
    .any(|checkpoint| checkpoint.status != "completed")
  {
    return Err(worker_error(
      "CHECKPOINT_STATE_UNSUPPORTED",
      "运行步骤存在未完成检查点，请先执行恢复流程",
      false,
    ));
  }

  let task_id = step.task_id.clone();
  let run_id = connection
    .query_row(
      "SELECT task_run_id FROM task_run_step WHERE id = ?1",
      params![step.id],
      |row| row.get::<_, String>(0),
    )
    .map_err(database_error)?;
  mark_step_running(&connection, &step.id)?;

  loop {
    if page_index >= step.request_limit {
      return Err(worker_error(
        "REQUEST_LIMIT_REACHED",
        "TikHub 续页请求次数达到计划上限",
        false,
      ));
    }
    let request = build_collection_request(
      &step.platform,
      &step.data_type,
      &step.params,
      cursor.as_ref(),
    )?;
    let checkpoint_id =
      insert_prepared_checkpoint(&connection, &step.id, page_index, cursor.as_ref())?;
    let requested_at = Utc::now().to_rfc3339();
    mark_checkpoint_requesting(&connection, &checkpoint_id, &requested_at)?;

    let page = match fetch_page(&request) {
      Ok(page) => page,
      Err(error) => {
        mark_checkpoint_uncertain(&connection, &checkpoint_id, &error.message)?;
        return Err(worker_error(
          "UNCERTAIN_REQUEST_AFTER_FAILURE",
          "TikHub 请求已发出但响应状态不确定，已禁止自动重试",
          false,
        ));
      }
    };
    let response_received_at = Utc::now().to_rfc3339();
    let persisted = match persist_collection_page(
      root_path,
      PersistCollectionPageInput {
        task_id: task_id.clone(),
        task_run_id: run_id.clone(),
        platform: step.platform.clone(),
        data_type: step.data_type.clone(),
        records: page.records.clone(),
        collected_at: Some(response_received_at.clone()),
      },
    ) {
      Ok(persisted) => persisted,
      Err(error) => {
        mark_checkpoint_uncertain(&connection, &checkpoint_id, &error.message)?;
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

    let raw_response = page.raw_response.to_string();
    let response_hash = format!("{:x}", Sha256::digest(raw_response.as_bytes()));
    let response_size = i64::try_from(raw_response.len())
      .map_err(|_| task_error("TikHub 响应体大小超出数据库范围"))?;
    let next_cursor_json = page.next_cursor.as_ref().map(Value::to_string);
    let input_cursor_json = cursor.as_ref().map(Value::to_string);
    let cost_actual_json = serde_json::json!({
      "currency": "USD",
      "amount_micros": 0
    })
    .to_string();
    mark_checkpoint_response_received(
      &connection,
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
    mark_checkpoint_completed(&connection, &checkpoint_id, &committed_at)?;

    if !page.has_more {
      mark_step_success(&connection, &step.id, &committed_at)?;
      return Ok(());
    }
    cursor = page.next_cursor;
    page_index += 1;
  }
}

fn resume_position(checkpoints: &[Checkpoint]) -> AppResult<(i64, Option<Value>)> {
  for (index, checkpoint) in checkpoints.iter().enumerate() {
    if checkpoint.page_index != index as i64 || checkpoint.status != "completed" {
      return Err(worker_error(
        "CHECKPOINT_CHAIN_INVALID",
        "运行步骤检查点链不连续，已停止执行",
        false,
      ));
    }
    if index + 1 < checkpoints.len() && checkpoint.has_more != Some(true) {
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
  if last.has_more == Some(false) {
    return Ok((last.page_index + 1, None));
  }
  if last.has_more != Some(true) || last.next_cursor.is_none() {
    return Err(worker_error(
      "CHECKPOINT_CHAIN_INVALID",
      "续页检查点缺少有效游标",
      false,
    ));
  }
  Ok((last.page_index + 1, last.next_cursor.clone()))
}

fn load_checkpoints(
  connection: &rusqlite::Connection,
  run_step_id: &str,
) -> AppResult<Vec<Checkpoint>> {
  let mut statement = connection
    .prepare(
      "SELECT id, page_index, status, next_cursor_json, has_more
       FROM collection_page_checkpoint
       WHERE task_run_step_id = ?1
       ORDER BY page_index, id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_step_id], |row| {
      let next_cursor: Option<String> = row.get(3)?;
      Ok(Checkpoint {
        page_index: row.get(1)?,
        status: row.get(2)?,
        next_cursor: next_cursor
          .map(|value| serde_json::from_str(&value))
          .transpose()
          .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
              3,
              rusqlite::types::Type::Text,
              Box::new(error),
            )
          })?,
        has_more: row.get::<_, Option<i64>>(4)?.map(|value| value != 0),
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn mark_step_running(connection: &rusqlite::Connection, step_id: &str) -> AppResult<()> {
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

fn mark_step_success(connection: &rusqlite::Connection, step_id: &str, now: &str) -> AppResult<()> {
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

fn insert_prepared_checkpoint(
  connection: &rusqlite::Connection,
  run_step_id: &str,
  page_index: i64,
  cursor: Option<&Value>,
) -> AppResult<String> {
  let checkpoint_id = Uuid::new_v4().to_string();
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
        Uuid::new_v4().to_string(),
        cursor.map(Value::to_string),
        now
      ],
    )
    .map_err(database_error)?;
  Ok(checkpoint_id)
}

fn mark_checkpoint_requesting(
  connection: &rusqlite::Connection,
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

fn mark_checkpoint_uncertain(
  connection: &rusqlite::Connection,
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

#[allow(clippy::too_many_arguments)]
fn mark_checkpoint_response_received(
  connection: &rusqlite::Connection,
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

fn mark_checkpoint_completed(
  connection: &rusqlite::Connection,
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

fn worker_error(code: &str, message: &str, retryable: bool) -> AppError {
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::tasks::{
    claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task,
    save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
  };
  use crate::workspace::create_workspace;
  use serde_json::json;
  use uuid::Uuid;

  #[test]
  fn worker_tick_does_not_leave_a_queued_task_unprocessed() {
    let root = std::env::temp_dir().join(format!("worker-{}", Uuid::new_v4()));
    create_workspace("执行器测试", &root).expect("workspace should be created");
    let task = create_collection_task(
      &root,
      CreateCollectionTaskInput {
        name: "无连接器任务".to_string(),
        source_type: "form".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["item_detail".to_string()],
      },
    )
    .expect("task should be created");
    let plan = save_collection_plan(
      &root,
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: "form_generated".to_string(),
        plan_json: json!({
          "platforms": ["tiktok"],
          "data_types": ["item_detail"],
          "region": null,
          "time_range": null,
          "steps": [{
            "endpoint_key": "tiktok.item_detail",
            "platform": "tiktok",
            "data_type": "item_detail",
            "params": {"item_id": "video-1"}
          }],
          "record_limit": 1,
          "request_limit": 1,
          "budget_limit": {"currency": "USD", "amount_micros": 35000000},
          "missing_fields": [],
          "requires_user_confirmation": true
        }),
        validation_status: "valid".to_string(),
        validation_errors_json: None,
        cost_estimate_json: None,
      },
    )
    .expect("plan should be saved");
    confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should be confirmed");
    enqueue_task(&root, &task.id).expect("task should be queued");

    let run = execute_next_task(&root)
      .expect("worker tick should complete its state transition")
      .expect("worker should claim the queued task");

    assert_eq!(run.status, "failed");
    assert_eq!(
      run.error_code.as_deref(),
      Some("TIKHUB_CONNECTOR_NOT_READY")
    );
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn worker_persists_a_page_and_completes_the_run() {
    let root = std::env::temp_dir().join(format!("worker-success-{}", Uuid::new_v4()));
    create_workspace("执行器成功测试", &root).expect("workspace should be created");
    let task = create_collection_task(
      &root,
      CreateCollectionTaskInput {
        name: "单页任务".to_string(),
        source_type: "form".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["item_detail".to_string()],
      },
    )
    .expect("task should be created");
    let plan = save_collection_plan(
      &root,
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: "form_generated".to_string(),
        plan_json: json!({
          "platforms": ["tiktok"],
          "data_types": ["item_detail"],
          "region": null,
          "time_range": null,
          "steps": [{
            "endpoint_key": "tiktok.item_detail",
            "platform": "tiktok",
            "data_type": "item_detail",
            "params": {"item_id": "video-1"}
          }],
          "record_limit": 1,
          "request_limit": 1,
          "budget_limit": {"currency": "USD", "amount_micros": 35000000},
          "missing_fields": [],
          "requires_user_confirmation": true
        }),
        validation_status: "valid".to_string(),
        validation_errors_json: None,
        cost_estimate_json: None,
      },
    )
    .expect("plan should be saved");
    confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should be confirmed");
    enqueue_task(&root, &task.id).expect("task should be queued");
    let run = claim_next_task(&root)
      .expect("worker should claim the task")
      .expect("queued task should exist");

    execute_claimed_run_with_fetcher(&root, &run, |_request| {
      Ok(CollectionPage {
        records: vec![json!({"aweme_id": "video-1", "desc": "test"})],
        next_cursor: None,
        has_more: false,
        raw_response: json!({
          "code": 200,
          "data": {"aweme_id": "video-1", "desc": "test"}
        }),
      })
    })
    .expect("page should execute");
    let completed = complete_task_run(&root, &run.id, Value::Null)
      .expect("run should complete from checkpoint evidence");

    assert_eq!(completed.status, "success");
    let connection = super::open_workspace_connection(&root).expect("database should open");
    let checkpoint: (String, i64, i64) = connection
      .query_row(
        "SELECT status, record_count_received, record_count_persisted
         FROM collection_page_checkpoint",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
      )
      .expect("checkpoint should be persisted");
    assert_eq!(checkpoint, ("completed".to_string(), 1, 1));
    let task_status: String = connection
      .query_row(
        "SELECT status FROM collection_task WHERE id = ?1",
        [&task.id],
        |row| row.get(0),
      )
      .expect("task should be readable");
    assert_eq!(task_status, "success");
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn worker_marks_checkpoint_uncertain_when_record_persistence_fails() {
    let root = std::env::temp_dir().join(format!("worker-persist-failure-{}", Uuid::new_v4()));
    create_workspace("执行器落库失败测试", &root).expect("workspace should be created");
    let (task, plan) = create_confirmed_item_detail_task(&root);
    enqueue_task(&root, &task.id).expect("task should be queued");
    let run = claim_next_task(&root)
      .expect("worker should claim the task")
      .expect("queued task should exist");

    execute_claimed_run_with_fetcher(&root, &run, |_request| {
      Ok(CollectionPage {
        records: vec![json!({"desc": "missing id"})],
        next_cursor: None,
        has_more: false,
        raw_response: json!({
          "code": 200,
          "data": {"desc": "missing id"}
        }),
      })
    })
    .expect_err("invalid records must fail the worker");

    let connection = super::open_workspace_connection(&root).expect("database should open");
    let checkpoint_status: String = connection
      .query_row(
        "SELECT status FROM collection_page_checkpoint
         WHERE task_run_step_id IN (SELECT id FROM task_run_step WHERE task_run_id = ?1)",
        [&run.id],
        |row| row.get(0),
      )
      .expect("checkpoint should be persisted");
    assert_eq!(checkpoint_status, "uncertain");
    let _ = plan;
    std::fs::remove_dir_all(root).ok();
  }

  fn create_confirmed_item_detail_task(
    root: &std::path::Path,
  ) -> (
    crate::tasks::CollectionTaskView,
    crate::tasks::CollectionPlanView,
  ) {
    let task = create_collection_task(
      root,
      CreateCollectionTaskInput {
        name: "单页任务".to_string(),
        source_type: "form".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["item_detail".to_string()],
      },
    )
    .expect("task should be created");
    let plan = save_collection_plan(
      root,
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: "form_generated".to_string(),
        plan_json: json!({
          "platforms": ["tiktok"],
          "data_types": ["item_detail"],
          "region": null,
          "time_range": null,
          "steps": [{
            "endpoint_key": "tiktok.item_detail",
            "platform": "tiktok",
            "data_type": "item_detail",
            "params": {"item_id": "video-1"}
          }],
          "record_limit": 1,
          "request_limit": 1,
          "budget_limit": {"currency": "USD", "amount_micros": 35000000},
          "missing_fields": [],
          "requires_user_confirmation": true
        }),
        validation_status: "valid".to_string(),
        validation_errors_json: None,
        cost_estimate_json: None,
      },
    )
    .expect("plan should be saved");
    confirm_collection_plan(root, &task.id, &plan.id).expect("plan should be confirmed");
    (task, plan)
  }
}
