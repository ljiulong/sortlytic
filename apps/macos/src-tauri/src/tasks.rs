use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

mod deletion;
mod execution;
mod plans;
mod recovery;
mod revisions;
mod snapshot;
mod validation;
mod worker;
mod worker_fence;
mod worker_lock;

#[cfg(test)]
mod test_support;

pub use deletion::delete_task;
pub(crate) use execution::claim_next_task_with_fence;
pub use execution::{
  cancel_task, claim_next_task, complete_task_run, enqueue_task, fail_task_run, retry_task,
};
use plans::latest_plan_for_task;
pub(crate) use plans::save_collection_plan_in_transaction;
pub use plans::{confirm_collection_plan, estimate_task_cost, save_collection_plan};
pub use revisions::revise_collection_task;
pub(crate) use worker_fence::WorkerFence;
pub use worker_lock::{execute_next_task, recover_interrupted_runs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskWorkerWorkState {
  pub(crate) has_queued_run: bool,
  pub(crate) has_running_run: bool,
}

pub(crate) fn task_worker_work_state(
  root_path: impl AsRef<Path>,
) -> AppResult<TaskWorkerWorkState> {
  let connection = open_workspace_connection(root_path)?;
  connection
    .query_row(
      "SELECT
         EXISTS(SELECT 1 FROM task_run WHERE status = 'queued'),
         EXISTS(SELECT 1 FROM task_run WHERE status = 'running')",
      [],
      |row| {
        Ok(TaskWorkerWorkState {
          has_queued_run: row.get(0)?,
          has_running_run: row.get(1)?,
        })
      },
    )
    .map_err(database_error)
}

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
pub struct ReviseCollectionTaskInput {
  pub task_id: String,
  pub name: String,
  pub platforms: Vec<String>,
  pub data_types: Vec<String>,
  #[serde(default)]
  pub original_intent: Option<String>,
  pub source: String,
  pub plan_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RevisedCollectionTaskView {
  pub task: CollectionTaskView,
  pub collection_plan: CollectionPlanView,
  pub copied_from_task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionTaskView {
  pub id: String,
  pub name: String,
  pub source_type: String,
  pub status: String,
  pub platforms_json: Value,
  pub data_types_json: Value,
  pub account_source: Option<String>,
  pub selected_fields_json: Value,
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
  pub plan_id: Option<String>,
  pub attempt_number: i64,
  pub claimed_at: Option<String>,
  pub status: String,
  pub started_at: String,
  pub ended_at: Option<String>,
  pub current_stage: Option<String>,
  pub current_stage_code: String,
  pub error_code: Option<String>,
  pub error_message: Option<String>,
  pub retryable: bool,
  pub cost_actual_json: Value,
  pub error_safe_details_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskLogView {
  pub id: String,
  pub task_run_id: String,
  pub stage: String,
  pub stage_code: String,
  pub level: String,
  pub message: String,
  pub message_code: String,
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

pub(crate) const MAX_NATURAL_INTENT_CHARACTERS: usize = 10_000;
pub(crate) const MAX_NATURAL_INTENT_BYTES: usize = 32_000;

pub fn create_collection_task(
  root_path: impl AsRef<Path>,
  input: CreateCollectionTaskInput,
) -> AppResult<CollectionTaskView> {
  create_collection_task_with_initial_intent(root_path, input, None)
}

pub fn create_collection_task_with_initial_intent(
  root_path: impl AsRef<Path>,
  input: CreateCollectionTaskInput,
  intent_text: Option<&str>,
) -> AppResult<CollectionTaskView> {
  let input = normalize_create_task_input(input)?;
  let intent_text = match (input.source_type.as_str(), intent_text) {
    ("form", Some(_)) => {
      return Err(task_error("表单任务不能携带自然语言解析原文"));
    }
    ("natural_language", Some(value)) => Some(normalize_natural_intent_text(value)?),
    (_, None) => None,
    _ => None,
  };
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection.transaction().map_err(database_error)?;
  let id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();

  transaction
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

  if let Some(intent_text) = intent_text {
    transaction
      .execute(
        "INSERT INTO task_intent (
          id, task_id, intent_text, parse_status, parse_phase,
          error_safe_details_json, created_at, updated_at
        ) VALUES (
          ?1, ?2, ?3, 'needs_review', 'preparing',
          '{\"source\":\"pending_generation\"}', ?4, ?4
        )",
        params![Uuid::new_v4().to_string(), id, intent_text, now],
      )
      .map_err(database_error)?;
  }

  write_task_audit_log(
    &transaction,
    "create_collection_task",
    Some(&id),
    serde_json::json!({}),
  )?;
  let task = get_task_by_id(&transaction, &id)?;
  transaction.commit().map_err(database_error)?;
  Ok(task)
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

pub fn get_latest_collection_plan(
  root_path: impl AsRef<Path>,
  task_id: &str,
) -> AppResult<CollectionPlanView> {
  let connection = open_workspace_connection(root_path)?;
  latest_plan_for_task(&connection, task_id)
}

pub fn list_tasks(
  root_path: impl AsRef<Path>,
  status: Option<String>,
) -> AppResult<Vec<CollectionTaskView>> {
  let connection = open_workspace_connection(root_path)?;

  let mut tasks = if let Some(status) = status {
    let mut statement = connection
      .prepare(
        "SELECT id, name, source_type, status, platforms_json, data_types_json,
                account_source, selected_fields_json,
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
    collect_rows(rows)?
  } else {
    let mut statement = connection
      .prepare(
        "SELECT id, name, source_type, status, platforms_json, data_types_json,
                account_source, selected_fields_json,
                created_at, updated_at, confirmed_at, completed_at, cancelled_at,
                cost_estimate_json, actual_cost_json
         FROM collection_task
         ORDER BY created_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement.query_map([], map_task).map_err(database_error)?;
    collect_rows(rows)?
  };
  refresh_task_cost_estimates(&connection, &mut tasks)?;
  Ok(tasks)
}

fn refresh_task_cost_estimates(
  connection: &Connection,
  tasks: &mut [CollectionTaskView],
) -> AppResult<()> {
  for task in tasks {
    let plan_json = connection
      .query_row(
        "SELECT plan_json FROM collection_plan
         WHERE task_id = ?1
         ORDER BY created_at DESC, id DESC LIMIT 1",
        params![task.id],
        |row| row.get::<_, String>(0),
      )
      .optional()
      .map_err(database_error)?;
    if let Some(plan_json) = plan_json {
      task.cost_estimate_json =
        validation::estimate_from_plan_json(&string_to_json(plan_json)).cost_estimate_json;
    }
  }
  Ok(())
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

const LIST_LATEST_TASK_RUNS_SQL: &str = "
  WITH ranked_plans AS (
    SELECT
      plan.id,
      plan.task_id,
      ROW_NUMBER() OVER (
        PARTITION BY plan.task_id
        ORDER BY plan.created_at DESC, plan.id DESC
      ) AS plan_rank
    FROM collection_plan AS plan
  ),
  current_plans AS (
    SELECT id, task_id
    FROM ranked_plans
    WHERE plan_rank = 1
  ),
  ranked_runs AS (
    SELECT
      run.id,
      run.task_id,
      run.status,
      run.started_at,
      run.ended_at,
      run.current_stage,
      run.error_code,
      run.error_message,
      run.retryable,
      run.cost_actual_json,
      run.plan_id,
      run.attempt_number,
      run.claimed_at,
      run.run_sequence,
      ROW_NUMBER() OVER (
        PARTITION BY run.task_id
        ORDER BY run.run_sequence DESC
      ) AS run_rank
    FROM task_run AS run
    JOIN current_plans AS plan
      ON plan.task_id = run.task_id AND plan.id = run.plan_id
  ),
  ranked_logs AS (
    SELECT
      log.task_run_id,
      log.level,
      log.safe_details_json,
      ROW_NUMBER() OVER (
        PARTITION BY log.task_run_id, log.level
        ORDER BY log.created_at DESC, log.id DESC
      ) AS log_rank
    FROM task_log AS log
    WHERE log.level IN ('error', 'warning')
  )
  SELECT
    run.id,
    run.task_id,
    run.status,
    run.started_at,
    run.ended_at,
    run.current_stage,
    run.error_code,
    run.error_message,
    run.retryable,
    run.cost_actual_json,
    run.plan_id,
    run.attempt_number,
    run.claimed_at,
    COALESCE(log.safe_details_json, '{}')
  FROM ranked_runs AS run
  LEFT JOIN ranked_logs AS log
    ON log.task_run_id = run.id
   AND log.log_rank = 1
   AND (
     (run.status = 'failed' AND log.level = 'error')
     OR (run.status = 'partial_success' AND log.level = 'warning')
   )
  WHERE run.run_rank = 1
  ORDER BY run.run_sequence DESC";

pub fn list_latest_task_runs(root_path: impl AsRef<Path>) -> AppResult<Vec<TaskRunView>> {
  let connection = open_workspace_connection(root_path)?;
  let mut statement = connection
    .prepare(LIST_LATEST_TASK_RUNS_SQL)
    .map_err(database_error)?;
  let rows = statement
    .query_map([], map_task_run)
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
  let allow_empty_scope = source_type == "natural_language";

  Ok(CreateCollectionTaskInput {
    name,
    source_type,
    platforms: normalize_string_list("平台", input.platforms, allow_empty_scope)?,
    data_types: normalize_string_list("数据类型", input.data_types, allow_empty_scope)?,
  })
}

fn normalize_string_list(
  label: &str,
  values: Vec<String>,
  allow_empty: bool,
) -> AppResult<Vec<String>> {
  let normalized = values
    .into_iter()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>();

  if normalized.is_empty() && !allow_empty {
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

pub(crate) fn normalize_natural_intent_text(value: &str) -> AppResult<String> {
  let value = normalize_required("自然语言需求", value)?;
  let character_count = value.chars().count();
  if character_count > MAX_NATURAL_INTENT_CHARACTERS {
    return Err(task_error(format!(
      "自然语言需求最多允许 {MAX_NATURAL_INTENT_CHARACTERS} 个字符，当前为 {character_count} 个字符"
    )));
  }
  let byte_count = value.len();
  if byte_count > MAX_NATURAL_INTENT_BYTES {
    return Err(task_error(format!(
      "自然语言需求最多允许 {MAX_NATURAL_INTENT_BYTES} 个 UTF-8 字节，当前为 {byte_count} 个字节"
    )));
  }
  Ok(value)
}

fn get_task_by_id(connection: &Connection, task_id: &str) -> AppResult<CollectionTaskView> {
  connection
    .query_row(
      "SELECT id, name, source_type, status, platforms_json, data_types_json,
              account_source, selected_fields_json,
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
      "SELECT run.id, run.task_id, run.status, run.started_at, run.ended_at, run.current_stage,
              run.error_code, run.error_message, run.retryable, run.cost_actual_json,
              run.plan_id, run.attempt_number, run.claimed_at,
              COALESCE((
                SELECT log.safe_details_json FROM task_log AS log
                WHERE log.task_run_id = run.id
                  AND ((run.status = 'failed' AND log.level = 'error')
                    OR (run.status = 'partial_success' AND log.level = 'warning'))
                ORDER BY log.created_at DESC, log.id DESC LIMIT 1
              ), '{}')
       FROM task_run AS run
       WHERE run.id = ?1",
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
    account_source: row.get(6)?,
    selected_fields_json: string_to_json(row.get(7)?),
    created_at: row.get(8)?,
    updated_at: row.get(9)?,
    confirmed_at: row.get(10)?,
    completed_at: row.get(11)?,
    cancelled_at: row.get(12)?,
    cost_estimate_json: string_to_json(row.get(13)?),
    actual_cost_json: string_to_json(row.get(14)?),
  })
}

fn map_task_run(row: &Row<'_>) -> rusqlite::Result<TaskRunView> {
  let current_stage = row.get::<_, Option<String>>(5)?;
  Ok(TaskRunView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    status: row.get(2)?,
    started_at: row.get(3)?,
    ended_at: row.get(4)?,
    current_stage_code: task_stage_code(current_stage.as_deref()).to_string(),
    current_stage,
    error_code: row.get(6)?,
    error_message: row.get(7)?,
    retryable: i64_to_bool(row.get(8)?),
    cost_actual_json: string_to_json(row.get(9)?),
    plan_id: row.get(10)?,
    attempt_number: row.get(11)?,
    claimed_at: row.get(12)?,
    error_safe_details_json: string_to_json(row.get(13)?),
  })
}

fn map_task_log(row: &Row<'_>) -> rusqlite::Result<TaskLogView> {
  let stage = row.get::<_, String>(2)?;
  let message = row.get::<_, String>(4)?;
  Ok(TaskLogView {
    id: row.get(0)?,
    task_run_id: row.get(1)?,
    stage_code: task_stage_code(Some(&stage)).to_string(),
    stage,
    level: row.get(3)?,
    message_code: task_message_code(&message).to_string(),
    message,
    safe_details_json: string_to_json(row.get(5)?),
    created_at: row.get(6)?,
  })
}

fn task_stage_code(stage: Option<&str>) -> &'static str {
  match stage {
    None => "STAGE_PENDING",
    Some("等待执行") => "WAITING_EXECUTION",
    Some("执行采集") => "COLLECTING",
    Some("持久化采集结果") => "PERSISTING_RESULTS",
    Some("已完成") => "COMPLETED",
    Some("部分成功") => "PARTIAL_SUCCESS",
    Some("执行失败") => "EXECUTION_FAILED",
    Some("用户取消") => "USER_CANCELLED",
    Some("恢复响应入库") => "RECOVERY_PERSIST_RESPONSE",
    Some("恢复重试") => "RECOVERY_RETRY",
    Some("恢复待发送") => "RECOVERY_READY_TO_SEND",
    Some("恢复续页") => "RECOVERY_NEXT_PAGE",
    Some("恢复收尾") => "RECOVERY_FINALIZE",
    Some("恢复等待") => "RECOVERY_WAITING",
    Some("请求状态不确定") => "REQUEST_STATE_UNCERTAIN",
    Some("运行快照不完整") => "RUN_SNAPSHOT_INCOMPLETE",
    Some("检查点状态冲突") => "CHECKPOINT_STATE_CONFLICT",
    Some("运行步骤状态冲突") => "RUN_STEP_STATE_CONFLICT",
    Some("检查点证据不完整") => "CHECKPOINT_EVIDENCE_INCOMPLETE",
    Some("检查点终止失败") => "CHECKPOINT_TERMINAL_FAILURE",
    Some("恢复指令冲突") => "RECOVERY_INSTRUCTION_CONFLICT",
    Some("请求证据需要人工处理") => "REQUEST_EVIDENCE_REQUIRES_REVIEW",
    Some("运行快照需要人工处理") => "RUN_SNAPSHOT_REQUIRES_REVIEW",
    Some("需要重新确认计划") => "PLAN_RECONFIRMATION_REQUIRED",
    Some("活动运行冲突") => "ACTIVE_RUN_CONFLICT",
    Some("活动运行冲突迁移") => "ACTIVE_RUN_CONFLICT_MIGRATION",
    Some("活动步骤冲突迁移") => "ACTIVE_STEP_CONFLICT_MIGRATION",
    Some("请求检查点冲突迁移") => "REQUEST_CHECKPOINT_CONFLICT_MIGRATION",
    Some(_) => "UNKNOWN_STAGE",
  }
}

fn task_message_code(message: &str) -> &'static str {
  match message {
    "任务已加入本地队列" => "TASK_ENQUEUED",
    "本地执行器已领取任务" => "TASK_CLAIMED",
    "本地执行器已领取恢复任务" => "RECOVERY_TASK_CLAIMED",
    "失败任务已重新排队" => "FAILED_TASK_REQUEUED",
    "任务部分目标失败，合格数据已保留" => "TASK_PARTIALLY_SUCCEEDED",
    "全部采集目标失败" => "ALL_TARGETS_FAILED",
    "任务执行成功" => "TASK_SUCCEEDED",
    "任务已由用户取消" => "TASK_CANCELLED_BY_USER",
    "队列中存在可能已发送的 TikHub 请求，远端副作用无法确认，禁止自动重发" => {
      "QUEUED_REQUEST_UNCERTAIN"
    }
    "运行步骤快照不完整，可能丢失远端请求证据，已停止自动执行"
    | "运行步骤快照不完整，或运行中步骤缺少检查点，禁止自动重发" => {
      "RUN_SNAPSHOT_INCOMPLETE"
    }
    "队列恢复指令与运行步骤及检查点证据不一致，已停止自动执行" => {
      "RECOVERY_INSTRUCTION_CONFLICT"
    }
    "进程在 TikHub 请求完成前中断，无法确认远端是否已计费或返回，禁止自动重发" => {
      "INTERRUPTED_REQUEST_UNCERTAIN"
    }
    "任务包含状态不确定的 TikHub 请求，必须人工确认后再处理" => {
      "UNCERTAIN_REQUEST_REQUIRES_REVIEW"
    }
    "任务存在多个冲突的恢复前沿，无法安全判断下一执行位置" => {
      "CHECKPOINT_STATE_CONFLICT"
    }
    "检查点页码或游标链不连续，无法安全判断恢复位置" => {
      "CHECKPOINT_CURSOR_CHAIN_INVALID"
    }
    "运行步骤状态与检查点证据不相容，已停止自动恢复" => {
      "RUN_STEP_STATE_CONFLICT"
    }
    "已接收或已提交的检查点缺少可验证响应、提交时间或续页游标" => {
      "CHECKPOINT_EVIDENCE_INCOMPLETE"
    }
    "任务包含不可重试的失败检查点，已停止自动恢复" => {
      "CHECKPOINT_TERMINAL_FAILURE"
    }
    "TikHub 响应已保存，恢复时只继续本地入库，不重新发送请求" => {
      "RECOVERY_PERSIST_SAVED_RESPONSE"
    }
    "失败检查点仍在请求、记录和预算限制内，等待安全重试" => {
      "RECOVERY_RETRY_SAFE"
    }
    "检查点仍处于 prepared，可从尚未发送的请求继续" => {
      "RECOVERY_PREPARED_REQUEST"
    }
    "从已提交检查点的 next_cursor 继续下一页" => "RECOVERY_CONTINUE_NEXT_PAGE",
    "已完成步骤没有续页，继续下一个尚未发送的运行步骤" => {
      "RECOVERY_CONTINUE_NEXT_STEP"
    }
    "最后一个检查点已提交且没有续页，等待完成本地收尾" => {
      "RECOVERY_FINALIZE_LOCAL"
    }
    "运行步骤尚未发送请求，可从待执行步骤继续" => "RECOVERY_PENDING_STEP",
    "未发现已发送请求的检查点，任务已重新排队" => {
      "RECOVERY_REQUEUED_WITHOUT_SENT_REQUEST"
    }
    "检测到同一任务存在多个活动运行，所有活动运行已停止并要求人工复核" => {
      "ACTIVE_RUN_CONFLICT_REQUIRES_REVIEW"
    }
    "活动运行冲突迁移已终止未完成的运行步骤" => "ACTIVE_STEP_CONFLICT_MIGRATION",
    "活动运行冲突迁移已将 requesting 检查点转为 uncertain" => {
      "REQUEST_CHECKPOINT_CONFLICT_MIGRATION"
    }
    value
      if value.starts_with(
        "采集计划不可执行，且运行记录包含已发送请求证据，禁止重新入队，必须人工处理：",
      ) =>
    {
      "REQUEST_EVIDENCE_REQUIRES_REVIEW"
    }
    value
      if value.starts_with(
        "采集计划不可执行，且运行快照无法证明请求从未发送，禁止重新入队，必须人工处理：",
      ) =>
    {
      "RUN_SNAPSHOT_REQUIRES_REVIEW"
    }
    value if value.starts_with("采集计划不可执行，任务已停止，请重新确认有效的 v2 计划：") => {
      "PLAN_RECONFIRMATION_REQUIRED"
    }
    _ => "UNKNOWN_MESSAGE",
  }
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
mod revision_tests;

#[cfg(test)]
#[path = "tasks/execution_tests.rs"]
mod execution_tests;
