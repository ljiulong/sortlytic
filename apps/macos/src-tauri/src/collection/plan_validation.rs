use std::collections::BTreeSet;

use serde_json::Value;

use super::capabilities::{find_endpoint, normalize_platform};
use super::{validate_collection_params, CollectionPlanValidationResult, FilterExecution};

pub fn validate_collection_plan(plan_json: &Value) -> CollectionPlanValidationResult {
  let mut errors = Vec::new();
  let platforms = plan_string_array(plan_json, "platforms", &mut errors);
  let data_types = plan_string_array(plan_json, "data_types", &mut errors);

  for platform in &platforms {
    if normalize_platform(platform).is_err() {
      errors.push(format!("平台 {platform} 不受支持"));
    }
  }

  let requires_time_range = platforms.iter().any(|platform| {
    data_types.iter().any(|data_type| {
      find_endpoint(platform, data_type)
        .is_some_and(|endpoint| endpoint.time_range_filter == FilterExecution::Provider)
    })
  });
  let requires_region = platforms.iter().any(|platform| {
    data_types.iter().any(|data_type| {
      find_endpoint(platform, data_type)
        .is_some_and(|endpoint| endpoint.region_filter == FilterExecution::Provider)
    })
  });
  let top_level_region = if requires_region {
    normalized_plan_region(plan_json.get("region"), &mut errors)
  } else {
    if plan_json
      .get("region")
      .and_then(Value::as_object)
      .and_then(|region| region.get("validation_status"))
      .and_then(Value::as_str)
      == Some("unverified")
    {
      errors.push("region 尚未验证".to_string());
    }
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
    if endpoint.region_filter == FilterExecution::Provider {
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
    if endpoint.time_range_filter == FilterExecution::Provider
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
