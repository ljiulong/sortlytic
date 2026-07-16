use serde_json::Value;

use super::{json_to_string_vec, CollectionTaskView, CostEstimateView};

pub(super) fn estimate_from_plan_json(plan_json: &Value) -> CostEstimateView {
  let platform_count = plan_json
    .get("platforms")
    .and_then(Value::as_array)
    .map_or(0, |items| items.len() as i64);
  let data_type_count = plan_json
    .get("data_types")
    .and_then(Value::as_array)
    .map_or(0, |items| items.len() as i64);
  let step_count = plan_json
    .get("steps")
    .and_then(Value::as_array)
    .map_or(1, |items| items.len().max(1) as i64);
  let request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .unwrap_or(1)
    .max(1);
  let request_count_estimate = step_count.saturating_mul(request_limit);
  let requires_confirmation =
    request_count_estimate > 1 || platform_count > 1 || data_type_count > 1;

  CostEstimateView {
    request_count_estimate,
    platform_count,
    data_type_count,
    requires_confirmation,
    cost_estimate_json: serde_json::json!({
      "request_count_estimate": request_count_estimate,
      "requires_confirmation": requires_confirmation
    }),
  }
}

pub(super) fn validate_plan_for_task(task: &CollectionTaskView, plan_json: &Value) -> Vec<String> {
  let mut errors = Vec::new();
  let mut task_platforms = json_to_string_vec(task.platforms_json.clone());
  let mut task_data_types = json_to_string_vec(task.data_types_json.clone());
  let mut plan_platforms = plan_json
    .get("platforms")
    .cloned()
    .map(json_to_string_vec)
    .unwrap_or_default();
  let mut plan_data_types = plan_json
    .get("data_types")
    .cloned()
    .map(json_to_string_vec)
    .unwrap_or_default();

  task_platforms.sort();
  task_platforms.dedup();
  task_data_types.sort();
  task_data_types.dedup();
  plan_platforms.sort();
  plan_platforms.dedup();
  plan_data_types.sort();
  plan_data_types.dedup();

  if task_platforms != plan_platforms {
    errors.push("计划 platforms 与任务范围不一致".to_string());
  }
  if task_data_types != plan_data_types {
    errors.push("计划 data_types 与任务范围不一致".to_string());
  }
  errors.sort();
  errors.dedup();
  errors
}
