use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

mod execution;
mod validation;

pub use execution::{
  cancel_task, claim_next_task, complete_task_run, enqueue_task, fail_task_run,
  recover_interrupted_runs, retry_task,
};
use validation::{estimate_from_plan_json, validate_plan_for_task};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CreateCollectionTaskInput {
  pub name: String,
  pub source_type: String,
  pub platforms: Vec<String>,
  pub data_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UpdateCollectionTaskInput {
  pub name: Option<String>,
  pub platforms: Option<Vec<String>>,
  pub data_types: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SaveCollectionPlanInput {
  pub task_id: String,
  pub source: String,
  pub plan_json: Value,
  pub validation_status: String,
  pub validation_errors_json: Option<Value>,
  pub cost_estimate_json: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionTaskView {
  pub id: String,
  pub name: String,
  pub source_type: String,
  pub status: String,
  pub platforms_json: Value,
  pub data_types_json: Value,
  pub created_at: String,
  pub updated_at: String,
  pub confirmed_at: Option<String>,
  pub completed_at: Option<String>,
  pub cancelled_at: Option<String>,
  pub cost_estimate_json: Value,
  pub actual_cost_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionPlanView {
  pub id: String,
  pub task_id: String,
  pub source: String,
  pub schema_version: i64,
  pub plan_json: Value,
  pub validation_status: String,
  pub validation_errors_json: Value,
  pub cost_estimate_json: Value,
  pub confirmed_by_user: bool,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskRunView {
  pub id: String,
  pub task_id: String,
  pub status: String,
  pub started_at: String,
  pub ended_at: Option<String>,
  pub current_stage: Option<String>,
  pub error_code: Option<String>,
  pub error_message: Option<String>,
  pub retryable: bool,
  pub cost_actual_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskLogView {
  pub id: String,
  pub task_run_id: String,
  pub stage: String,
  pub level: String,
  pub message: String,
  pub safe_details_json: Value,
  pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostEstimateView {
  pub request_count_estimate: i64,
  pub platform_count: i64,
  pub data_type_count: i64,
  pub requires_confirmation: bool,
  pub cost_estimate_json: Value,
}

pub fn create_collection_task(
  root_path: impl AsRef<Path>,
  input: CreateCollectionTaskInput,
) -> AppResult<CollectionTaskView> {
  let connection = open_workspace_connection(root_path)?;
  let input = normalize_create_task_input(input)?;
  let id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();

  connection
    .execute(
      "INSERT INTO collection_task (
        id, name, source_type, status, platforms_json, data_types_json,
        created_at, updated_at, cost_estimate_json, actual_cost_json
      ) VALUES (?1, ?2, ?3, 'draft', ?4, ?5, ?6, ?7, '{}', '{}')",
      params![
        id,
        input.name,
        input.source_type,
        serde_json::to_string(&input.platforms).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string(&input.data_types).unwrap_or_else(|_| "[]".to_string()),
        now,
        now
      ],
    )
    .map_err(database_error)?;

  write_task_audit_log(
    &connection,
    "create_collection_task",
    Some(&id),
    serde_json::json!({}),
  )?;
  get_task_by_id(&connection, &id)
}

pub fn update_collection_task(
  root_path: impl AsRef<Path>,
  task_id: &str,
  input: UpdateCollectionTaskInput,
) -> AppResult<CollectionTaskView> {
  let connection = open_workspace_connection(root_path)?;
  let current = get_task_by_id(&connection, task_id)?;

  if !["draft", "waiting_confirmation"].contains(&current.status.as_str()) {
    return Err(task_error("只允许更新草稿或等待确认状态的任务"));
  }

  let name = input.name.unwrap_or_else(|| current.name.clone());
  let platforms = input
    .platforms
    .map_or_else(|| current.platforms_json.clone(), json_array);
  let data_types = input
    .data_types
    .map_or_else(|| current.data_types_json.clone(), json_array);
  let scope_changed = platforms != current.platforms_json || data_types != current.data_types_json;
  let confirmed_at = if scope_changed {
    None
  } else {
    current.confirmed_at
  };
  let now = Utc::now().to_rfc3339();

  connection
    .execute(
      "UPDATE collection_task
       SET name = ?1, platforms_json = ?2, data_types_json = ?3, confirmed_at = ?4,
           updated_at = ?5
       WHERE id = ?6",
      params![
        normalize_required("name", &name)?,
        platforms.to_string(),
        data_types.to_string(),
        confirmed_at,
        now,
        task_id
      ],
    )
    .map_err(database_error)?;
  if scope_changed {
    connection
      .execute(
        "UPDATE collection_plan
         SET confirmed_by_user = 0, updated_at = ?1
         WHERE task_id = ?2 AND confirmed_by_user = 1",
        params![now, task_id],
      )
      .map_err(database_error)?;
  }

  get_task_by_id(&connection, task_id)
}

pub fn save_collection_plan(
  root_path: impl AsRef<Path>,
  input: SaveCollectionPlanInput,
) -> AppResult<CollectionPlanView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let task = get_task_by_id(&transaction, &input.task_id)?;

  if !["draft", "waiting_confirmation"].contains(&task.status.as_str()) {
    return Err(task_error("只允许给草稿或等待确认状态的任务保存采集计划"));
  }

  let source = normalize_plan_source(&input.source)?;
  let id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  let validation_errors = validate_plan_for_task(&task, &input.plan_json);
  let validation_status = if validation_errors.is_empty() {
    "valid"
  } else {
    "needs_review"
  };
  let validation_errors = serde_json::json!(validation_errors);
  let cost_estimate = estimate_from_plan_json(&input.plan_json).cost_estimate_json;

  transaction
    .execute(
      "UPDATE collection_plan
       SET confirmed_by_user = 0, updated_at = ?1
       WHERE task_id = ?2 AND confirmed_by_user = 1",
      params![now, input.task_id],
    )
    .map_err(database_error)?;

  transaction
    .execute(
      "INSERT INTO collection_plan (
        id, task_id, source, schema_version, plan_json, validation_status,
        validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
      ) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, 0, ?8, ?9)",
      params![
        id,
        input.task_id,
        source,
        input.plan_json.to_string(),
        validation_status,
        validation_errors.to_string(),
        cost_estimate.to_string(),
        now,
        now
      ],
    )
    .map_err(database_error)?;

  persist_api_call_steps(&transaction, &id, &input.plan_json, validation_status, &now)?;

  transaction
    .execute(
      "UPDATE collection_task
       SET status = 'waiting_confirmation', confirmed_at = NULL,
           cost_estimate_json = ?1, updated_at = ?2
       WHERE id = ?3",
      params![cost_estimate.to_string(), now, input.task_id],
    )
    .map_err(database_error)?;

  transaction.commit().map_err(database_error)?;
  get_collection_plan(&connection, &id)
}

fn persist_api_call_steps(
  connection: &Connection,
  plan_id: &str,
  plan_json: &Value,
  validation_status: &str,
  created_at: &str,
) -> AppResult<()> {
  let Some(steps) = plan_json.get("steps").and_then(Value::as_array) else {
    return Ok(());
  };
  let request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .unwrap_or(1)
    .max(1);
  let step_status = if validation_status == "valid" {
    "planned"
  } else {
    "needs_review"
  };

  for (index, step) in steps.iter().enumerate() {
    let Some(step) = step.as_object() else {
      continue;
    };
    let (Some(platform), Some(data_type), Some(endpoint_key)) = (
      step.get("platform").and_then(Value::as_str),
      step.get("data_type").and_then(Value::as_str),
      step.get("endpoint_key").and_then(Value::as_str),
    ) else {
      continue;
    };
    let params_json = step
      .get("params")
      .cloned()
      .unwrap_or_else(|| serde_json::json!({}));
    connection
      .execute(
        "INSERT INTO api_call_step (
          id, plan_id, step_order, platform, data_type, endpoint_key, params_json,
          status, request_count_estimate, cost_estimate_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
          Uuid::new_v4().to_string(),
          plan_id,
          index as i64,
          platform,
          data_type,
          endpoint_key,
          params_json.to_string(),
          step_status,
          request_limit,
          serde_json::json!({ "request_count_estimate": request_limit }).to_string(),
          created_at
        ],
      )
      .map_err(database_error)?;
  }
  Ok(())
}

pub fn estimate_task_cost(
  root_path: impl AsRef<Path>,
  task_id: Option<String>,
  plan_json: Option<Value>,
) -> AppResult<CostEstimateView> {
  let connection = open_workspace_connection(root_path)?;

  if let Some(plan_json) = plan_json {
    return Ok(estimate_from_plan_json(&plan_json));
  }

  if let Some(task_id) = task_id {
    let plan = latest_plan_for_task(&connection, &task_id)?;
    return Ok(estimate_from_plan_json(&plan.plan_json));
  }

  Err(task_error("需要 task_id 或 plan_json 才能估算成本"))
}

pub fn confirm_collection_plan(
  root_path: impl AsRef<Path>,
  task_id: &str,
  plan_id: &str,
) -> AppResult<CollectionTaskView> {
  let connection = open_workspace_connection(root_path)?;
  let plan = get_collection_plan(&connection, plan_id)?;
  let task = get_task_by_id(&connection, task_id)?;

  if plan.task_id != task_id {
    return Err(task_error("采集计划不属于当前任务"));
  }

  if latest_plan_for_task(&connection, task_id)?.id != plan.id {
    return Err(task_error("只能确认当前任务的最新采集计划"));
  }

  let validation_errors = validate_plan_for_task(&task, &plan.plan_json);
  if !validation_errors.is_empty() {
    connection
      .execute(
        "UPDATE collection_plan
         SET validation_status = 'needs_review', validation_errors_json = ?1,
             confirmed_by_user = 0, updated_at = ?2
         WHERE id = ?3",
        params![
          serde_json::json!(validation_errors).to_string(),
          Utc::now().to_rfc3339(),
          plan_id
        ],
      )
      .map_err(database_error)?;
    return Err(task_error("采集计划未通过后端校验，不能确认"));
  }

  let now = Utc::now().to_rfc3339();
  connection
    .execute(
      "UPDATE collection_plan
       SET validation_status = 'valid', validation_errors_json = '[]',
           confirmed_by_user = 1, updated_at = ?1
       WHERE id = ?2",
      params![now, plan_id],
    )
    .map_err(database_error)?;
  connection
    .execute(
      "UPDATE collection_task SET confirmed_at = ?1, updated_at = ?1 WHERE id = ?2",
      params![now, task_id],
    )
    .map_err(database_error)?;

  get_task_by_id(&connection, task_id)
}

pub fn copy_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<CollectionTaskView> {
  let connection = open_workspace_connection(&root_path)?;
  let task = get_task_by_id(&connection, task_id)?;
  create_collection_task(
    root_path,
    CreateCollectionTaskInput {
      name: format!("{} 副本", task.name),
      source_type: task.source_type,
      platforms: json_to_string_vec(task.platforms_json),
      data_types: json_to_string_vec(task.data_types_json),
    },
  )
}

pub fn get_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<CollectionTaskView> {
  let connection = open_workspace_connection(root_path)?;
  get_task_by_id(&connection, task_id)
}

pub fn list_tasks(
  root_path: impl AsRef<Path>,
  status: Option<String>,
) -> AppResult<Vec<CollectionTaskView>> {
  let connection = open_workspace_connection(root_path)?;

  if let Some(status) = status {
    let mut statement = connection
      .prepare(
        "SELECT id, name, source_type, status, platforms_json, data_types_json,
                created_at, updated_at, confirmed_at, completed_at, cancelled_at,
                cost_estimate_json, actual_cost_json
         FROM collection_task
         WHERE status = ?1
         ORDER BY created_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![status], map_task)
      .map_err(database_error)?;
    collect_rows(rows)
  } else {
    let mut statement = connection
      .prepare(
        "SELECT id, name, source_type, status, platforms_json, data_types_json,
                created_at, updated_at, confirmed_at, completed_at, cancelled_at,
                cost_estimate_json, actual_cost_json
         FROM collection_task
         ORDER BY created_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement.query_map([], map_task).map_err(database_error)?;
    collect_rows(rows)
  }
}

pub fn list_task_logs(
  root_path: impl AsRef<Path>,
  task_run_id: &str,
) -> AppResult<Vec<TaskLogView>> {
  let connection = open_workspace_connection(root_path)?;
  let mut statement = connection
    .prepare(
      "SELECT id, task_run_id, stage, level, message, safe_details_json, created_at
       FROM task_log
       WHERE task_run_id = ?1
       ORDER BY created_at ASC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_run_id], map_task_log)
    .map_err(database_error)?;
  collect_rows(rows)
}

fn open_workspace_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
}

fn normalize_create_task_input(
  input: CreateCollectionTaskInput,
) -> AppResult<CreateCollectionTaskInput> {
  let name = normalize_required("任务名称", &input.name)?;
  let source_type = match input.source_type.trim() {
    "form" | "natural_language" => input.source_type.trim().to_string(),
    _ => return Err(task_error("任务来源只支持 form 或 natural_language")),
  };

  Ok(CreateCollectionTaskInput {
    name,
    source_type,
    platforms: normalize_string_list("平台", input.platforms)?,
    data_types: normalize_string_list("数据类型", input.data_types)?,
  })
}

fn normalize_string_list(label: &str, values: Vec<String>) -> AppResult<Vec<String>> {
  let normalized = values
    .into_iter()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>();

  if normalized.is_empty() {
    return Err(task_error(format!("{label}不能为空")));
  }

  Ok(normalized)
}

fn normalize_required(field: &str, value: &str) -> AppResult<String> {
  let value = value.trim();

  if value.is_empty() {
    return Err(task_error(format!("{field}不能为空")));
  }

  Ok(value.to_string())
}

fn normalize_plan_source(source: &str) -> AppResult<String> {
  match source.trim() {
    "ai_generated" | "user_edited" | "form_generated" => Ok(source.trim().to_string()),
    _ => Err(task_error("采集计划来源不受支持")),
  }
}

fn latest_plan_for_task(connection: &Connection, task_id: &str) -> AppResult<CollectionPlanView> {
  connection
    .query_row(
      "SELECT id, task_id, source, schema_version, plan_json, validation_status,
              validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
       FROM collection_plan
       WHERE task_id = ?1
       ORDER BY created_at DESC
       LIMIT 1",
      params![task_id],
      map_collection_plan,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| task_error("任务还没有采集计划"))
}

fn get_collection_plan(connection: &Connection, plan_id: &str) -> AppResult<CollectionPlanView> {
  connection
    .query_row(
      "SELECT id, task_id, source, schema_version, plan_json, validation_status,
              validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
       FROM collection_plan
       WHERE id = ?1",
      params![plan_id],
      map_collection_plan,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| task_error("采集计划不存在"))
}

fn get_task_by_id(connection: &Connection, task_id: &str) -> AppResult<CollectionTaskView> {
  connection
    .query_row(
      "SELECT id, name, source_type, status, platforms_json, data_types_json,
              created_at, updated_at, confirmed_at, completed_at, cancelled_at,
              cost_estimate_json, actual_cost_json
       FROM collection_task
       WHERE id = ?1",
      params![task_id],
      map_task,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| task_error("任务不存在"))
}

fn get_task_run(connection: &Connection, run_id: &str) -> AppResult<TaskRunView> {
  connection
    .query_row(
      "SELECT id, task_id, status, started_at, ended_at, current_stage, error_code,
              error_message, retryable, cost_actual_json
       FROM task_run
       WHERE id = ?1",
      params![run_id],
      map_task_run,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| task_error("任务运行记录不存在"))
}

fn write_task_audit_log(
  connection: &Connection,
  action: &str,
  entity_id: Option<&str>,
  safe_details: Value,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO audit_log (id, entity_type, entity_id, action, safe_details_json, created_at)
       VALUES (?1, 'collection_task', ?2, ?3, ?4, ?5)",
      params![
        Uuid::new_v4().to_string(),
        entity_id,
        action,
        safe_details.to_string(),
        Utc::now().to_rfc3339()
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn map_task(row: &Row<'_>) -> rusqlite::Result<CollectionTaskView> {
  Ok(CollectionTaskView {
    id: row.get(0)?,
    name: row.get(1)?,
    source_type: row.get(2)?,
    status: row.get(3)?,
    platforms_json: string_to_json(row.get(4)?),
    data_types_json: string_to_json(row.get(5)?),
    created_at: row.get(6)?,
    updated_at: row.get(7)?,
    confirmed_at: row.get(8)?,
    completed_at: row.get(9)?,
    cancelled_at: row.get(10)?,
    cost_estimate_json: string_to_json(row.get(11)?),
    actual_cost_json: string_to_json(row.get(12)?),
  })
}

fn map_collection_plan(row: &Row<'_>) -> rusqlite::Result<CollectionPlanView> {
  Ok(CollectionPlanView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    source: row.get(2)?,
    schema_version: row.get(3)?,
    plan_json: string_to_json(row.get(4)?),
    validation_status: row.get(5)?,
    validation_errors_json: string_to_json(row.get(6)?),
    cost_estimate_json: string_to_json(row.get(7)?),
    confirmed_by_user: i64_to_bool(row.get(8)?),
    created_at: row.get(9)?,
    updated_at: row.get(10)?,
  })
}

fn map_task_run(row: &Row<'_>) -> rusqlite::Result<TaskRunView> {
  Ok(TaskRunView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    status: row.get(2)?,
    started_at: row.get(3)?,
    ended_at: row.get(4)?,
    current_stage: row.get(5)?,
    error_code: row.get(6)?,
    error_message: row.get(7)?,
    retryable: i64_to_bool(row.get(8)?),
    cost_actual_json: string_to_json(row.get(9)?),
  })
}

fn map_task_log(row: &Row<'_>) -> rusqlite::Result<TaskLogView> {
  Ok(TaskLogView {
    id: row.get(0)?,
    task_run_id: row.get(1)?,
    stage: row.get(2)?,
    level: row.get(3)?,
    message: row.get(4)?,
    safe_details_json: string_to_json(row.get(5)?),
    created_at: row.get(6)?,
  })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> AppResult<Vec<T>> {
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn json_array(values: Vec<String>) -> Value {
  serde_json::json!(values)
}

fn json_to_string_vec(value: Value) -> Vec<String> {
  value
    .as_array()
    .map(|values| {
      values
        .iter()
        .filter_map(|value| value.as_str().map(ToString::to_string))
        .collect()
    })
    .unwrap_or_default()
}

fn string_to_json(value: String) -> Value {
  serde_json::from_str(&value).unwrap_or_else(|_| serde_json::json!({}))
}

fn i64_to_bool(value: i64) -> bool {
  value != 0
}

fn task_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::Validation,
    false,
  )
}

fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "tasks/execution_tests.rs"]
mod execution_tests;
