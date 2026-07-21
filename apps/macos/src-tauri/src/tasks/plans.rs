use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use crate::collection::{
  validate_collection_plan_v2, validate_collection_plan_v3, validate_collection_plan_v4,
};
use crate::domain::AppResult;

use super::validation::{estimate_from_plan_json, validate_plan_for_task};
use super::{
  database_error, get_task_by_id, i64_to_bool, open_workspace_connection, string_to_json,
  task_error, CollectionPlanView, CollectionTaskView, CostEstimateView, SaveCollectionPlanInput,
};

const LEGACY_PLAN_ERROR: &str = "v1 采集计划仅兼容读取，不能确认/执行";

pub fn save_collection_plan(
  root_path: impl AsRef<Path>,
  input: SaveCollectionPlanInput,
) -> AppResult<CollectionPlanView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let id = save_collection_plan_in_transaction(&transaction, input)?;
  transaction.commit().map_err(database_error)?;
  get_collection_plan(&connection, &id)
}

pub(super) fn save_collection_plan_in_transaction(
  connection: &Connection,
  input: SaveCollectionPlanInput,
) -> AppResult<String> {
  let task = get_task_by_id(connection, &input.task_id)?;

  if !["draft", "waiting_confirmation"].contains(&task.status.as_str()) {
    return Err(task_error("只允许给草稿或等待确认状态的任务保存采集计划"));
  }

  let source = normalize_plan_source(&input.source)?;
  let id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  let schema_version = detect_plan_schema_version(&input.plan_json);
  let validation_errors = validate_plan_for_schema(&task, &input.plan_json, schema_version);
  let validation_status = if validation_errors.is_empty() {
    "valid"
  } else {
    "needs_review"
  };
  let task_status = if validation_status == "valid" {
    "waiting_confirmation"
  } else {
    "draft"
  };
  let validation_errors = serde_json::json!(validation_errors);
  let cost_estimate = estimate_from_plan_json(&input.plan_json).cost_estimate_json;
  let account_source = (schema_version == 4)
    .then(|| {
      input
        .plan_json
        .get("account_source")
        .and_then(Value::as_str)
    })
    .flatten()
    .map(ToString::to_string);
  let selected_fields_json = (schema_version == 4)
    .then(|| input.plan_json.get("selected_fields").cloned())
    .flatten()
    .map(|value| value.to_string());

  connection
    .execute(
      "UPDATE collection_plan
       SET confirmed_by_user = 0, updated_at = ?1
       WHERE task_id = ?2 AND confirmed_by_user = 1",
      params![now, input.task_id],
    )
    .map_err(database_error)?;

  connection
    .execute(
      "INSERT INTO collection_plan (
        id, task_id, source, schema_version, plan_json, validation_status,
        validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)",
      params![
        id,
        input.task_id,
        source,
        schema_version,
        input.plan_json.to_string(),
        validation_status,
        validation_errors.to_string(),
        cost_estimate.to_string(),
        now,
        now
      ],
    )
    .map_err(database_error)?;

  persist_api_call_steps(connection, &id, &input.plan_json, validation_status, &now)?;

  connection
    .execute(
      "UPDATE collection_task
       SET status = ?1, confirmed_at = NULL,
           cost_estimate_json = ?2, updated_at = ?3,
           account_source = CASE WHEN ?4 = 4 THEN ?5 ELSE account_source END,
           selected_fields_json = CASE WHEN ?4 = 4 THEN ?6 ELSE selected_fields_json END
       WHERE id = ?7",
      params![
        task_status,
        cost_estimate.to_string(),
        now,
        schema_version,
        account_source,
        selected_fields_json,
        input.task_id
      ],
    )
    .map_err(database_error)?;

  Ok(id)
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
  let default_request_limit = plan_json
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
    let request_limit = step
      .get("request_limit")
      .and_then(Value::as_i64)
      .unwrap_or(default_request_limit)
      .max(1);
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
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let plan = get_collection_plan(&transaction, plan_id)?;
  let task = get_task_by_id(&transaction, task_id)?;

  if plan.task_id != task_id {
    return Err(task_error("采集计划不属于当前任务"));
  }

  if !["draft", "waiting_confirmation"].contains(&task.status.as_str()) {
    return Err(task_error("只有草稿或等待确认状态的任务可以确认采集计划"));
  }

  if latest_plan_for_task(&transaction, task_id)?.id != plan.id {
    return Err(task_error("只能确认当前任务的最新采集计划"));
  }

  let validation_errors = validate_plan_for_schema(&task, &plan.plan_json, plan.schema_version);
  if !matches!(plan.schema_version, 2..=4) || !validation_errors.is_empty() {
    let error_message = confirmation_error_message(plan.schema_version);
    let now = Utc::now().to_rfc3339();
    transaction
      .execute(
        "UPDATE collection_plan
         SET validation_status = 'needs_review', validation_errors_json = ?1,
             confirmed_by_user = 0, updated_at = ?2
         WHERE id = ?3",
        params![
          serde_json::json!(validation_errors).to_string(),
          now,
          plan_id
        ],
      )
      .map_err(database_error)?;
    transaction
      .execute(
        "UPDATE collection_task
         SET status = 'draft', confirmed_at = NULL, updated_at = ?1
         WHERE id = ?2",
        params![now, task_id],
      )
      .map_err(database_error)?;
    transaction.commit().map_err(database_error)?;
    return Err(task_error(error_message));
  }

  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "UPDATE collection_plan
       SET confirmed_by_user = 0, updated_at = ?1
       WHERE task_id = ?2 AND id <> ?3 AND confirmed_by_user = 1",
      params![now, task_id, plan_id],
    )
    .map_err(database_error)?;
  let confirmed = transaction
    .execute(
      "UPDATE collection_plan
       SET validation_status = 'valid', validation_errors_json = '[]',
           confirmed_by_user = 1, updated_at = ?1
       WHERE id = ?2 AND task_id = ?3
         AND id = (
           SELECT id FROM collection_plan
           WHERE task_id = ?3
           ORDER BY created_at DESC, id DESC
           LIMIT 1
         )",
      params![now, plan_id, task_id],
    )
    .map_err(database_error)?;
  if confirmed != 1 {
    return Err(task_error("采集计划状态已变化，请重新确认最新采集计划"));
  }
  let updated = transaction
    .execute(
      "UPDATE collection_task
       SET confirmed_at = ?1, updated_at = ?1
       WHERE id = ?2 AND status IN ('draft', 'waiting_confirmation')",
      params![now, task_id],
    )
    .map_err(database_error)?;
  if updated != 1 {
    return Err(task_error("任务状态已变化，请重新确认采集计划"));
  }
  transaction.commit().map_err(database_error)?;

  get_task_by_id(&connection, task_id)
}

fn normalize_plan_source(source: &str) -> AppResult<String> {
  match source.trim() {
    "ai_generated" | "user_edited" | "form_generated" => Ok(source.trim().to_string()),
    _ => Err(task_error("采集计划来源不受支持")),
  }
}

fn detect_plan_schema_version(plan_json: &Value) -> i64 {
  if let Some(version) = plan_json.get("schema_version").and_then(Value::as_i64) {
    return version;
  }
  if plan_json.get("record_limit").is_some() || plan_json.get("budget_limit").is_some() {
    2
  } else {
    1
  }
}

fn validate_plan_for_schema(
  task: &CollectionTaskView,
  plan_json: &Value,
  schema_version: i64,
) -> Vec<String> {
  let mut errors = validate_plan_for_task(task, plan_json);
  match schema_version {
    4 => errors.extend(validate_collection_plan_v4(plan_json).errors),
    3 => errors.extend(validate_collection_plan_v3(plan_json).errors),
    2 => errors.extend(validate_collection_plan_v2(plan_json).errors),
    1 => errors.push(LEGACY_PLAN_ERROR.to_string()),
    version => errors.push(format!(
      "schema_version={version} 不受支持，只有 v2/v3/v4 采集计划可以确认/执行"
    )),
  }
  errors.sort();
  errors.dedup();
  errors
}

fn confirmation_error_message(schema_version: i64) -> String {
  match schema_version {
    4 => "采集计划未通过后端校验（v4），不能确认".to_string(),
    3 => "采集计划未通过后端校验（v3），不能确认".to_string(),
    2 => "采集计划未通过后端校验（v2），不能确认".to_string(),
    1 => LEGACY_PLAN_ERROR.to_string(),
    version => format!("schema_version={version} 不受支持，只有 v2/v3/v4 采集计划可以确认/执行"),
  }
}

pub(super) fn latest_plan_for_task(
  connection: &Connection,
  task_id: &str,
) -> AppResult<CollectionPlanView> {
  connection
    .query_row(
      "SELECT id, task_id, source, schema_version, plan_json, validation_status,
              validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
       FROM collection_plan
       WHERE task_id = ?1
       ORDER BY created_at DESC, id DESC
       LIMIT 1",
      params![task_id],
      map_collection_plan,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| {
      task_error("任务还没有采集计划")
        .with_safe_detail("reason", "no_plan")
        .with_safe_detail("entity", "collection_plan")
    })
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
