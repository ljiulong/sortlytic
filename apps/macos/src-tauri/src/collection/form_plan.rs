use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{
  endpoint_for, estimate_plan_cost, validate_collection_params, validate_collection_plan_v3,
  value_to_array, AgeRangeInput, CollectionPlanDraftView, FilterExecution,
  FormCollectionPlanRequest, PaginationMode,
};
use crate::domain::{AppError, AppErrorStage, AppResult};

pub fn generate_form_collection_plan(
  request: FormCollectionPlanRequest,
) -> AppResult<CollectionPlanDraftView> {
  let data_types = normalized_data_types(&request)?;
  let needs_search_dependency = !data_types.iter().any(|value| value == "keyword_search")
    && requires_search_dependency(&data_types, &request.params);
  let executable_data_types = if needs_search_dependency {
    std::iter::once("keyword_search".to_string())
      .chain(data_types.iter().cloned())
      .collect::<Vec<_>>()
  } else {
    data_types.clone()
  };
  let requested_limit = request.request_limit.unwrap_or(1).max(1);
  let mut steps = Vec::new();
  let mut total_request_limit = 0_i64;

  for data_type in &executable_data_types {
    let endpoint = endpoint_for(&request.platform, data_type)?;
    let params = step_params(data_type, &request.params, endpoint.optional_params)?;
    let validation = validate_collection_params(&request.platform, data_type, params)?;
    if !validation.valid {
      let mut messages = validation.errors;
      messages.extend(
        validation
          .missing_fields
          .into_iter()
          .map(|field| format!("{data_type} 缺少参数 {field}")),
      );
      return Err(AppError::validation(
        messages.join("；"),
        AppErrorStage::Collection,
      ));
    }
    let step_request_limit = match endpoint.pagination_mode {
      PaginationMode::Single => 1,
      PaginationMode::Cursor => requested_limit.clamp(1, endpoint.max_request_count),
    };
    total_request_limit = total_request_limit.saturating_add(step_request_limit);
    let depends_on_step_key =
      (needs_search_dependency && data_type != "keyword_search").then_some("keyword_search");
    let input_binding = if depends_on_step_key.is_some() {
      dependency_binding(data_type)
    } else {
      Value::Null
    };
    steps.push(serde_json::json!({
      "step_key": data_type,
      "role": if data_type == "keyword_search" { "entry" } else { "target" },
      "depends_on_step_key": depends_on_step_key,
      "input_binding": input_binding,
      "endpoint_key": endpoint.endpoint_key,
      "platform": endpoint.platform,
      "data_type": endpoint.data_type,
      "params": validation.normalized_params,
      "request_limit": step_request_limit,
      "output_selected": data_types.contains(data_type)
    }));
  }

  let default_record_limit = executable_data_types
    .iter()
    .filter_map(|data_type| endpoint_for(&request.platform, data_type).ok())
    .map(|endpoint| endpoint.max_page_size)
    .max()
    .unwrap_or(1)
    .saturating_mul(requested_limit)
    .max(1);
  let record_limit = request.record_limit.unwrap_or(default_record_limit);
  let budget_limit_micros = request.budget_limit_micros.unwrap_or(35_000_000);
  let cost = estimate_plan_cost(1, 1, total_request_limit.max(1));
  let region = filter_value_for_plan(&request, &executable_data_types, "region", |endpoint| {
    endpoint.region_filter
  });
  let time_range =
    filter_value_for_plan(&request, &executable_data_types, "time_range", |endpoint| {
      endpoint.time_range_filter
    });
  let age_range = request.age_range.as_ref().map(age_range_json);

  let plan_json = serde_json::json!({
    "schema_version": 3,
    "platforms": [request.platform],
    "data_types": data_types,
    "internal_data_types": if needs_search_dependency { vec!["keyword_search"] } else { Vec::<&str>::new() },
    "region": region,
    "keywords": value_to_array(request.params.get("keyword")),
    "accounts": value_to_array(request.params.get("account")),
    "time_range": time_range,
    "age_range": age_range,
    "steps": steps,
    "record_limit": record_limit,
    "request_limit": requested_limit,
    "budget_limit": {
      "currency": "USD",
      "amount_micros": budget_limit_micros
    },
    "output_rules": {
      "entity": "account",
      "dedupe_key": ["platform", "platform_user_id"],
      "fallback_dedupe_key": ["platform", "normalized_account"],
      "selected_data_types": data_types
    },
    "cost_estimate": cost.cost_estimate_json,
    "missing_fields": [],
    "confidence": 1.0,
    "requires_user_confirmation": true
  });
  let plan_validation = validate_collection_plan_v3(&plan_json);

  Ok(CollectionPlanDraftView {
    source: "form_generated".to_string(),
    schema_version: 3,
    plan_json,
    validation_status: if plan_validation.valid {
      "valid".to_string()
    } else {
      "needs_review".to_string()
    },
    validation_errors_json: serde_json::json!(plan_validation.errors),
    cost_estimate_json: cost.cost_estimate_json,
  })
}

fn normalized_data_types(request: &FormCollectionPlanRequest) -> AppResult<Vec<String>> {
  let candidates = if request.data_types.is_empty() {
    request.data_type.iter().cloned().collect::<Vec<_>>()
  } else {
    request.data_types.clone()
  };
  let mut seen = BTreeSet::new();
  let data_types = candidates
    .into_iter()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty() && seen.insert(value.clone()))
    .collect::<Vec<_>>();
  if data_types.is_empty() {
    return Err(AppError::validation(
      "至少选择一种目标数据",
      AppErrorStage::Collection,
    ));
  }
  for data_type in &data_types {
    endpoint_for(&request.platform, data_type)?;
  }
  Ok(data_types)
}

fn step_params(data_type: &str, source: &Value, optional_params: &[&str]) -> AppResult<Value> {
  let source = source
    .as_object()
    .ok_or_else(|| AppError::validation("params 必须是对象", AppErrorStage::Collection))?;
  let mut params = Map::new();
  match data_type {
    "keyword_search" => copy_string(source, "keyword", "keyword", &mut params),
    "item_detail" | "comments" => {
      params.insert(
        "item_id".to_string(),
        source
          .get("item_id")
          .cloned()
          .unwrap_or_else(|| Value::String("$steps.keyword_search.items[].item_id".to_string())),
      );
    }
    "account_profile" | "account_posts" => {
      params.insert(
        "account_id".to_string(),
        source
          .get("account_id")
          .cloned()
          .unwrap_or_else(|| Value::String("$steps.keyword_search.items[].account_id".to_string())),
      );
    }
    _ => {}
  }
  for key in optional_params {
    if let Some(value) = source.get(*key).cloned() {
      if *key == "page_size" {
        continue;
      }
      params.insert((*key).to_string(), value);
    }
  }
  Ok(Value::Object(params))
}

fn requires_search_dependency(data_types: &[String], params: &Value) -> bool {
  data_types.iter().any(|data_type| {
    let required_key = match data_type.as_str() {
      "item_detail" | "comments" => "item_id",
      "account_profile" | "account_posts" => "account_id",
      _ => return false,
    };
    params
      .get(required_key)
      .and_then(Value::as_str)
      .map(str::trim)
      .filter(|value| !value.is_empty())
      .is_none()
  })
}

fn copy_string(
  source: &Map<String, Value>,
  source_key: &str,
  target_key: &str,
  target: &mut Map<String, Value>,
) {
  if let Some(value) = source.get(source_key).cloned() {
    target.insert(target_key.to_string(), value);
  }
}

fn dependency_binding(data_type: &str) -> Value {
  match data_type {
    "item_detail" | "comments" => serde_json::json!({ "item_id": "item_id" }),
    "account_profile" | "account_posts" => serde_json::json!({ "account_id": "account_id" }),
    _ => Value::Null,
  }
}

fn filter_value_for_plan(
  request: &FormCollectionPlanRequest,
  data_types: &[String],
  field: &str,
  execution: impl Fn(&super::capabilities::EndpointDefinition) -> FilterExecution,
) -> Value {
  let supported = data_types.iter().any(|data_type| {
    endpoint_for(&request.platform, data_type)
      .is_ok_and(|endpoint| execution(endpoint) != FilterExecution::Unsupported)
  });
  if !supported {
    return Value::Null;
  }
  request.params.get(field).cloned().unwrap_or(Value::Null)
}

fn age_range_json(age_range: &AgeRangeInput) -> Value {
  serde_json::json!({ "min": age_range.min, "max": age_range.max })
}
