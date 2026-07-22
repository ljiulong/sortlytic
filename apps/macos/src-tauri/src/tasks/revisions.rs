use std::path::Path;

use chrono::Utc;
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use uuid::Uuid;

use super::plans::{latest_plan_for_task, save_collection_plan_in_transaction};
use super::{
  database_error, get_task_by_id, json_array, normalize_required, normalize_string_list,
  open_workspace_connection, task_error, write_task_audit_log, ReviseCollectionTaskInput,
  RevisedCollectionTaskView, SaveCollectionPlanInput,
};
use crate::domain::AppResult;

pub fn revise_collection_task(
  root_path: impl AsRef<Path>,
  input: ReviseCollectionTaskInput,
) -> AppResult<RevisedCollectionTaskView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let current = get_task_by_id(&transaction, input.task_id.trim())?;
  if ["queued", "running"].contains(&current.status.as_str()) {
    return Err(task_error(
      "排队或运行中的任务必须先取消，才能编辑并生成新计划版本",
    ));
  }
  if ![
    "draft",
    "waiting_confirmation",
    "failed",
    "cancelled",
    "success",
    "partial_success",
  ]
  .contains(&current.status.as_str())
  {
    return Err(task_error("当前任务状态不支持编辑"));
  }
  if input.source.trim() != "user_edited" {
    return Err(task_error("任务修订的计划来源必须为 user_edited"));
  }

  let name = normalize_required("任务名称", &input.name)?;
  let platforms = normalize_string_list("平台", input.platforms, false)?;
  let data_types = normalize_string_list("数据类型", input.data_types, false)?;
  let now = Utc::now().to_rfc3339();
  let copied_from_task_id =
    matches!(current.status.as_str(), "success" | "partial_success").then(|| current.id.clone());
  let target_task_id = if copied_from_task_id.is_some() {
    let id = Uuid::new_v4().to_string();
    transaction
      .execute(
        "INSERT INTO collection_task (
          id, name, source_type, status, platforms_json, data_types_json,
          created_at, updated_at, cost_estimate_json, actual_cost_json
        ) VALUES (?1, ?2, ?3, 'draft', ?4, ?5, ?6, ?6, '{}', '{}')",
        params![
          id,
          name,
          current.source_type,
          json_array(platforms).to_string(),
          json_array(data_types).to_string(),
          now
        ],
      )
      .map_err(database_error)?;
    id
  } else {
    transaction
      .execute(
        "UPDATE collection_task
         SET name = ?1, platforms_json = ?2, data_types_json = ?3,
             status = 'draft', confirmed_at = NULL, completed_at = NULL,
             cancelled_at = NULL, cost_estimate_json = '{}', actual_cost_json = '{}',
             updated_at = ?4
         WHERE id = ?5",
        params![
          name,
          json_array(platforms).to_string(),
          json_array(data_types).to_string(),
          now,
          current.id
        ],
      )
      .map_err(database_error)?;
    current.id.clone()
  };

  if current.source_type == "natural_language" {
    if let Some(source_task_id) = copied_from_task_id.as_deref() {
      copy_latest_natural_language_intent(&transaction, source_task_id, &target_task_id, &now)?;
    }
  }

  let plan_id = save_collection_plan_in_transaction(
    &transaction,
    SaveCollectionPlanInput {
      task_id: target_task_id.clone(),
      source: "user_edited".to_string(),
      plan_json: input.plan_json,
      validation_status: String::new(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )?;
  write_task_audit_log(
    &transaction,
    "revise_collection_task",
    Some(&target_task_id),
    serde_json::json!({
      "plan_id": plan_id,
      "copied_from_task_id": copied_from_task_id,
    }),
  )?;
  let task = get_task_by_id(&transaction, &target_task_id)?;
  let collection_plan = latest_plan_for_task(&transaction, &target_task_id)?;
  transaction.commit().map_err(database_error)?;

  Ok(RevisedCollectionTaskView {
    task,
    collection_plan,
    copied_from_task_id,
  })
}

fn copy_latest_natural_language_intent(
  transaction: &Transaction<'_>,
  source_task_id: &str,
  target_task_id: &str,
  now: &str,
) -> AppResult<()> {
  let latest_intent = transaction
    .query_row(
      "SELECT intent_text, language
       FROM task_intent
       WHERE task_id = ?1
       ORDER BY updated_at DESC, created_at DESC, id DESC
       LIMIT 1",
      params![source_task_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .optional()
    .map_err(database_error)?;
  let Some((intent_text, language)) = latest_intent else {
    return Ok(());
  };
  let safe_details = serde_json::json!({
    "source": "user_edited_copy",
    "copied_from_task_id": source_task_id,
  });
  transaction
    .execute(
      "INSERT INTO task_intent (
        id, task_id, intent_text, language, parse_status, parse_phase,
        ai_run_id, error_code, error_message, retryable,
        error_safe_details_json, created_at, updated_at
      ) VALUES (?1, ?2, ?3, ?4, 'valid', 'success', NULL, NULL, NULL, NULL, ?5, ?6, ?6)",
      params![
        Uuid::new_v4().to_string(),
        target_task_id,
        intent_text,
        language,
        safe_details.to_string(),
        now
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}
