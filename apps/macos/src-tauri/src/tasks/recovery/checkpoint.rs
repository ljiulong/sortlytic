use std::collections::BTreeMap;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::tikhub::{build_collection_request, parse_collection_page};

use super::{CheckpointState, RecoveryAction, RecoveryLimits, RunStepState};

pub(super) fn has_checkpoint_state_conflict(checkpoints: &[CheckpointState]) -> bool {
  let mut active_per_step = BTreeMap::<&str, usize>::new();
  let mut prepared_steps = 0_usize;
  let mut retry_steps = 0_usize;
  for checkpoint in checkpoints.iter().filter(|checkpoint| {
    matches!(
      checkpoint.status.as_str(),
      "prepared" | "requesting" | "response_received" | "failed" | "uncertain"
    )
  }) {
    let count = active_per_step
      .entry(checkpoint.step_id.as_str())
      .or_default();
    *count += 1;
    if *count > 1 {
      return true;
    }
    if checkpoint.status == "prepared" {
      prepared_steps += 1;
      if checkpoint.request_attempt_count != 0
        || checkpoint.record_count_received != 0
        || checkpoint.record_count_persisted != 0
        || checkpoint.requested_at.is_some()
        || checkpoint.provider_response_json.is_some()
        || checkpoint.provider_response_hash.is_some()
        || checkpoint.provider_response_size.is_some()
        || checkpoint.response_received_at.is_some()
        || checkpoint.committed_at.is_some()
        || checkpoint.has_more.is_some()
        || checkpoint.next_cursor_json.is_some()
        || !is_empty_json_object(&checkpoint.cost_actual_json)
      {
        return true;
      }
    }
    if checkpoint.status == "failed" && checkpoint.retryable {
      retry_steps += 1;
      if !retryable_failed_state_is_consistent(checkpoint) {
        return true;
      }
    }
  }
  prepared_steps + retry_steps + completed_frontier_count(checkpoints) > 1
}

pub(super) fn run_steps_are_compatible(
  run_steps: &[RunStepState],
  checkpoints: &[CheckpointState],
) -> bool {
  let mut checkpoints_by_step = BTreeMap::<&str, Vec<&CheckpointState>>::new();
  for checkpoint in checkpoints {
    checkpoints_by_step
      .entry(checkpoint.step_id.as_str())
      .or_default()
      .push(checkpoint);
  }

  let mut phase = 0_u8;
  for step in run_steps {
    let chain = checkpoints_by_step
      .get(step.id.as_str())
      .map(Vec::as_slice)
      .unwrap_or_default();
    match step.status.as_str() {
      "success" => {
        if phase != 0 || !is_terminal_completed_chain(chain) {
          return false;
        }
      }
      "running" => {
        if phase != 0 {
          return false;
        }
        phase = 1;
      }
      "pending" => {
        if !chain.is_empty() {
          return false;
        }
        phase = 2;
      }
      "failed" | "cancelled" => return true,
      _ => return false,
    }
  }
  true
}

pub(super) fn checkpoint_chains_are_valid(checkpoints: &[CheckpointState]) -> bool {
  let mut by_step = BTreeMap::<&str, Vec<&CheckpointState>>::new();
  for checkpoint in checkpoints {
    by_step
      .entry(checkpoint.step_id.as_str())
      .or_default()
      .push(checkpoint);
  }
  for chain in by_step.values_mut() {
    chain.sort_by_key(|checkpoint| checkpoint.page_index);
    for (index, checkpoint) in chain.iter().enumerate() {
      if checkpoint.page_index != index as i64 {
        return false;
      }
      if index == 0 {
        if checkpoint.input_cursor_json.is_some() {
          return false;
        }
      } else {
        let previous = chain[index - 1];
        if previous.status != "completed"
          || previous.has_more != Some(true)
          || parsed_json(previous.next_cursor_json.as_deref())
            != parsed_json(checkpoint.input_cursor_json.as_deref())
        {
          return false;
        }
      }
      if index + 1 < chain.len()
        && (checkpoint.status != "completed" || checkpoint.has_more != Some(true))
      {
        return false;
      }
    }
  }
  true
}

pub(super) fn checkpoint_has_complete_evidence(checkpoint: &CheckpointState) -> bool {
  if !matches!(
    checkpoint.status.as_str(),
    "response_received" | "completed"
  ) {
    return true;
  }
  let (Some(response), Some(expected_hash), Some(expected_size)) = (
    checkpoint.provider_response_json.as_deref(),
    checkpoint.provider_response_hash.as_deref(),
    checkpoint.provider_response_size,
  ) else {
    return false;
  };
  let (Some(requested_at), Some(response_received_at), Some(has_more)) = (
    valid_timestamp(checkpoint.requested_at.as_deref()),
    valid_timestamp(checkpoint.response_received_at.as_deref()),
    checkpoint.has_more,
  ) else {
    return false;
  };
  let response_size = i64::try_from(response.len()).ok();
  let response_hash = format!("{:x}", Sha256::digest(response.as_bytes()));
  let Some(response_json) = serde_json::from_str::<Value>(response).ok() else {
    return false;
  };
  let Some(params_json) = serde_json::from_str::<Value>(&checkpoint.params_json).ok() else {
    return false;
  };
  let input_cursor = match checkpoint.input_cursor_json.as_deref() {
    Some(cursor) => match serde_json::from_str::<Value>(cursor) {
      Ok(cursor) => Some(cursor),
      Err(_) => return false,
    },
    None => None,
  };
  let Ok(request) = build_collection_request(
    &checkpoint.platform,
    &checkpoint.data_type,
    &params_json,
    input_cursor.as_ref(),
  ) else {
    return false;
  };
  let Ok(page) = parse_collection_page(&request, response_json) else {
    return false;
  };
  let next_cursor = match checkpoint.next_cursor_json.as_deref() {
    Some(cursor) => match serde_json::from_str::<Value>(cursor) {
      Ok(cursor) => Some(cursor),
      Err(_) => return false,
    },
    None => None,
  };
  let response_is_valid = checkpoint.request_attempt_count > 0
    && requested_at <= response_received_at
    && response_size == Some(expected_size)
    && response_hash == expected_hash
    && page.has_more == has_more
    && page.next_cursor == next_cursor
    && i64::try_from(page.records.len()).ok() == Some(checkpoint.record_count_received)
    && checkpoint_cost_micros(checkpoint).is_some();
  if !response_is_valid {
    return false;
  }
  if checkpoint.status == "response_received" {
    return checkpoint.committed_at.is_none();
  }
  valid_timestamp(checkpoint.committed_at.as_deref())
    .is_some_and(|committed_at| response_received_at <= committed_at)
    && checkpoint.record_count_persisted == checkpoint.record_count_received
}

pub(super) fn latest_completed_next_step(checkpoints: &[CheckpointState]) -> Option<&str> {
  let mut latest_by_step = BTreeMap::<&str, &CheckpointState>::new();
  for checkpoint in checkpoints
    .iter()
    .filter(|checkpoint| checkpoint.status == "completed")
  {
    let entry = latest_by_step
      .entry(checkpoint.step_id.as_str())
      .or_insert(checkpoint);
    if checkpoint.page_index > entry.page_index {
      *entry = checkpoint;
    }
  }
  latest_by_step
    .values()
    .find(|checkpoint| checkpoint.has_more == Some(true))
    .map(|checkpoint| checkpoint.step_id.as_str())
}

pub(super) fn retry_limit_stop(
  checkpoints: &[CheckpointState],
  limits: &RecoveryLimits,
  target_step_id: &str,
) -> Option<RecoveryAction> {
  let mut attempts_per_step = BTreeMap::<&str, i64>::new();
  let mut persisted_records = 0_i64;

  for checkpoint in checkpoints {
    let attempts = attempts_per_step
      .entry(checkpoint.step_id.as_str())
      .or_default();
    *attempts = match attempts.checked_add(checkpoint.request_attempt_count) {
      Some(total) => total,
      None => return Some(checkpoint_counter_invalid()),
    };
    persisted_records = match persisted_records.checked_add(checkpoint.record_count_persisted) {
      Some(total) => total,
      None => return Some(checkpoint_counter_invalid()),
    };
  }

  if attempts_per_step.get(target_step_id).copied().unwrap_or(0) >= limits.request_limit {
    return Some(RecoveryAction::Stop {
      stage: "请求上限已到",
      code: "REQUEST_LIMIT_REACHED",
      message: "目标步骤已达到 request_limit，禁止继续发送请求",
    });
  }
  if persisted_records >= limits.record_limit {
    return Some(RecoveryAction::Stop {
      stage: "记录上限已到",
      code: "RECORD_LIMIT_REACHED",
      message: "已持久化记录数达到 record_limit，禁止继续发送请求",
    });
  }

  let mut actual_cost_micros = 0_i64;
  for checkpoint in checkpoints {
    if checkpoint.request_attempt_count == 0 {
      continue;
    }
    let amount = match checkpoint_cost_micros(checkpoint) {
      Some(amount) => amount,
      None => return Some(budget_accounting_incomplete()),
    };
    actual_cost_micros = match actual_cost_micros.checked_add(amount) {
      Some(total) => total,
      None => return Some(budget_accounting_incomplete()),
    };
  }
  if actual_cost_micros >= limits.budget_micros {
    return Some(RecoveryAction::Stop {
      stage: "预算上限已到",
      code: "BUDGET_LIMIT_REACHED",
      message: "已记录成本达到 budget_limit，禁止继续发送请求",
    });
  }
  None
}

fn completed_frontier_count(checkpoints: &[CheckpointState]) -> usize {
  let mut latest_by_step = BTreeMap::<&str, &CheckpointState>::new();
  for checkpoint in checkpoints {
    let entry = latest_by_step
      .entry(checkpoint.step_id.as_str())
      .or_insert(checkpoint);
    if checkpoint.page_index > entry.page_index {
      *entry = checkpoint;
    }
  }
  latest_by_step
    .values()
    .filter(|checkpoint| checkpoint.status == "completed" && checkpoint.has_more == Some(true))
    .count()
}

fn retryable_failed_state_is_consistent(checkpoint: &CheckpointState) -> bool {
  let has_remote_result = checkpoint.record_count_received != 0
    || checkpoint.record_count_persisted != 0
    || checkpoint.provider_response_json.is_some()
    || checkpoint.provider_response_hash.is_some()
    || checkpoint.provider_response_size.is_some()
    || checkpoint.response_received_at.is_some()
    || checkpoint.committed_at.is_some()
    || checkpoint.has_more.is_some()
    || checkpoint.next_cursor_json.is_some();
  if has_remote_result {
    return false;
  }
  if checkpoint.request_attempt_count == 0 {
    return checkpoint.requested_at.is_none() && is_empty_json_object(&checkpoint.cost_actual_json);
  }
  valid_timestamp(checkpoint.requested_at.as_deref()).is_some()
}

fn is_terminal_completed_chain(chain: &[&CheckpointState]) -> bool {
  !chain.is_empty()
    && chain
      .iter()
      .all(|checkpoint| checkpoint.status == "completed")
    && chain
      .last()
      .is_some_and(|checkpoint| checkpoint.has_more == Some(false))
}

fn parsed_json(value: Option<&str>) -> Option<Value> {
  value.and_then(|value| serde_json::from_str(value).ok())
}

fn valid_timestamp(value: Option<&str>) -> Option<chrono::DateTime<chrono::FixedOffset>> {
  chrono::DateTime::parse_from_rfc3339(value?.trim()).ok()
}

fn is_empty_json_object(value: &str) -> bool {
  serde_json::from_str::<Value>(value).ok() == Some(serde_json::json!({}))
}

fn checkpoint_cost_micros(checkpoint: &CheckpointState) -> Option<i64> {
  let cost = serde_json::from_str::<Value>(&checkpoint.cost_actual_json).ok()?;
  if cost.get("currency").and_then(Value::as_str) != Some("USD") {
    return None;
  }
  cost
    .get("amount_micros")
    .and_then(Value::as_i64)
    .filter(|amount| *amount >= 0)
}

fn budget_accounting_incomplete() -> RecoveryAction {
  RecoveryAction::Stop {
    stage: "成本证据不完整",
    code: "BUDGET_ACCOUNTING_INCOMPLETE",
    message: "已发送请求缺少可信的 USD 微美元成本，禁止在未知预算占用下自动重试",
  }
}

fn checkpoint_counter_invalid() -> RecoveryAction {
  RecoveryAction::Stop {
    stage: "检查点计数异常",
    code: "CHECKPOINT_COUNTER_INVALID",
    message: "检查点请求数或记录数发生整数溢出，禁止自动恢复",
  }
}
