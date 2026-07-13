use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use crate::collection::validate_collection_plan_v2;
use crate::domain::{redact_sensitive_text, AppErrorCode, AppResult};

use super::recovery::{
  gate_queued_run_for_claim, quarantine_queued_request_uncertainty, recovery_stage,
  run_snapshot_allows_plan_reconfirmation,
};
use super::validation::validate_plan_for_task;
use super::{
  database_error, get_task_by_id, get_task_run, latest_plan_for_task, map_task_run,
  normalize_required, open_workspace_connection, task_error, CollectionTaskView, TaskRunView,
};

struct PersistedPlanStep {
  order: i64,
  platform: String,
  data_type: String,
  endpoint_key: String,
  params_json: String,
  status: String,
  request_count_estimate: i64,
  cost_estimate_json: String,
}

pub fn enqueue_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let task = get_task_by_id(&transaction, task_id)?;

  if task.confirmed_at.is_none() {
    return Err(task_error("任务必须先确认采集计划才能入队"));
  }
  if task.status != "waiting_confirmation" {
    return Err(task_error(
      "只有已确认待执行任务可以入队，失败任务必须使用重试流程",
    ));
  }

  let plan_id = confirmed_plan_id(&transaction, task_id)?;
  let attempt_number = next_attempt_number(&transaction, task_id, &plan_id)?;
  let run_id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "INSERT INTO task_run (
        id, task_id, plan_id, attempt_number, status, started_at, current_stage, retryable
      ) VALUES (?1, ?2, ?3, ?4, 'queued', ?5, '等待执行', 0)",
      params![run_id, task_id, plan_id, attempt_number, now],
    )
    .map_err(database_error)?;
  materialize_run_steps(&transaction, &run_id, &plan_id, &now)?;
  transaction
    .execute(
      "UPDATE collection_task
       SET status = 'queued', completed_at = NULL, cancelled_at = NULL, updated_at = ?1
       WHERE id = ?2",
      params![now, task_id],
    )
    .map_err(database_error)?;
  append_task_log(
    &transaction,
    &run_id,
    "等待执行",
    "info",
    "任务已加入本地队列",
  )?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, &run_id)
}

pub fn claim_next_task(root_path: impl AsRef<Path>) -> AppResult<Option<TaskRunView>> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  loop {
    let queued = transaction
      .query_row(
        "SELECT id, task_id, status, started_at, ended_at, current_stage, error_code,
                error_message, retryable, cost_actual_json, plan_id, attempt_number, claimed_at
         FROM task_run WHERE status = 'queued'
         ORDER BY started_at ASC, id ASC
         LIMIT 1",
        [],
        map_task_run,
      )
      .optional()
      .map_err(database_error)?;
    let Some(queued) = queued else {
      transaction.commit().map_err(database_error)?;
      return Ok(None);
    };
    if quarantine_queued_request_uncertainty(&transaction, &queued.id, &queued.task_id)? {
      continue;
    }
    let plan_id = queued.plan_id.as_deref();
    let plan_validation = plan_id.map_or_else(
      || Err(task_error("队列任务缺少采集计划，不能领取")),
      |plan_id| require_executable_plan(&transaction, &queued.task_id, plan_id),
    );
    if let Err(error) = plan_validation {
      if !matches!(&error.code, AppErrorCode::ValidationError) {
        return Err(error);
      }
      quarantine_run_for_reconfirmation(
        &transaction,
        &queued.id,
        &queued.task_id,
        plan_id,
        &error.message,
      )?;
      continue;
    }
    let plan_id = plan_id.ok_or_else(|| task_error("已校验的队列任务缺少采集计划"))?;
    if !gate_queued_run_for_claim(
      &transaction,
      &queued.id,
      &queued.task_id,
      plan_id,
      queued.current_stage.as_deref(),
    )? {
      continue;
    }

    let now = Utc::now().to_rfc3339();
    let stage = claim_stage(queued.current_stage.as_deref());
    let changed = transaction
      .execute(
        "UPDATE task_run
         SET status = 'running', current_stage = ?1, error_code = NULL,
             error_message = NULL, retryable = 0, claimed_at = ?2
         WHERE id = ?3 AND status = 'queued' AND plan_id IS NOT NULL
           AND EXISTS (
             SELECT 1 FROM collection_plan AS plan
             WHERE plan.id = task_run.plan_id AND plan.task_id = task_run.task_id
               AND plan.schema_version = 2 AND plan.validation_status = 'valid'
           )",
        params![stage, now, queued.id],
      )
      .map_err(database_error)?;
    if changed != 1 {
      return Err(task_error("队列任务状态已变化，无法领取"));
    }
    let task_changed = transaction
      .execute(
        "UPDATE collection_task SET status = 'running', updated_at = ?1
         WHERE id = ?2 AND status = 'queued'",
        params![now, queued.task_id],
      )
      .map_err(database_error)?;
    if task_changed != 1 {
      return Err(task_error("父任务状态已变化，无法原子领取队列任务"));
    }
    append_task_log(
      &transaction,
      &queued.id,
      stage,
      "info",
      if stage == "执行采集" {
        "本地执行器已领取任务"
      } else {
        "本地执行器已领取恢复任务"
      },
    )?;
    transaction.commit().map_err(database_error)?;

    return get_task_run(&connection, &queued.id).map(Some);
  }
}

pub fn complete_task_run(
  root_path: impl AsRef<Path>,
  run_id: &str,
  actual_cost_json: Value,
) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let run = get_task_run(&transaction, run_id)?;
  require_running(&run)?;
  let task = get_task_by_id(&transaction, &run.task_id)?;
  if task.status != "running" {
    return Err(task_error("任务已不在运行状态，不能标记成功"));
  }

  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "UPDATE task_run
       SET status = 'success', ended_at = ?1, current_stage = '已完成',
           cost_actual_json = ?2, retryable = 0
       WHERE id = ?3 AND status = 'running'",
      params![now, actual_cost_json.to_string(), run_id],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE collection_task
       SET status = 'success', completed_at = ?1, actual_cost_json = ?2, updated_at = ?1
       WHERE id = ?3 AND status = 'running'",
      params![now, actual_cost_json.to_string(), run.task_id],
    )
    .map_err(database_error)?;
  append_task_log(&transaction, run_id, "已完成", "info", "任务执行成功")?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, run_id)
}

pub fn fail_task_run(
  root_path: impl AsRef<Path>,
  run_id: &str,
  error_code: &str,
  error_message: &str,
  retryable: bool,
) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let run = get_task_run(&transaction, run_id)?;
  require_running(&run)?;
  let code = normalize_required("错误代码", error_code)?;
  let message = redact_sensitive_text(&normalize_required("错误信息", error_message)?);
  let now = Utc::now().to_rfc3339();

  transaction
    .execute(
      "UPDATE task_run
       SET status = 'failed', ended_at = ?1, current_stage = '执行失败', error_code = ?2,
           error_message = ?3, retryable = ?4
       WHERE id = ?5 AND status = 'running'",
      params![now, code, message, i64::from(retryable), run_id],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE collection_task SET status = 'failed', updated_at = ?1
       WHERE id = ?2 AND status = 'running'",
      params![now, run.task_id],
    )
    .map_err(database_error)?;
  append_task_log(&transaction, run_id, "执行失败", "error", &message)?;
  transaction.commit().map_err(database_error)?;

  get_task_run(&connection, run_id)
}

pub fn cancel_task(root_path: impl AsRef<Path>, task_id: &str) -> AppResult<CollectionTaskView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let task = get_task_by_id(&transaction, task_id)?;
  if matches!(task.status.as_str(), "success" | "cancelled") {
    return Err(task_error("成功或已取消任务不能再次取消"));
  }
  let active_runs = active_run_ids(&transaction, task_id)?;
  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "UPDATE collection_task SET status = 'cancelled', cancelled_at = ?1, updated_at = ?1
       WHERE id = ?2",
      params![now, task_id],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE task_run SET status = 'cancelled', ended_at = ?1, current_stage = '用户取消'
       WHERE task_id = ?2 AND status IN ('queued', 'running')",
      params![now, task_id],
    )
    .map_err(database_error)?;
  for run_id in active_runs {
    append_task_log(
      &transaction,
      &run_id,
      "用户取消",
      "warning",
      "任务已由用户取消",
    )?;
  }
  transaction.commit().map_err(database_error)?;
  get_task_by_id(&connection, task_id)
}

pub fn retry_task(
  root_path: impl AsRef<Path>,
  task_id: &str,
  stage: Option<String>,
) -> AppResult<TaskRunView> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = immediate_transaction(&mut connection)?;
  let task = get_task_by_id(&transaction, task_id)?;
  if task.status != "failed" {
    return Err(task_error("只有失败任务可以重试"));
  }
  let stage = stage
    .as_deref()
    .map(|value| normalize_required("重试阶段", value))
    .transpose()?
    .unwrap_or_else(|| "等待执行".to_string());
  if recovery_stage(Some(&stage)).is_some() {
    return Err(task_error("普通重试不能使用保留恢复阶段"));
  }
  let plan_id = latest_retryable_failed_plan_id(&transaction, task_id)?;
  require_executable_plan(&transaction, task_id, &plan_id)?;
  let attempt_number = next_attempt_number(&transaction, task_id, &plan_id)?;
  let run_id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "INSERT INTO task_run (
        id, task_id, plan_id, attempt_number, status, started_at, current_stage, retryable
      ) VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6, 0)",
      params![run_id, task_id, plan_id, attempt_number, now, stage],
    )
    .map_err(database_error)?;
  materialize_run_steps(&transaction, &run_id, &plan_id, &now)?;
  transaction
    .execute(
      "UPDATE collection_task SET status = 'queued', updated_at = ?1 WHERE id = ?2",
      params![now, task_id],
    )
    .map_err(database_error)?;
  append_task_log(&transaction, &run_id, &stage, "info", "失败任务已重新排队")?;
  transaction.commit().map_err(database_error)?;
  get_task_run(&connection, &run_id)
}

fn immediate_transaction(connection: &mut Connection) -> AppResult<Transaction<'_>> {
  Transaction::new(connection, TransactionBehavior::Immediate).map_err(database_error)
}

fn materialize_run_steps(
  connection: &Connection,
  run_id: &str,
  plan_id: &str,
  created_at: &str,
) -> AppResult<()> {
  let api_step_ids = {
    let mut statement = connection
      .prepare(
        "SELECT id FROM api_call_step
         WHERE plan_id = ?1
         ORDER BY step_order, id",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![plan_id], |row| row.get::<_, String>(0))
      .map_err(database_error)?;
    rows
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(database_error)?
  };
  if api_step_ids.is_empty() {
    return Err(task_error("采集计划没有可物化的运行步骤，不能创建运行记录"));
  }

  for api_step_id in &api_step_ids {
    let changed = connection
      .execute(
        "INSERT INTO task_run_step (
           id, task_run_id, api_call_step_id, status, created_at, updated_at
         ) VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
        params![Uuid::new_v4().to_string(), run_id, api_step_id, created_at],
      )
      .map_err(database_error)?;
    if changed != 1 {
      return Err(task_error("运行步骤快照写入不完整，已取消创建运行记录"));
    }
  }

  let materialized_count = connection
    .query_row(
      "SELECT COUNT(*)
       FROM task_run_step AS run_step
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1 AND api_step.plan_id = ?2
         AND run_step.status = 'pending' AND run_step.stop_reason IS NULL
         AND run_step.started_at IS NULL AND run_step.completed_at IS NULL",
      params![run_id, plan_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  let expected_count =
    i64::try_from(api_step_ids.len()).map_err(|_| task_error("采集计划步骤数量超出可执行范围"))?;
  if materialized_count != expected_count {
    return Err(task_error("运行步骤快照数量不完整，已取消创建运行记录"));
  }
  Ok(())
}

fn claim_stage(current_stage: Option<&str>) -> &'static str {
  recovery_stage(current_stage).unwrap_or("执行采集")
}

fn confirmed_plan_id(connection: &Connection, task_id: &str) -> AppResult<String> {
  let (count, plan_id) = connection
    .query_row(
      "SELECT COUNT(*), MIN(id)
       FROM collection_plan
       WHERE task_id = ?1 AND confirmed_by_user = 1 AND validation_status = 'valid'",
      params![task_id],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .map_err(database_error)?;

  let plan_id = match (count, plan_id) {
    (1, Some(plan_id)) => plan_id,
    (0, _) => {
      return Err(task_error(
        "任务没有唯一且有效的已确认采集计划，请重新确认后再运行",
      ))
    }
    _ => {
      return Err(task_error(
        "任务存在多个已确认采集计划，无法确定唯一执行计划",
      ))
    }
  };
  if latest_plan_for_task(connection, task_id)?.id != plan_id {
    return Err(task_error(
      "当前确认的采集计划不是最新计划，请重新确认最新采集计划",
    ));
  }
  require_executable_plan(connection, task_id, &plan_id)?;
  Ok(plan_id)
}

pub(super) fn require_executable_plan(
  connection: &Connection,
  task_id: &str,
  plan_id: &str,
) -> AppResult<()> {
  let plan = connection
    .query_row(
      "SELECT task_id, schema_version, validation_status, plan_json
       FROM collection_plan WHERE id = ?1",
      params![plan_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, String>(2)?,
          row.get::<_, String>(3)?,
        ))
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| task_error("任务绑定的采集计划不存在，请重新确认后再运行"))?;

  if plan.0 != task_id {
    return Err(task_error("任务运行绑定了其他任务的采集计划"));
  }
  if plan.1 != 2 {
    return Err(task_error(format!(
      "schema_version={} 的采集计划不能执行，必须重新确认有效的 v2 计划",
      plan.1
    )));
  }
  if plan.2 != "valid" {
    return Err(task_error("采集计划不是 valid 状态，不能执行"));
  }

  let plan_json = serde_json::from_str::<Value>(&plan.3)
    .map_err(|_| task_error("采集计划 v2 不是合法 JSON，不能执行"))?;
  let task = get_task_by_id(connection, task_id)?;
  let mut validation_errors = validate_plan_for_task(&task, &plan_json);
  validation_errors.extend(validate_collection_plan_v2(&plan_json).errors);
  validation_errors.sort();
  validation_errors.dedup();
  if !validation_errors.is_empty() {
    return Err(task_error(format!(
      "采集计划 v2 已损坏或不再满足执行条件：{}",
      validation_errors.join("；")
    )));
  }
  require_persisted_steps_match_plan(connection, plan_id, &plan_json)?;

  Ok(())
}

fn require_persisted_steps_match_plan(
  connection: &Connection,
  plan_id: &str,
  plan_json: &Value,
) -> AppResult<()> {
  let expected_steps = plan_json
    .get("steps")
    .and_then(Value::as_array)
    .ok_or_else(|| task_error("采集计划 v2 缺少步骤，不能执行"))?;
  let request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .ok_or_else(|| task_error("采集计划 v2 缺少有效请求上限，不能执行"))?;
  let persisted_steps = {
    let mut statement = connection
      .prepare(
        "SELECT step_order, platform, data_type, endpoint_key, params_json, status,
                request_count_estimate, cost_estimate_json
         FROM api_call_step WHERE plan_id = ?1 ORDER BY step_order, id",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![plan_id], |row| {
        Ok(PersistedPlanStep {
          order: row.get(0)?,
          platform: row.get(1)?,
          data_type: row.get(2)?,
          endpoint_key: row.get(3)?,
          params_json: row.get(4)?,
          status: row.get(5)?,
          request_count_estimate: row.get(6)?,
          cost_estimate_json: row.get(7)?,
        })
      })
      .map_err(database_error)?;
    rows
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(database_error)?
  };

  if persisted_steps.len() != expected_steps.len() {
    return Err(task_error("采集计划与持久化步骤数量不一致，不能执行"));
  }

  for (index, (persisted, expected)) in persisted_steps.iter().zip(expected_steps).enumerate() {
    let expected = expected
      .as_object()
      .ok_or_else(|| task_error("采集计划 v2 包含无效步骤，不能执行"))?;
    let expected_params = expected
      .get("params")
      .cloned()
      .unwrap_or_else(|| serde_json::json!({}));
    let persisted_params = serde_json::from_str::<Value>(&persisted.params_json)
      .map_err(|_| task_error("持久化采集步骤参数不是合法 JSON，不能执行"))?;
    let persisted_cost = serde_json::from_str::<Value>(&persisted.cost_estimate_json)
      .map_err(|_| task_error("持久化采集步骤成本不是合法 JSON，不能执行"))?;
    let expected_cost = serde_json::json!({ "request_count_estimate": request_limit });
    let matches_plan = persisted.order == index as i64
      && expected.get("platform").and_then(Value::as_str) == Some(&persisted.platform)
      && expected.get("data_type").and_then(Value::as_str) == Some(&persisted.data_type)
      && expected.get("endpoint_key").and_then(Value::as_str) == Some(&persisted.endpoint_key)
      && persisted_params == expected_params
      && persisted.status == "planned"
      && persisted.request_count_estimate == request_limit
      && persisted_cost == expected_cost;
    if !matches_plan {
      return Err(task_error(format!(
        "采集计划与持久化步骤 {} 不一致，不能执行",
        index + 1
      )));
    }
  }

  Ok(())
}

fn run_has_sent_request_evidence(connection: &Connection, run_id: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(
         SELECT 1 FROM collection_page_checkpoint AS checkpoint
         JOIN task_run_step AS run_step ON run_step.id = checkpoint.task_run_step_id
         WHERE run_step.task_run_id = ?1 AND (
           checkpoint.status IN ('requesting','uncertain','response_received','completed')
           OR checkpoint.page_index > 0 OR checkpoint.input_cursor_json IS NOT NULL
           OR checkpoint.request_attempt_count > 0 OR checkpoint.retry_count > 0
           OR checkpoint.fallback_count > 0 OR checkpoint.final_endpoint_key IS NOT NULL
           OR checkpoint.provider_response_json IS NOT NULL
           OR checkpoint.provider_response_hash IS NOT NULL
           OR checkpoint.provider_response_size IS NOT NULL OR checkpoint.has_more IS NOT NULL
           OR checkpoint.next_cursor_json IS NOT NULL OR checkpoint.record_count_received > 0
           OR checkpoint.record_count_persisted > 0
           OR TRIM(checkpoint.cost_actual_json) <> '{}'
           OR checkpoint.requested_at IS NOT NULL OR checkpoint.response_received_at IS NOT NULL
           OR checkpoint.committed_at IS NOT NULL
         )
       )",
      params![run_id],
      |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
    .map_err(database_error)
}

pub(super) fn quarantine_run_for_reconfirmation(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  plan_id: Option<&str>,
  reason: &str,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let has_sent_evidence = run_has_sent_request_evidence(connection, run_id)?;
  let snapshot_allows_reconfirmation = plan_id
    .map(|plan_id| run_snapshot_allows_plan_reconfirmation(connection, run_id, plan_id))
    .transpose()?
    .unwrap_or(false);
  let requires_manual_review = has_sent_evidence || !snapshot_allows_reconfirmation;
  let (stage, code, message) = if has_sent_evidence {
    (
      "请求证据需要人工处理",
      "REQUEST_EVIDENCE_REQUIRES_REVIEW",
      redact_sensitive_text(&format!(
        "采集计划不可执行，且运行记录包含已发送请求证据，禁止重新入队，必须人工处理：{reason}"
      )),
    )
  } else if !snapshot_allows_reconfirmation {
    (
      "运行快照需要人工处理",
      "RUN_SNAPSHOT_REQUIRES_REVIEW",
      redact_sensitive_text(&format!(
        "采集计划不可执行，且运行快照无法证明请求从未发送，禁止重新入队，必须人工处理：{reason}"
      )),
    )
  } else {
    (
      "需要重新确认计划",
      "PLAN_RECONFIRMATION_REQUIRED",
      redact_sensitive_text(&format!(
        "采集计划不可执行，任务已停止，请重新确认有效的 v2 计划：{reason}"
      )),
    )
  };
  let changed = connection
    .execute(
      "UPDATE task_run
       SET status = 'failed', ended_at = ?1, current_stage = ?2,
           error_code = ?3, error_message = ?4,
           retryable = 0, claimed_at = NULL
       WHERE id = ?5 AND task_id = ?6 AND status IN ('queued', 'running')",
      params![now, stage, code, message, run_id, task_id],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("活动任务状态已变化，无法隔离无效采集计划"));
  }
  connection
    .execute(
      "UPDATE task_run_step
       SET status = 'failed', stop_reason = 'terminal_error',
           completed_at = COALESCE(completed_at, ?1), updated_at = ?1
       WHERE task_run_id = ?2 AND status IN ('pending', 'running')",
      params![now, run_id],
    )
    .map_err(database_error)?;

  if let Some(plan_id) = plan_id {
    connection
      .execute(
        "UPDATE collection_plan
         SET validation_status = 'needs_review', validation_errors_json = ?1,
             confirmed_by_user = 0, updated_at = ?2
         WHERE id = ?3 AND task_id = ?4",
        params![
          serde_json::json!([message]).to_string(),
          now,
          plan_id,
          task_id
        ],
      )
      .map_err(database_error)?;
  }
  append_task_log(connection, run_id, stage, "error", &message)?;

  let has_other_active_runs = connection
    .query_row(
      "SELECT EXISTS(
         SELECT 1 FROM task_run
         WHERE task_id = ?1 AND status IN ('queued', 'running')
       )",
      params![task_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?
    != 0;
  if !has_other_active_runs {
    let task_changed = if requires_manual_review {
      connection
        .execute(
          "UPDATE collection_task SET status = 'failed', updated_at = ?1
           WHERE id = ?2 AND status IN ('queued', 'running', 'waiting_confirmation')",
          params![now, task_id],
        )
        .map_err(database_error)?
    } else {
      connection
        .execute(
          "UPDATE collection_task
           SET status = 'waiting_confirmation', confirmed_at = NULL,
               completed_at = NULL, cancelled_at = NULL, updated_at = ?1
           WHERE id = ?2 AND status IN ('queued', 'running')",
          params![now, task_id],
        )
        .map_err(database_error)?
    };
    if requires_manual_review && task_changed != 1 {
      return Err(task_error("父任务状态已变化，无法原子隔离无效采集计划"));
    }
    if !requires_manual_review && task_changed == 1 {
      connection
        .execute(
          "UPDATE collection_plan
           SET confirmed_by_user = 0, updated_at = ?1
           WHERE task_id = ?2 AND confirmed_by_user = 1",
          params![now, task_id],
        )
        .map_err(database_error)?;
    }
  }

  Ok(())
}

fn latest_retryable_failed_plan_id(connection: &Connection, task_id: &str) -> AppResult<String> {
  let (plan_id, retryable) = connection
    .query_row(
      "SELECT plan_id, retryable
       FROM task_run
       WHERE task_id = ?1 AND status = 'failed'
       ORDER BY started_at DESC, id DESC
       LIMIT 1",
      params![task_id],
      |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)? != 0)),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| task_error("任务没有可重试的失败运行记录"))?;

  if !retryable {
    return Err(task_error(
      "最近失败运行不可直接重试，请先完成明确的人工处理或重新确认流程",
    ));
  }

  plan_id.ok_or_else(|| task_error("历史任务运行缺少已确认计划，请重新确认采集计划后再运行"))
}

fn next_attempt_number(connection: &Connection, task_id: &str, plan_id: &str) -> AppResult<i64> {
  let latest = connection
    .query_row(
      "SELECT MAX(attempt_number) FROM task_run WHERE task_id = ?1 AND plan_id = ?2",
      params![task_id, plan_id],
      |row| row.get::<_, Option<i64>>(0),
    )
    .map_err(database_error)?
    .unwrap_or(0);

  latest
    .checked_add(1)
    .ok_or_else(|| task_error("任务运行尝试次数已达到上限"))
}

fn require_running(run: &TaskRunView) -> AppResult<()> {
  if run.status == "running" {
    Ok(())
  } else {
    Err(task_error("只有运行中的任务记录可以结束"))
  }
}

fn active_run_ids(connection: &Connection, task_id: &str) -> AppResult<Vec<String>> {
  let mut statement = connection
    .prepare("SELECT id FROM task_run WHERE task_id = ?1 AND status IN ('queued', 'running')")
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], |row| row.get::<_, String>(0))
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

pub(super) fn append_task_log(
  connection: &Connection,
  run_id: &str,
  stage: &str,
  level: &str,
  message: &str,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO task_log (id, task_run_id, stage, level, message, safe_details_json, created_at)
       VALUES (?1, ?2, ?3, ?4, ?5, '{}', ?6)",
      params![
        Uuid::new_v4().to_string(),
        run_id,
        stage,
        level,
        message,
        Utc::now().to_rfc3339()
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}
