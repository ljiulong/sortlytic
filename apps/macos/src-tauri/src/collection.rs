use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tasks::CostEstimateView;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlatformCapabilityView {
  pub platform: String,
  pub display_name: String,
  pub data_types: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataTypeCapabilityView {
  pub platform: String,
  pub data_type: String,
  pub display_name: String,
  pub endpoint_key: String,
  pub required_params: Vec<String>,
  pub optional_params: Vec<String>,
  pub supports_region: bool,
  pub max_page_size: i64,
  pub max_request_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FormCollectionPlanRequest {
  pub platform: String,
  pub data_type: String,
  pub params: Value,
  pub request_limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CollectionParamValidationResult {
  pub valid: bool,
  pub errors: Vec<String>,
  pub missing_fields: Vec<String>,
  pub normalized_params: Value,
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

#[derive(Debug, Clone, Copy)]
struct EndpointDefinition {
  platform: &'static str,
  platform_name: &'static str,
  data_type: &'static str,
  data_type_name: &'static str,
  endpoint_key: &'static str,
  required_params: &'static [&'static str],
  optional_params: &'static [&'static str],
  supports_region: bool,
  max_page_size: i64,
  max_request_count: i64,
}

const ENDPOINTS: &[EndpointDefinition] = &[
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "tiktok.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 50,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "comments",
    data_type_name: "评论采集",
    endpoint_key: "tiktok.comments",
    required_params: &["item_id"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 100,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "account_profile",
    data_type_name: "账号公开信息",
    endpoint_key: "tiktok.account_profile",
    required_params: &["account_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 50,
  },
  EndpointDefinition {
    platform: "tiktok",
    platform_name: "TikTok",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "tiktok.item_detail",
    required_params: &["item_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "douyin.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 50,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "comments",
    data_type_name: "评论采集",
    endpoint_key: "douyin.comments",
    required_params: &["item_id"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 100,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "account_profile",
    data_type_name: "账号公开信息",
    endpoint_key: "douyin.account_profile",
    required_params: &["account_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 50,
  },
  EndpointDefinition {
    platform: "douyin",
    platform_name: "抖音",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "douyin.item_detail",
    required_params: &["item_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "keyword_search",
    data_type_name: "关键词搜索",
    endpoint_key: "xiaohongshu.keyword_search",
    required_params: &["keyword"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 50,
    max_request_count: 100,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "comments",
    data_type_name: "评论采集",
    endpoint_key: "xiaohongshu.comments",
    required_params: &["item_id"],
    optional_params: &["region", "time_range", "page_size"],
    supports_region: true,
    max_page_size: 100,
    max_request_count: 200,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "account_profile",
    data_type_name: "账号公开信息",
    endpoint_key: "xiaohongshu.account_profile",
    required_params: &["account_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 50,
  },
  EndpointDefinition {
    platform: "xiaohongshu",
    platform_name: "小红书",
    data_type: "item_detail",
    data_type_name: "笔记详情",
    endpoint_key: "xiaohongshu.item_detail",
    required_params: &["item_id"],
    optional_params: &["region"],
    supports_region: true,
    max_page_size: 1,
    max_request_count: 100,
  },
];

pub fn list_supported_platforms() -> Vec<PlatformCapabilityView> {
  ["tiktok", "douyin", "xiaohongshu"]
    .iter()
    .filter_map(|platform| {
      let endpoints = ENDPOINTS
        .iter()
        .filter(|endpoint| endpoint.platform == *platform)
        .collect::<Vec<_>>();
      endpoints.first().map(|first| PlatformCapabilityView {
        platform: (*platform).to_string(),
        display_name: first.platform_name.to_string(),
        data_types: endpoints
          .iter()
          .map(|endpoint| endpoint.data_type.to_string())
          .collect(),
      })
    })
    .collect()
}

pub fn list_platform_data_types(platform: &str) -> AppResult<Vec<DataTypeCapabilityView>> {
  let platform = normalize_platform(platform)?;
  let items = ENDPOINTS
    .iter()
    .filter(|endpoint| endpoint.platform == platform)
    .map(endpoint_to_view)
    .collect::<Vec<_>>();

  if items.is_empty() {
    return Err(collection_error("平台不受支持"));
  }

  Ok(items)
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

  if normalized_params.get("region").is_some() && !endpoint.supports_region {
    errors.push("该数据类型不支持国家/地区筛选".to_string());
  }

  if let Some(page_size) = normalized_params.get("page_size").and_then(Value::as_i64) {
    if page_size > endpoint.max_page_size {
      errors.push(format!("page_size 不能超过 {}", endpoint.max_page_size));
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
      if !allowed_params.contains(&key.as_str()) {
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

pub fn generate_form_collection_plan(
  request: FormCollectionPlanRequest,
) -> AppResult<CollectionPlanDraftView> {
  let endpoint = endpoint_for(&request.platform, &request.data_type)?;
  let validation = validate_collection_params(
    &request.platform,
    &request.data_type,
    request.params.clone(),
  )?;
  let request_limit = request
    .request_limit
    .unwrap_or(1)
    .clamp(1, endpoint.max_request_count);
  let cost = estimate_plan_cost(1, 1, request_limit);
  let validation_status = if validation.valid {
    "valid"
  } else {
    "needs_review"
  };

  let plan_json = serde_json::json!({
    "platforms": [endpoint.platform],
    "data_types": [endpoint.data_type],
    "region": validation.normalized_params.get("region").cloned().unwrap_or(Value::Null),
    "keywords": value_to_array(validation.normalized_params.get("keyword")),
    "accounts": value_to_array(validation.normalized_params.get("account")),
    "time_range": validation.normalized_params.get("time_range").cloned().unwrap_or(Value::Null),
    "steps": [{
      "endpoint_key": endpoint.endpoint_key,
      "platform": endpoint.platform,
      "data_type": endpoint.data_type,
      "params": validation.normalized_params
    }],
    "request_limit": request_limit,
    "cost_estimate": cost.cost_estimate_json,
    "missing_fields": validation.missing_fields,
    "confidence": if validation.valid { 1.0 } else { 0.4 },
    "requires_user_confirmation": true
  });

  Ok(CollectionPlanDraftView {
    source: "form_generated".to_string(),
    schema_version: 1,
    plan_json,
    validation_status: validation_status.to_string(),
    validation_errors_json: serde_json::json!(validation.errors),
    cost_estimate_json: cost.cost_estimate_json,
  })
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

fn endpoint_to_view(endpoint: &EndpointDefinition) -> DataTypeCapabilityView {
  DataTypeCapabilityView {
    platform: endpoint.platform.to_string(),
    data_type: endpoint.data_type.to_string(),
    display_name: endpoint.data_type_name.to_string(),
    endpoint_key: endpoint.endpoint_key.to_string(),
    required_params: endpoint
      .required_params
      .iter()
      .map(|value| (*value).to_string())
      .collect(),
    optional_params: endpoint
      .optional_params
      .iter()
      .map(|value| (*value).to_string())
      .collect(),
    supports_region: endpoint.supports_region,
    max_page_size: endpoint.max_page_size,
    max_request_count: endpoint.max_request_count,
  }
}

fn endpoint_for(platform: &str, data_type: &str) -> AppResult<&'static EndpointDefinition> {
  let platform = normalize_platform(platform)?;
  let data_type = data_type.trim();

  ENDPOINTS
    .iter()
    .find(|endpoint| endpoint.platform == platform && endpoint.data_type == data_type)
    .ok_or_else(|| collection_error("平台或数据类型不受支持"))
}

fn normalize_platform(platform: &str) -> AppResult<String> {
  match platform.trim() {
    "tiktok" | "douyin" | "xiaohongshu" => Ok(platform.trim().to_string()),
    _ => Err(collection_error("MVP 只支持 TikTok、抖音、小红书")),
  }
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
      platform: "xiaohongshu".to_string(),
      data_type: "comments".to_string(),
      params: serde_json::json!({ "item_id": "note-1", "region": "CN" }),
      request_limit: Some(2),
    })
    .expect("plan should generate");

    assert_eq!(plan.validation_status, "valid");
    assert_eq!(
      plan.plan_json["steps"][0]["endpoint_key"],
      "xiaohongshu.comments"
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
