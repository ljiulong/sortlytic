use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tasks::CostEstimateView;

mod capabilities;
mod form_plan;

use capabilities::{endpoint_for, find_endpoint, normalize_platform};

pub use form_plan::generate_form_collection_plan;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformCapabilityView {
  pub platform: String,
  pub display_name: String,
  pub data_types: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PaginationMode {
  Single,
  Cursor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FilterExecution {
  Provider,
  Local,
  Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataTypeCapabilityView {
  pub platform: String,
  pub data_type: String,
  pub display_name: String,
  pub endpoint_key: String,
  pub required_params: Vec<String>,
  pub optional_params: Vec<String>,
  pub pagination_mode: PaginationMode,
  pub region_filter: FilterExecution,
  pub time_range_filter: FilterExecution,
  pub max_page_size: i64,
  pub max_request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormCollectionPlanRequest {
  pub platform: String,
  pub data_type: String,
  pub params: Value,
  pub request_limit: Option<i64>,
  pub record_limit: Option<i64>,
  pub budget_limit_micros: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionParamValidationResult {
  pub valid: bool,
  pub errors: Vec<String>,
  pub missing_fields: Vec<String>,
  pub normalized_params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionPlanValidationResult {
  pub valid: bool,
  pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionPlanDraftView {
  pub source: String,
  pub schema_version: i64,
  pub plan_json: Value,
  pub validation_status: String,
  pub validation_errors_json: Value,
  pub cost_estimate_json: Value,
}

pub fn list_supported_platforms() -> Vec<PlatformCapabilityView> {
  capabilities::list_supported_platforms()
}

pub fn list_platform_data_types(platform: &str) -> AppResult<Vec<DataTypeCapabilityView>> {
  capabilities::list_platform_data_types(platform)
}

pub fn validate_collection_params(
  platform: &str,
  data_type: &str,
  params: Value,
) -> AppResult<CollectionParamValidationResult> {
  let endpoint = endpoint_for(platform, data_type)?;
  let mut errors = Vec::new();
  let mut missing_fields = Vec::new();
  let normalized_params = normalize_params(params);

  for required_param in endpoint.required_params {
    if normalized_params
      .get(required_param)
      .and_then(Value::as_str)
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .is_none()
    {
      missing_fields.push((*required_param).to_string());
    }
  }

  if let Some(value) = normalized_params.get("page_size") {
    match value.as_i64() {
      Some(page_size) if page_size > 0 && page_size <= endpoint.max_page_size => {}
      _ => {
        errors.push(format!(
          "page_size 必须是大于 0 且不超过 {} 的整数",
          endpoint.max_page_size
        ));
      }
    }
  }

  let allowed_params = endpoint
    .required_params
    .iter()
    .chain(endpoint.optional_params.iter())
    .copied()
    .collect::<Vec<_>>();
  if let Some(object) = normalized_params.as_object() {
    for key in object.keys() {
      let legacy_readable_region = key == "region"
        && matches!(endpoint.data_type, "account_profile" | "item_detail")
        && endpoint.region_filter == FilterExecution::Unsupported;
      if !allowed_params.contains(&key.as_str()) && !legacy_readable_region {
        errors.push(format!("参数 {key} 不在 endpoint 白名单内"));
      }
    }
  }

  Ok(CollectionParamValidationResult {
    valid: errors.is_empty() && missing_fields.is_empty(),
    errors,
    missing_fields,
    normalized_params,
  })
}

pub fn validate_collection_plan(plan_json: &Value) -> CollectionPlanValidationResult {
  let mut errors = Vec::new();
  let platforms = plan_string_array(plan_json, "platforms", &mut errors);
  let data_types = plan_string_array(plan_json, "data_types", &mut errors);

  for platform in &platforms {
    if normalize_platform(platform).is_err() {
      errors.push(format!("平台 {platform} 不受支持"));
    }
  }

  let requires_time_range = data_types
    .iter()
    .any(|data_type| matches!(data_type.as_str(), "keyword_search" | "comments"));
  let requires_region = platforms.iter().any(|platform| {
    data_types.iter().any(|data_type| {
      find_endpoint(platform, data_type)
        .is_some_and(|endpoint| endpoint.region_filter != FilterExecution::Unsupported)
    })
  });
  let top_level_region = if requires_region {
    normalized_plan_region(plan_json.get("region"), &mut errors)
  } else {
    None
  };
  if requires_time_range
    && plan_json
      .get("time_range")
      .and_then(Value::as_str)
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .is_none()
  {
    errors.push("time_range 不能为空".to_string());
  }

  let request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .filter(|value| *value > 0);
  if request_limit.is_none() {
    errors.push("request_limit 必须是大于 0 的整数".to_string());
  }

  if plan_json
    .get("requires_user_confirmation")
    .and_then(Value::as_bool)
    != Some(true)
  {
    errors.push("requires_user_confirmation 必须为 true".to_string());
  }

  if let Some(missing_fields) = plan_json.get("missing_fields").and_then(Value::as_array) {
    for field in missing_fields.iter().filter_map(Value::as_str) {
      errors.push(format!("计划缺少字段 {field}"));
    }
  } else {
    errors.push("missing_fields 必须是数组".to_string());
  }

  let mut covered_pairs = BTreeSet::new();
  let Some(steps) = plan_json.get("steps").and_then(Value::as_array) else {
    errors.push("steps 必须是非空数组".to_string());
    return CollectionPlanValidationResult {
      valid: false,
      errors,
    };
  };
  if steps.is_empty() {
    errors.push("steps 必须是非空数组".to_string());
  }

  for (index, step) in steps.iter().enumerate() {
    let prefix = format!("steps[{index}]");
    let Some(step_object) = step.as_object() else {
      errors.push(format!("{prefix} 必须是对象"));
      continue;
    };
    let platform = required_object_string(step_object, "platform", &prefix, &mut errors);
    let data_type = required_object_string(step_object, "data_type", &prefix, &mut errors);
    let endpoint_key = required_object_string(step_object, "endpoint_key", &prefix, &mut errors);
    let Some((platform, data_type, endpoint_key)) = platform
      .as_deref()
      .zip(data_type.as_deref())
      .zip(endpoint_key.as_deref())
      .map(|((platform, data_type), endpoint_key)| (platform, data_type, endpoint_key))
    else {
      continue;
    };

    if !platforms.iter().any(|value| value == platform) {
      errors.push(format!("{prefix}.platform 未包含在顶层 platforms 中"));
    }
    if !data_types.iter().any(|value| value == data_type) {
      errors.push(format!("{prefix}.data_type 未包含在顶层 data_types 中"));
    }

    let Some(endpoint) = find_endpoint(platform, data_type) else {
      errors.push(format!("{prefix} 的平台或数据类型组合不受支持"));
      continue;
    };
    if endpoint.endpoint_key != endpoint_key {
      errors.push(format!(
        "{prefix}.endpoint_key 应为 {}",
        endpoint.endpoint_key
      ));
    }
    if request_limit.is_some_and(|limit| limit > endpoint.max_request_count) {
      errors.push(format!(
        "request_limit 不能超过 {} 的上限 {}",
        endpoint.endpoint_key, endpoint.max_request_count
      ));
    }

    let params = step_object
      .get("params")
      .cloned()
      .unwrap_or_else(|| serde_json::json!({}));
    match validate_collection_params(platform, data_type, params.clone()) {
      Ok(validation) => {
        for field in validation.missing_fields {
          errors.push(format!("{prefix}.params 缺少 {field}"));
        }
        for error in validation.errors {
          errors.push(format!("{prefix}.params：{error}"));
        }
      }
      Err(error) => errors.push(format!("{prefix}：{}", error.message)),
    }
    if endpoint.region_filter != FilterExecution::Unsupported {
      let params_region = params
        .get("region")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
      if params_region.is_none() {
        errors.push(format!("{prefix}.params 缺少 region"));
      } else if top_level_region.as_deref() != params_region {
        errors.push(format!("{prefix}.params.region 与顶层 region 不一致"));
      }
    }
    if matches!(data_type, "keyword_search" | "comments")
      && params
        .get("time_range")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
      errors.push(format!("{prefix}.params 缺少 time_range"));
    }

    covered_pairs.insert((platform.to_string(), data_type.to_string()));
  }

  for platform in &platforms {
    for data_type in &data_types {
      if find_endpoint(platform, data_type).is_some()
        && !covered_pairs.contains(&(platform.clone(), data_type.clone()))
      {
        errors.push(format!("steps 未覆盖 {platform}.{data_type}"));
      }
    }
  }

  errors.sort();
  errors.dedup();
  CollectionPlanValidationResult {
    valid: errors.is_empty(),
    errors,
  }
}

pub fn validate_collection_plan_v2(plan_json: &Value) -> CollectionPlanValidationResult {
  let mut errors = validate_collection_plan(plan_json).errors;
  let record_limit = positive_integer_field(plan_json, "record_limit");
  if record_limit.is_none() {
    errors.push("record_limit 必须是大于 0 的整数".to_string());
  }
  let request_limit = positive_integer_field(plan_json, "request_limit");
  if request_limit.is_none() {
    errors.push("request_limit 必须是大于 0 的整数".to_string());
  }

  match plan_json.get("budget_limit").and_then(Value::as_object) {
    Some(budget_limit) => {
      if budget_limit.get("currency").and_then(Value::as_str) != Some("USD") {
        errors.push("budget_limit.currency 只能为 USD".to_string());
      }
      if budget_limit
        .get("amount_micros")
        .and_then(Value::as_i64)
        .filter(|amount| *amount > 0)
        .is_none()
      {
        errors.push("budget_limit.amount_micros 必须是大于 0 的整数微美元".to_string());
      }
    }
    None => errors.push("budget_limit 必须是对象".to_string()),
  }

  let top_level_region = filter_constraint(plan_json.get("region"));
  let top_level_time_range = filter_constraint(plan_json.get("time_range"));
  if let Some(steps) = plan_json.get("steps").and_then(Value::as_array) {
    for (index, step) in steps.iter().enumerate() {
      let prefix = format!("steps[{index}]");
      let Some(step_object) = step.as_object() else {
        continue;
      };
      let Some(platform) = step_object.get("platform").and_then(Value::as_str) else {
        continue;
      };
      let Some(data_type) = step_object.get("data_type").and_then(Value::as_str) else {
        continue;
      };
      let Some(endpoint) = find_endpoint(platform.trim(), data_type.trim()) else {
        continue;
      };

      if endpoint.pagination_mode == PaginationMode::Single && request_limit != Some(1) {
        errors.push(format!(
          "{prefix} 的 pagination_mode=single，request_limit 必须为 1"
        ));
      }

      let params = step_object.get("params").and_then(Value::as_object);
      let step_region = params
        .and_then(|params| params.get("region"))
        .and_then(|value| filter_constraint(Some(value)));
      validate_filter_constraint(
        &prefix,
        "region",
        endpoint.region_filter,
        step_region.or(top_level_region),
        &[],
        &mut errors,
      );

      let step_time_range = params
        .and_then(|params| params.get("time_range"))
        .and_then(|value| filter_constraint(Some(value)));
      validate_filter_constraint(
        &prefix,
        "time_range",
        endpoint.time_range_filter,
        step_time_range.or(top_level_time_range),
        endpoint.provider_time_ranges,
        &mut errors,
      );

      if endpoint.time_range_filter == FilterExecution::Provider {
        validate_filter_values_match(
          &prefix,
          "time_range",
          top_level_time_range,
          step_time_range,
          &mut errors,
        );
      }
    }
  }

  errors.sort();
  errors.dedup();
  CollectionPlanValidationResult {
    valid: errors.is_empty(),
    errors,
  }
}

fn positive_integer_field(plan_json: &Value, field: &str) -> Option<i64> {
  plan_json
    .get(field)
    .and_then(Value::as_i64)
    .filter(|value| *value > 0)
}

fn filter_constraint(value: Option<&Value>) -> Option<&Value> {
  value.filter(|value| match value {
    Value::Null => false,
    Value::String(text) => !text.trim().is_empty(),
    _ => true,
  })
}

fn validate_filter_constraint(
  prefix: &str,
  field: &str,
  execution: FilterExecution,
  value: Option<&Value>,
  provider_time_ranges: &[&str],
  errors: &mut Vec<String>,
) {
  let Some(value) = value else {
    return;
  };

  match execution {
    FilterExecution::Provider => {
      let Some(text) = value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
      else {
        errors.push(format!("{prefix}.{field} 必须是非空字符串"));
        return;
      };
      if field == "time_range"
        && normalized_relative_days(text)
          .filter(|days| provider_time_ranges.contains(&days.as_str()))
          .is_none()
      {
        errors.push(format!(
          "{prefix}.time_range 仅支持 {} 天（也可使用近N天表示）",
          provider_time_ranges.join("/")
        ));
      }
    }
    FilterExecution::Local => errors.push(format!(
      "{prefix}.{field} 需要本地过滤，但本地过滤器尚未接通"
    )),
    FilterExecution::Unsupported => {
      errors.push(format!("{prefix} 的数据类型不支持 {field} 筛选"));
    }
  }
}

fn validate_filter_values_match(
  prefix: &str,
  field: &str,
  top_level: Option<&Value>,
  step_value: Option<&Value>,
  errors: &mut Vec<String>,
) {
  let Some(top_level) = top_level.and_then(compact_filter_text) else {
    return;
  };
  let Some(step_value) = step_value.and_then(compact_filter_text) else {
    return;
  };
  if top_level != step_value {
    errors.push(format!("{prefix}.params.{field} 与顶层 {field} 不一致"));
  }
}

fn compact_filter_text(value: &Value) -> Option<String> {
  value.as_str().and_then(normalized_relative_days)
}

fn normalized_relative_days(value: &str) -> Option<String> {
  let compact = value
    .chars()
    .filter(|character| !character.is_whitespace())
    .collect::<String>();
  let days = compact
    .strip_prefix('近')
    .and_then(|value| value.strip_suffix('天'))
    .unwrap_or(&compact);
  (!days.is_empty()).then(|| days.to_string())
}

pub fn preview_collection_plan(plan_json: Value) -> AppResult<CostEstimateView> {
  let platform_count = plan_json
    .get("platforms")
    .and_then(Value::as_array)
    .map_or(0, |items| items.len() as i64);
  let data_type_count = plan_json
    .get("data_types")
    .and_then(Value::as_array)
    .map_or(0, |items| items.len() as i64);
  let request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .unwrap_or(1)
    .max(1);

  Ok(estimate_plan_cost(
    platform_count,
    data_type_count,
    request_limit,
  ))
}

fn normalize_params(params: Value) -> Value {
  let Some(object) = params.as_object() else {
    return serde_json::json!({});
  };

  let mut normalized = serde_json::Map::new();
  for (key, value) in object {
    let normalized_value = match value {
      Value::String(text) => Value::String(text.trim().to_string()),
      _ => value.clone(),
    };
    normalized.insert(key.trim().to_string(), normalized_value);
  }

  Value::Object(normalized)
}

fn plan_string_array(plan_json: &Value, field: &str, errors: &mut Vec<String>) -> Vec<String> {
  let Some(values) = plan_json.get(field).and_then(Value::as_array) else {
    errors.push(format!("{field} 必须是非空字符串数组"));
    return Vec::new();
  };
  let mut normalized = Vec::new();
  for value in values {
    if let Some(value) = value
      .as_str()
      .map(str::trim)
      .filter(|value| !value.is_empty())
    {
      if !normalized.iter().any(|existing| existing == value) {
        normalized.push(value.to_string());
      }
    } else {
      errors.push(format!("{field} 只能包含非空字符串"));
    }
  }
  if normalized.is_empty() {
    errors.push(format!("{field} 不能为空"));
  }
  normalized
}

fn normalized_plan_region(value: Option<&Value>, errors: &mut Vec<String>) -> Option<String> {
  match value {
    Some(Value::String(region)) if !region.trim().is_empty() => Some(region.trim().to_string()),
    Some(Value::Object(region)) => {
      let value = region
        .get("value")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
      if region.get("validation_status").and_then(Value::as_str) == Some("unverified") {
        errors.push("region 尚未验证".to_string());
      }
      if value.is_none() {
        errors.push("region.value 不能为空".to_string());
      }
      value.map(ToString::to_string)
    }
    _ => {
      errors.push("region 不能为空".to_string());
      None
    }
  }
}

fn required_object_string(
  object: &serde_json::Map<String, Value>,
  field: &str,
  prefix: &str,
  errors: &mut Vec<String>,
) -> Option<String> {
  let value = object
    .get(field)
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty());
  if value.is_none() {
    errors.push(format!("{prefix}.{field} 不能为空"));
  }
  value.map(ToString::to_string)
}

fn value_to_array(value: Option<&Value>) -> Value {
  match value {
    Some(Value::String(text)) if !text.trim().is_empty() => serde_json::json!([text.trim()]),
    Some(Value::Array(items)) => Value::Array(items.clone()),
    _ => serde_json::json!([]),
  }
}

fn estimate_plan_cost(
  platform_count: i64,
  data_type_count: i64,
  request_limit: i64,
) -> CostEstimateView {
  let platform_count = platform_count.max(1);
  let data_type_count = data_type_count.max(1);
  let request_count_estimate = platform_count * data_type_count * request_limit.max(1);
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

fn collection_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::Collection,
    false,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn lists_only_mvp_platforms() {
    let platforms = list_supported_platforms();

    assert_eq!(platforms.len(), 3);
    assert!(platforms
      .iter()
      .any(|platform| platform.platform == "tiktok"));
    assert!(platforms
      .iter()
      .any(|platform| platform.platform == "douyin"));
    assert!(platforms
      .iter()
      .any(|platform| platform.platform == "xiaohongshu"));
  }

  #[test]
  fn validates_params_against_whitelist() {
    let result = validate_collection_params(
      "tiktok",
      "keyword_search",
      serde_json::json!({ "keyword": " car ", "unexpected": true }),
    )
    .expect("validation should run");

    assert!(!result.valid);
    assert!(result.errors[0].contains("unexpected"));
    assert_eq!(result.normalized_params["keyword"], "car");
  }

  #[test]
  fn form_plan_contains_endpoint_and_confirmation_gate() {
    let plan = generate_form_collection_plan(FormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      data_type: "keyword_search".to_string(),
      params: serde_json::json!({
        "keyword": "car",
        "region": "US",
        "time_range": "30",
        "page_size": 50
      }),
      request_limit: Some(2),
      record_limit: None,
      budget_limit_micros: None,
    })
    .expect("plan should generate");

    assert_eq!(
      plan.validation_status, "valid",
      "{:?}",
      plan.validation_errors_json
    );
    assert_eq!(plan.schema_version, 2);
    assert!(plan.plan_json["record_limit"]
      .as_i64()
      .is_some_and(|value| value > 0));
    assert_eq!(plan.plan_json["budget_limit"]["currency"], "USD");
    assert_eq!(
      plan.plan_json["steps"][0]["endpoint_key"],
      "tiktok.keyword_search"
    );
    assert_eq!(plan.plan_json["requires_user_confirmation"], true);
    assert_eq!(plan.cost_estimate_json["request_count_estimate"], 2);
  }

  #[test]
  fn account_profile_and_item_detail_are_registered_for_all_platforms() {
    for platform in ["tiktok", "douyin", "xiaohongshu"] {
      let data_types = list_platform_data_types(platform).expect("platform should be supported");

      assert!(data_types
        .iter()
        .any(|item| item.data_type == "account_profile"));
      assert!(data_types
        .iter()
        .any(|item| item.data_type == "item_detail"));
    }
  }

  #[test]
  fn comments_accept_time_range_from_form_builder() {
    let result = validate_collection_params(
      "xiaohongshu",
      "comments",
      serde_json::json!({ "item_id": "note-1", "region": "CN", "time_range": "近 30 天" }),
    )
    .expect("validation should run");

    assert!(result.valid);
  }
}

#[cfg(test)]
#[path = "collection/plan_validation_tests.rs"]
mod plan_validation_tests;
