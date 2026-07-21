use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tasks::CostEstimateView;

mod account_capabilities;
mod account_input;
mod account_plan;
mod account_plan_filters;
#[cfg(test)]
#[path = "collection/account_plan_tests.rs"]
mod account_plan_tests;
mod capabilities;
mod form_plan;
pub(crate) mod plan_estimate;
mod plan_validation;

use capabilities::{endpoint_for, find_endpoint};

pub(crate) use account_capabilities::{account_field_keys, account_source_keys};
pub use account_capabilities::{
  AccountCollectionCapabilityView, AccountFieldAvailability, AccountFieldCapabilityView,
  AccountFieldGroupView, AccountFieldValueType, AccountSourceCapabilityView,
  AccountSourceInputKind, DataTypeCapabilityView, FilterExecution, PaginationMode,
  PlatformCapabilityView,
};
pub use account_plan::{
  generate_account_collection_plan, validate_collection_plan_v4, AccountFormCollectionPlanRequest,
};
pub use form_plan::generate_form_collection_plan;
pub use plan_validation::validate_collection_plan;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormCollectionPlanRequest {
  pub platform: String,
  #[serde(default)]
  pub data_type: Option<String>,
  #[serde(default)]
  pub data_types: Vec<String>,
  pub params: Value,
  pub age_range: Option<AgeRangeInput>,
  pub request_limit: Option<i64>,
  pub record_limit: Option<i64>,
  pub budget_limit_micros: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgeRangeInput {
  pub min: i64,
  pub max: i64,
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

pub fn get_account_collection_capabilities(
  platform: &str,
) -> AppResult<AccountCollectionCapabilityView> {
  account_capabilities::get_account_collection_capabilities(platform)
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

pub fn validate_collection_plan_v3(plan_json: &Value) -> CollectionPlanValidationResult {
  let mut executable_plan = plan_json.clone();
  let mut executable_data_types = executable_plan
    .get("data_types")
    .and_then(Value::as_array)
    .cloned()
    .unwrap_or_default();
  if let Some(internal_data_types) = executable_plan
    .get("internal_data_types")
    .and_then(Value::as_array)
  {
    for data_type in internal_data_types {
      if !executable_data_types.contains(data_type) {
        executable_data_types.push(data_type.clone());
      }
    }
  }
  executable_plan["data_types"] = Value::Array(executable_data_types);
  executable_plan["request_limit"] = Value::from(1);
  let mut errors = validate_collection_plan(&executable_plan).errors;
  if plan_json.get("schema_version").and_then(Value::as_i64) != Some(3) {
    errors.push("schema_version 必须为 3".to_string());
  }
  if positive_integer_field(plan_json, "record_limit").is_none() {
    errors.push("record_limit 必须是大于 0 的整数".to_string());
  }
  if positive_integer_field(plan_json, "request_limit").is_none() {
    errors.push("request_limit 必须是大于 0 的整数".to_string());
  }
  validate_budget_limit(plan_json, &mut errors);
  validate_age_range(plan_json.get("age_range"), &mut errors);
  validate_gender_filter(plan_json.get("gender_filter"), &mut errors);

  let top_level_time_range = filter_constraint(plan_json.get("time_range"));
  let mut prior_steps = std::collections::BTreeSet::new();
  if let Some(steps) = plan_json.get("steps").and_then(Value::as_array) {
    for (index, step) in steps.iter().enumerate() {
      let prefix = format!("steps[{index}]");
      let Some(step) = step.as_object() else {
        continue;
      };
      let step_key = step
        .get("step_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
      match step_key {
        Some(step_key) if prior_steps.insert(step_key.to_string()) => {}
        Some(_) => errors.push(format!("{prefix}.step_key 不能重复")),
        None => errors.push(format!("{prefix}.step_key 不能为空")),
      }
      if let Some(dependency) = step
        .get("depends_on_step_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
      {
        if !prior_steps.contains(dependency) {
          errors.push(format!("{prefix}.depends_on_step_key 必须引用前置步骤"));
        }
      }
      let request_limit = step
        .get("request_limit")
        .and_then(Value::as_i64)
        .filter(|value| *value > 0);
      let endpoint = step
        .get("platform")
        .and_then(Value::as_str)
        .zip(step.get("data_type").and_then(Value::as_str))
        .and_then(|(platform, data_type)| find_endpoint(platform, data_type));
      if request_limit.is_none() {
        errors.push(format!("{prefix}.request_limit 必须是大于 0 的整数"));
      } else if endpoint.is_some_and(|endpoint| request_limit > Some(endpoint.max_request_count)) {
        errors.push(format!("{prefix}.request_limit 超过端点上限"));
      }

      if let Some(endpoint) =
        endpoint.filter(|endpoint| endpoint.time_range_filter == FilterExecution::Provider)
      {
        let step_time_range = step
          .get("params")
          .and_then(Value::as_object)
          .and_then(|params| params.get("time_range"))
          .and_then(|value| filter_constraint(Some(value)));
        validate_filter_constraint(
          &prefix,
          "time_range",
          FilterExecution::Provider,
          step_time_range.or(top_level_time_range),
          endpoint.provider_time_ranges,
          &mut errors,
        );
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

fn validate_budget_limit(plan_json: &Value, errors: &mut Vec<String>) {
  match plan_json.get("budget_limit").and_then(Value::as_object) {
    Some(budget_limit)
      if budget_limit.get("currency").and_then(Value::as_str) == Some("USD")
        && budget_limit
          .get("amount_micros")
          .and_then(Value::as_i64)
          .is_some_and(|amount| amount > 0) => {}
    _ => errors.push("budget_limit 必须包含 USD 正整数微美元上限".to_string()),
  }
}

fn validate_age_range(age_range: Option<&Value>, errors: &mut Vec<String>) {
  let Some(age_range) = age_range.filter(|value| !value.is_null()) else {
    return;
  };
  let bounds = age_range
    .get("min")
    .and_then(Value::as_i64)
    .zip(age_range.get("max").and_then(Value::as_i64));
  if !bounds.is_some_and(|(min, max)| (0..=130).contains(&min) && min <= max && max <= 130) {
    errors.push("age_range 必须是 0–130 内且 min <= max 的整数闭区间".to_string());
  }
}

fn validate_gender_filter(gender_filter: Option<&Value>, errors: &mut Vec<String>) {
  let Some(gender_filter) = gender_filter.filter(|value| !value.is_null()) else {
    return;
  };
  let Some(values) = gender_filter.as_array() else {
    errors.push("gender_filter 必须是性别规范值数组".to_string());
    return;
  };
  let mut seen = std::collections::BTreeSet::new();
  if values.is_empty()
    || values.iter().any(|value| {
      value
        .as_str()
        .is_none_or(|value| !matches!(value, "male" | "female" | "other") || !seen.insert(value))
    })
  {
    errors.push("gender_filter 只能包含不重复的 male、female、other".to_string());
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
      data_type: Some("keyword_search".to_string()),
      data_types: Vec::new(),
      params: serde_json::json!({
        "keyword": "car",
        "region": "US",
        "time_range": "30",
        "page_size": 50
      }),
      age_range: None,
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
    assert_eq!(plan.schema_version, 3);
    assert_eq!(
      plan.plan_json["data_types"],
      serde_json::json!(["keyword_search"])
    );
    assert_eq!(plan.plan_json["steps"][0]["step_key"], "keyword_search");
    assert_eq!(plan.plan_json["steps"][0]["output_selected"], true);
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
  fn form_plan_prices_every_materialized_dependency_target() {
    let plan = generate_form_collection_plan(FormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      data_type: Some("item_detail".to_string()),
      data_types: Vec::new(),
      params: serde_json::json!({
        "keyword": "car",
        "region": "US",
        "time_range": "30"
      }),
      age_range: None,
      request_limit: Some(2),
      record_limit: Some(2),
      budget_limit_micros: None,
    })
    .expect("依赖型计划应生成");

    assert_eq!(plan.plan_json["steps"][0]["step_key"], "keyword_search");
    assert_eq!(plan.plan_json["steps"][1]["step_key"], "item_detail");
    assert_eq!(
      plan.cost_estimate_json["request_count_estimate"], 102,
      "两页搜索最多产生 100 个目标，下游详情必须逐个计价"
    );
  }

  #[test]
  fn form_plan_normalizes_multi_targets_dependencies_and_age_range() {
    let plan = generate_form_collection_plan(FormCollectionPlanRequest {
      platform: "xiaohongshu".to_string(),
      data_type: None,
      data_types: vec!["item_detail".to_string(), "comments".to_string()],
      params: serde_json::json!({
        "keyword": "新能源汽车",
        "time_range": "近 180 天",
        "genders": ["female", "other"]
      }),
      age_range: Some(AgeRangeInput { min: 18, max: 35 }),
      request_limit: Some(4),
      record_limit: Some(1200),
      budget_limit_micros: Some(35_000_000),
    })
    .expect("多目标计划应自动生成搜索依赖链");

    assert_eq!(
      plan.validation_status, "valid",
      "{:?}",
      plan.validation_errors_json
    );
    assert_eq!(
      plan.plan_json["age_range"],
      serde_json::json!({ "min": 18, "max": 35 })
    );
    assert_eq!(
      plan.plan_json["gender_filter"],
      serde_json::json!(["female", "other"])
    );
    assert_eq!(
      plan.plan_json["internal_data_types"],
      serde_json::json!(["keyword_search"])
    );
    assert_eq!(
      plan.plan_json["steps"][1]["depends_on_step_key"],
      "keyword_search"
    );
    assert_eq!(plan.plan_json["steps"][0]["output_selected"], false);
  }

  #[test]
  fn form_plan_keeps_dependencies_when_search_is_selected_for_output() {
    let plan = generate_form_collection_plan(FormCollectionPlanRequest {
      platform: "xiaohongshu".to_string(),
      data_type: None,
      data_types: vec![
        "keyword_search".to_string(),
        "item_detail".to_string(),
        "account_profile".to_string(),
        "comments".to_string(),
      ],
      params: serde_json::json!({
        "keyword": "新能源汽车",
        "region": "CN"
      }),
      age_range: None,
      request_limit: Some(4),
      record_limit: Some(1200),
      budget_limit_micros: Some(35_000_000),
    })
    .expect("显式选择搜索结果时仍应生成下游依赖链");

    assert!(plan.plan_json["steps"][0]["depends_on_step_key"].is_null());
    for step in plan.plan_json["steps"]
      .as_array()
      .expect("steps 应为数组")
      .iter()
      .skip(1)
    {
      assert_eq!(step["depends_on_step_key"], "keyword_search");
      assert!(!step["input_binding"].is_null());
    }
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
