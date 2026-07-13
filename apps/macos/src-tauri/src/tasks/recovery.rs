use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, TransactionBehavior};
use serde_json::Value;

use crate::domain::{AppErrorCode, AppResult};

mod checkpoint;

use checkpoint::{
  checkpoint_chains_are_valid, checkpoint_has_complete_evidence, has_checkpoint_state_conflict,
  latest_completed_next_step, retry_limit_stop, run_steps_are_compatible,
};

use super::execution::{
  append_task_log, quarantine_run_for_reconfirmation, require_executable_plan,
};
use super::{database_error, open_workspace_connection, task_error};

const UNCERTAIN_REQUEST_CODE: &str = "UNCERTAIN_REQUEST_AFTER_CRASH";

#[derive(Default)]
struct CheckpointState {
  step_id: String,
  page_index: i64,
  input_cursor_json: Option<String>,
  status: String,
  request_attempt_count: i64,
  record_count_received: i64,
  record_count_persisted: i64,
  cost_actual_json: String,
  retryable: bool,
  has_more: Option<bool>,
  next_cursor_json: Option<String>,
  provider_response_json: Option<String>,
  provider_response_hash: Option<String>,
  provider_response_size: Option<i64>,
  requested_at: Option<String>,
  response_received_at: Option<String>,
  committed_at: Option<String>,
  platform: String,
  data_type: String,
  params_json: String,
}

struct RunStepState {
  id: String,
  status: String,
}

struct RecoveryLimits {
  request_limit: i64,
  record_limit: i64,
  budget_micros: i64,
}

type SnapshotCounts = (i64, i64, i64, i64, i64, i64, i64, i64);
enum RecoveryAction {
  Resume {
    stage: &'static str,
    message: &'static str,
  },
  Stop {
    stage: &'static str,
    code: &'static str,
    message: &'static str,
  },
  MarkRequestingUncertain,
}

pub(super) fn recovery_stage(current_stage: Option<&str>) -> Option<&'static str> {
  match current_stage {
    Some("恢复响应入库") => Some("恢复响应入库"),
    Some("恢复重试") => Some("恢复重试"),
    Some("恢复待发送") => Some("恢复待发送"),
    Some("恢复续页") => Some("恢复续页"),
    Some("恢复收尾") => Some("恢复收尾"),
    Some("恢复等待") => Some("恢复等待"),
    _ => None,
  }
}

pub(super) fn quarantine_queued_request_uncertainty(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
) -> AppResult<bool> {
  let requesting = checkpoint_status_exists(connection, run_id, "requesting")?;
  if requesting {
    mark_requesting_uncertain(connection, run_id)?;
  }
  if requesting || checkpoint_status_exists(connection, run_id, "uncertain")? {
    stop_queued_run(
      connection,
      run_id,
      task_id,
      "请求状态不确定",
      UNCERTAIN_REQUEST_CODE,
      "队列中存在可能已发送的 TikHub 请求，远端副作用无法确认，禁止自动重发",
    )?;
    return Ok(true);
  }
  Ok(false)
}

pub(super) fn gate_queued_run_for_claim(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  plan_id: &str,
  current_stage: Option<&str>,
) -> AppResult<bool> {
  let expected_recovery_stage = recovery_stage(current_stage);
  let (complete, pristine, _) = run_snapshot_state(connection, run_id, plan_id)?;
  if expected_recovery_stage.is_none() && pristine {
    if complete {
      return Ok(true);
    }
    stop_queued_run(
      connection,
      run_id,
      task_id,
      "运行快照不完整",
      "RUN_STEP_SNAPSHOT_INCOMPLETE",
      "运行步骤快照不完整，可能丢失远端请求证据，已停止自动执行",
    )?;
    return Ok(false);
  }

  match classify_recovery(connection, run_id, plan_id)? {
    RecoveryAction::MarkRequestingUncertain => {
      mark_requesting_uncertain(connection, run_id)?;
      stop_queued_run(
        connection,
        run_id,
        task_id,
        "请求状态不确定",
        UNCERTAIN_REQUEST_CODE,
        "队列中存在可能已发送的 TikHub 请求，远端副作用无法确认，禁止自动重发",
      )?;
    }
    RecoveryAction::Stop {
      stage,
      code,
      message,
    } => stop_queued_run(connection, run_id, task_id, stage, code, message)?,
    RecoveryAction::Resume { stage, .. } if complete && Some(stage) == expected_recovery_stage => {
      return Ok(true)
    }
    RecoveryAction::Resume { .. } if !complete => stop_queued_run(
      connection,
      run_id,
      task_id,
      "运行快照不完整",
      "RUN_STEP_SNAPSHOT_INCOMPLETE",
      "运行步骤快照不完整，可能丢失远端请求证据，已停止自动执行",
    )?,
    RecoveryAction::Resume { .. } => stop_queued_run(
      connection,
      run_id,
      task_id,
      "恢复指令冲突",
      "CHECKPOINT_STATE_CONFLICT",
      "队列恢复指令与运行步骤及检查点证据不一致，已停止自动执行",
    )?,
  }
  Ok(false)
}

pub fn recover_interrupted_runs(root_path: impl AsRef<Path>) -> AppResult<i64> {
  let mut connection = open_workspace_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let interrupted = {
    let mut statement = transaction
      .prepare(
        "SELECT id, task_id, plan_id
         FROM task_run WHERE status = 'running'
         ORDER BY started_at, id",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map([], |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      })
      .map_err(database_error)?;
    rows
      .collect::<rusqlite::Result<Vec<_>>>()
      .map_err(database_error)?
  };
  let mut recoverable = Vec::new();

  for (run_id, task_id, plan_id) in interrupted {
    if checkpoint_status_exists(&transaction, &run_id, "requesting")? {
      mark_requesting_uncertain(&transaction, &run_id)?;
      stop_run(
        &transaction,
        &run_id,
        &task_id,
        "请求状态不确定",
        UNCERTAIN_REQUEST_CODE,
        "进程在 TikHub 请求完成前中断，无法确认远端是否已计费或返回，禁止自动重发",
      )?;
      continue;
    }
    if checkpoint_status_exists(&transaction, &run_id, "uncertain")? {
      stop_run(
        &transaction,
        &run_id,
        &task_id,
        "请求状态不确定",
        UNCERTAIN_REQUEST_CODE,
        "任务包含状态不确定的 TikHub 请求，必须人工确认后再处理",
      )?;
      continue;
    }

    let plan_validation = plan_id.as_deref().map_or_else(
      || Err(task_error("运行中的任务缺少采集计划，不能恢复")),
      |plan_id| require_executable_plan(&transaction, &task_id, plan_id),
    );
    match plan_validation {
      Ok(()) => {}
      Err(error) if matches!(&error.code, AppErrorCode::ValidationError) => {
        quarantine_run_for_reconfirmation(
          &transaction,
          &run_id,
          &task_id,
          plan_id.as_deref(),
          &error.message,
        )?;
        continue;
      }
      Err(error) => return Err(error),
    }

    let plan_id = plan_id.ok_or_else(|| task_error("已校验的运行记录缺少采集计划"))?;
    match classify_recovery(&transaction, &run_id, &plan_id)? {
      RecoveryAction::Resume { stage, message } => {
        recoverable.push((run_id, task_id, stage, message));
      }
      RecoveryAction::Stop {
        stage,
        code,
        message,
      } => stop_run(&transaction, &run_id, &task_id, stage, code, message)?,
      RecoveryAction::MarkRequestingUncertain => {
        mark_requesting_uncertain(&transaction, &run_id)?;
        stop_run(
          &transaction,
          &run_id,
          &task_id,
          "请求状态不确定",
          UNCERTAIN_REQUEST_CODE,
          "进程在 TikHub 请求完成前中断，无法确认远端是否已计费或返回，禁止自动重发",
        )?;
      }
    }
  }

  let now = Utc::now().to_rfc3339();
  for (run_id, task_id, stage, message) in &recoverable {
    transaction
      .execute(
        "UPDATE task_run
         SET status = 'queued', current_stage = ?1, ended_at = NULL,
             error_code = NULL, error_message = NULL, retryable = 0, claimed_at = NULL
         WHERE id = ?2 AND status = 'running'",
        params![stage, run_id],
      )
      .map_err(database_error)?;
    transaction
      .execute(
        "UPDATE collection_task SET status = 'queued', updated_at = ?1
         WHERE id = ?2 AND status = 'running'",
        params![now, task_id],
      )
      .map_err(database_error)?;
    append_task_log(&transaction, run_id, stage, "warning", message)?;
  }
  let recovered = recoverable.len() as i64;
  transaction.commit().map_err(database_error)?;
  Ok(recovered)
}

fn classify_recovery(
  connection: &Connection,
  run_id: &str,
  plan_id: &str,
) -> AppResult<RecoveryAction> {
  let run_step_statuses = load_run_step_statuses(connection, run_id)?;
  let checkpoints = load_checkpoints(connection, run_id)?;

  if checkpoints
    .iter()
    .any(|checkpoint| checkpoint.status == "requesting")
  {
    return Ok(RecoveryAction::MarkRequestingUncertain);
  }
  if checkpoints
    .iter()
    .any(|checkpoint| checkpoint.status == "uncertain")
  {
    return Ok(RecoveryAction::Stop {
      stage: "请求状态不确定",
      code: UNCERTAIN_REQUEST_CODE,
      message: "任务包含状态不确定的 TikHub 请求，必须人工确认后再处理",
    });
  }
  if !run_step_snapshot_is_complete(connection, plan_id, &run_step_statuses, &checkpoints)? {
    return Ok(RecoveryAction::Stop {
      stage: "运行快照不完整",
      code: "CHECKPOINT_EVIDENCE_INCOMPLETE",
      message: "运行步骤快照不完整，或运行中步骤缺少检查点，禁止自动重发",
    });
  }
  if has_checkpoint_state_conflict(&checkpoints) {
    return Ok(RecoveryAction::Stop {
      stage: "检查点状态冲突",
      code: "CHECKPOINT_STATE_CONFLICT",
      message: "任务存在多个冲突的恢复前沿，无法安全判断下一执行位置",
    });
  }
  if !checkpoint_chains_are_valid(&checkpoints) {
    return Ok(RecoveryAction::Stop {
      stage: "检查点状态冲突",
      code: "CHECKPOINT_STATE_CONFLICT",
      message: "检查点页码或游标链不连续，无法安全判断恢复位置",
    });
  }
  if !run_steps_are_compatible(&run_step_statuses, &checkpoints) {
    return Ok(RecoveryAction::Stop {
      stage: "运行步骤状态冲突",
      code: "CHECKPOINT_STATE_CONFLICT",
      message: "运行步骤状态与检查点证据不相容，已停止自动恢复",
    });
  }
  if checkpoints
    .iter()
    .any(|checkpoint| !checkpoint_has_complete_evidence(checkpoint))
  {
    return Ok(RecoveryAction::Stop {
      stage: "检查点证据不完整",
      code: "CHECKPOINT_EVIDENCE_INCOMPLETE",
      message: "已接收或已提交的检查点缺少可验证响应、提交时间或续页游标",
    });
  }
  if checkpoints
    .iter()
    .any(|checkpoint| checkpoint.status == "failed" && !checkpoint.retryable)
    || run_step_statuses
      .iter()
      .any(|step| matches!(step.status.as_str(), "failed" | "cancelled"))
  {
    return Ok(RecoveryAction::Stop {
      stage: "检查点终止失败",
      code: "CHECKPOINT_TERMINAL_FAILURE",
      message: "任务包含不可重试的失败检查点，已停止自动恢复",
    });
  }
  if checkpoints
    .iter()
    .any(|checkpoint| checkpoint.status == "response_received")
  {
    return Ok(RecoveryAction::Resume {
      stage: "恢复响应入库",
      message: "TikHub 响应已保存，恢复时只继续本地入库，不重新发送请求",
    });
  }
  if checkpoints
    .iter()
    .any(|checkpoint| checkpoint.status == "failed" && checkpoint.retryable)
  {
    let step_id = checkpoints
      .iter()
      .find(|checkpoint| checkpoint.status == "failed" && checkpoint.retryable)
      .map(|checkpoint| checkpoint.step_id.as_str())
      .ok_or_else(|| task_error("可重试检查点缺少运行步骤"))?;
    return resume_remote_request(
      connection,
      plan_id,
      &checkpoints,
      step_id,
      "恢复重试",
      "失败检查点仍在请求、记录和预算限制内，等待安全重试",
    );
  }
  if checkpoints
    .iter()
    .any(|checkpoint| checkpoint.status == "prepared")
  {
    let step_id = checkpoints
      .iter()
      .find(|checkpoint| checkpoint.status == "prepared")
      .map(|checkpoint| checkpoint.step_id.as_str())
      .ok_or_else(|| task_error("prepared 检查点缺少运行步骤"))?;
    return resume_remote_request(
      connection,
      plan_id,
      &checkpoints,
      step_id,
      "恢复待发送",
      "检查点仍处于 prepared，可从尚未发送的请求继续",
    );
  }
  if checkpoints
    .iter()
    .any(|checkpoint| checkpoint.status == "completed")
  {
    if let Some(step_id) = latest_completed_next_step(&checkpoints) {
      return resume_remote_request(
        connection,
        plan_id,
        &checkpoints,
        step_id,
        "恢复续页",
        "从已提交检查点的 next_cursor 继续下一页",
      );
    }
    if let Some(step) = next_eligible_pending_step(&run_step_statuses) {
      return resume_remote_request(
        connection,
        plan_id,
        &checkpoints,
        &step.id,
        "恢复待发送",
        "已完成步骤没有续页，继续下一个尚未发送的运行步骤",
      );
    }
    return Ok(RecoveryAction::Resume {
      stage: "恢复收尾",
      message: "最后一个检查点已提交且没有续页，等待完成本地收尾",
    });
  }
  if let Some(step) = next_eligible_pending_step(&run_step_statuses) {
    return resume_remote_request(
      connection,
      plan_id,
      &checkpoints,
      &step.id,
      "恢复待发送",
      "运行步骤尚未发送请求，可从待执行步骤继续",
    );
  }

  Ok(RecoveryAction::Resume {
    stage: "恢复等待",
    message: "未发现已发送请求的检查点，任务已重新排队",
  })
}

fn checkpoint_status_exists(
  connection: &Connection,
  run_id: &str,
  status: &str,
) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(
         SELECT 1 FROM collection_page_checkpoint AS checkpoint
         JOIN task_run_step AS run_step ON run_step.id = checkpoint.task_run_step_id
         WHERE run_step.task_run_id = ?1 AND checkpoint.status = ?2
       )",
      params![run_id, status],
      |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
    .map_err(database_error)
}

fn run_step_snapshot_is_complete(
  connection: &Connection,
  plan_id: &str,
  run_steps: &[RunStepState],
  checkpoints: &[CheckpointState],
) -> AppResult<bool> {
  let expected_step_count = connection
    .query_row(
      "SELECT COUNT(*) FROM api_call_step WHERE plan_id = ?1",
      params![plan_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  let actual_step_count =
    i64::try_from(run_steps.len()).map_err(|_| task_error("运行步骤数量超出可恢复范围"))?;
  if expected_step_count == 0 || actual_step_count != expected_step_count {
    return Ok(false);
  }
  Ok(!run_steps.iter().any(|step| {
    step.status == "running"
      && !checkpoints
        .iter()
        .any(|checkpoint| checkpoint.step_id == step.id)
  }))
}

fn next_eligible_pending_step(run_steps: &[RunStepState]) -> Option<&RunStepState> {
  let index = run_steps.iter().position(|step| step.status == "pending")?;
  run_steps[..index]
    .iter()
    .all(|step| step.status == "success")
    .then_some(&run_steps[index])
}

fn load_run_step_statuses(connection: &Connection, run_id: &str) -> AppResult<Vec<RunStepState>> {
  let mut statement = connection
    .prepare(
      "SELECT run_step.id, run_step.status
       FROM task_run_step AS run_step
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1
       ORDER BY api_step.step_order, run_step.id",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_id], |row| {
      Ok(RunStepState {
        id: row.get(0)?,
        status: row.get(1)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn load_checkpoints(connection: &Connection, run_id: &str) -> AppResult<Vec<CheckpointState>> {
  let mut statement = connection
    .prepare(
      "SELECT checkpoint.task_run_step_id, checkpoint.page_index, checkpoint.status,
              checkpoint.request_attempt_count, checkpoint.record_count_persisted,
              checkpoint.cost_actual_json, checkpoint.retryable, checkpoint.has_more,
              checkpoint.next_cursor_json, checkpoint.provider_response_json,
              checkpoint.provider_response_hash, checkpoint.provider_response_size,
              checkpoint.response_received_at, checkpoint.committed_at,
              checkpoint.input_cursor_json, checkpoint.record_count_received,
              checkpoint.requested_at, api_step.platform, api_step.data_type,
              api_step.params_json
       FROM collection_page_checkpoint AS checkpoint
       JOIN task_run_step AS run_step ON run_step.id = checkpoint.task_run_step_id
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1
       ORDER BY checkpoint.task_run_step_id, checkpoint.page_index",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![run_id], |row| {
      Ok(CheckpointState {
        step_id: row.get(0)?,
        page_index: row.get(1)?,
        input_cursor_json: row.get(14)?,
        status: row.get(2)?,
        request_attempt_count: row.get(3)?,
        record_count_received: row.get(15)?,
        record_count_persisted: row.get(4)?,
        cost_actual_json: row.get(5)?,
        retryable: row.get::<_, i64>(6)? != 0,
        has_more: row.get::<_, Option<i64>>(7)?.map(|value| value != 0),
        next_cursor_json: row.get(8)?,
        provider_response_json: row.get(9)?,
        provider_response_hash: row.get(10)?,
        provider_response_size: row.get(11)?,
        requested_at: row.get(16)?,
        response_received_at: row.get(12)?,
        committed_at: row.get(13)?,
        platform: row.get(17)?,
        data_type: row.get(18)?,
        params_json: row.get(19)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn load_recovery_limits(connection: &Connection, plan_id: &str) -> AppResult<RecoveryLimits> {
  let plan_json = connection
    .query_row(
      "SELECT plan_json FROM collection_plan WHERE id = ?1",
      params![plan_id],
      |row| row.get::<_, String>(0),
    )
    .map_err(database_error)?;
  let plan_json = serde_json::from_str::<Value>(&plan_json)
    .map_err(|_| task_error("采集计划 v2 不是合法 JSON，无法恢复"))?;
  let request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0)
    .ok_or_else(|| task_error("采集计划缺少有效 request_limit，无法恢复"))?;
  let record_limit = plan_json
    .get("record_limit")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0)
    .ok_or_else(|| task_error("采集计划缺少有效 record_limit，无法恢复"))?;
  let budget_micros = plan_json
    .pointer("/budget_limit/amount_micros")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0)
    .ok_or_else(|| task_error("采集计划缺少有效 budget_limit，无法恢复"))?;
  Ok(RecoveryLimits {
    request_limit,
    record_limit,
    budget_micros,
  })
}

fn resume_remote_request(
  connection: &Connection,
  plan_id: &str,
  checkpoints: &[CheckpointState],
  step_id: &str,
  stage: &'static str,
  message: &'static str,
) -> AppResult<RecoveryAction> {
  let limits = load_recovery_limits(connection, plan_id)?;
  if let Some(stop) = retry_limit_stop(checkpoints, &limits, step_id) {
    return Ok(stop);
  }
  Ok(RecoveryAction::Resume { stage, message })
}

fn run_snapshot_state(
  connection: &Connection,
  run_id: &str,
  plan_id: &str,
) -> AppResult<(bool, bool, bool)> {
  let (expected, matching, total, dirty, checkpoints, unsafe_steps, unsafe_checkpoints, running): SnapshotCounts = connection
    .query_row(
      "SELECT
         (SELECT COUNT(*) FROM api_call_step WHERE plan_id = ?2),
         (SELECT COUNT(*) FROM task_run_step AS run_step
          JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
          WHERE run_step.task_run_id = ?1 AND api_step.plan_id = ?2),
         (SELECT COUNT(*) FROM task_run_step WHERE task_run_id = ?1),
         (SELECT COUNT(*) FROM task_run_step WHERE task_run_id = ?1 AND (
            status <> 'pending' OR stop_reason IS NOT NULL OR started_at IS NOT NULL
            OR completed_at IS NOT NULL
          )),
         (SELECT COUNT(*) FROM collection_page_checkpoint AS checkpoint
          JOIN task_run_step AS run_step ON run_step.id = checkpoint.task_run_step_id
          WHERE run_step.task_run_id = ?1),
         (SELECT COUNT(*) FROM task_run_step WHERE task_run_id = ?1 AND (
            status NOT IN ('pending','running') OR stop_reason IS NOT NULL
            OR completed_at IS NOT NULL OR (status = 'pending' AND started_at IS NOT NULL)
            OR (status = 'running' AND started_at IS NULL)
          )),
         (SELECT COUNT(*) FROM collection_page_checkpoint AS checkpoint
          JOIN task_run_step AS run_step ON run_step.id = checkpoint.task_run_step_id
          WHERE run_step.task_run_id = ?1 AND (
            checkpoint.status <> 'prepared' OR checkpoint.retryable <> 0
            OR checkpoint.last_error_code IS NOT NULL OR checkpoint.last_error_message IS NOT NULL
            OR checkpoint.page_index <> 0
            OR checkpoint.input_cursor_json IS NOT NULL OR run_step.status <> 'running'
          )),
         (SELECT COUNT(*) FROM task_run_step WHERE task_run_id = ?1 AND status = 'running')",
      params![run_id, plan_id],
      |row| {
        Ok((
          row.get(0)?,
          row.get(1)?,
          row.get(2)?,
          row.get(3)?,
          row.get(4)?,
          row.get(5)?,
          row.get(6)?,
          row.get(7)?,
        ))
      },
    )
    .map_err(database_error)?;
  Ok((
    expected > 0 && expected == matching && matching == total,
    dirty == 0 && checkpoints == 0,
    expected > 0
      && expected == matching
      && matching == total
      && unsafe_steps == 0
      && unsafe_checkpoints == 0
      && running == checkpoints
      && running <= 1,
  ))
}
pub(super) fn run_snapshot_allows_plan_reconfirmation(
  connection: &Connection,
  run_id: &str,
  plan_id: &str,
) -> AppResult<bool> {
  run_snapshot_state(connection, run_id, plan_id).map(|state| state.2)
}

fn stop_queued_run(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  stage: &str,
  code: &str,
  message: &str,
) -> AppResult<()> {
  stop_run_in_state(connection, run_id, task_id, stage, code, message, "queued")
}

fn mark_requesting_uncertain(connection: &Connection, run_id: &str) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let message = "进程中断时请求处于 requesting，远端副作用无法确认";
  let changed = connection
    .execute(
      "UPDATE collection_page_checkpoint
       SET status = 'uncertain', retryable = 0, last_error_code = ?1,
           last_error_message = ?2, updated_at = ?3
       WHERE status = 'requesting' AND task_run_step_id IN (
         SELECT id FROM task_run_step WHERE task_run_id = ?4
       )",
      params![UNCERTAIN_REQUEST_CODE, message, now, run_id],
    )
    .map_err(database_error)?;
  if changed == 0 {
    return Err(task_error(
      "requesting 检查点状态已变化，无法标记 uncertain",
    ));
  }
  Ok(())
}

fn stop_run(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  stage: &str,
  code: &str,
  message: &str,
) -> AppResult<()> {
  stop_run_in_state(connection, run_id, task_id, stage, code, message, "running")
}

fn stop_run_in_state(
  connection: &Connection,
  run_id: &str,
  task_id: &str,
  stage: &str,
  code: &str,
  message: &str,
  expected_status: &str,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  let changed = connection
    .execute(
      "UPDATE task_run
       SET status = 'failed', ended_at = ?1, current_stage = ?2,
           error_code = ?3, error_message = ?4, retryable = 0, claimed_at = NULL
       WHERE id = ?5 AND task_id = ?6 AND status = ?7",
      params![now, stage, code, message, run_id, task_id, expected_status],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(task_error("活动运行状态已变化，无法安全停止"));
  }
  connection
    .execute(
      "UPDATE task_run_step
       SET status = 'failed', stop_reason = ?1,
           completed_at = COALESCE(completed_at, ?2), updated_at = ?2
       WHERE task_run_id = ?3 AND status IN ('pending', 'running')",
      params![checkpoint_stop_reason(code), now, run_id],
    )
    .map_err(database_error)?;
  let task_changed = connection
    .execute(
      "UPDATE collection_task SET status = 'failed', updated_at = ?1
       WHERE id = ?2 AND status = ?3",
      params![now, task_id, expected_status],
    )
    .map_err(database_error)?;
  if task_changed != 1 {
    return Err(task_error("父任务状态已变化，无法原子停止活动运行"));
  }
  append_task_log(connection, run_id, stage, "error", message)
}

fn checkpoint_stop_reason(error_code: &str) -> &'static str {
  match error_code {
    UNCERTAIN_REQUEST_CODE => "uncertain_request",
    "REQUEST_LIMIT_REACHED" => "request_limit",
    "RECORD_LIMIT_REACHED" => "record_limit",
    "BUDGET_LIMIT_REACHED" => "budget_limit",
    _ => "terminal_error",
  }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
