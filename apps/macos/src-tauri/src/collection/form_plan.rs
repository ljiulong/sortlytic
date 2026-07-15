use serde_json::Value;

use super::{
  endpoint_for, estimate_plan_cost, validate_collection_params, validate_collection_plan_v2,
  value_to_array, CollectionPlanDraftView, FormCollectionPlanRequest,
};
use crate::domain::AppResult;

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
  let default_record_limit = validation
    .normalized_params
    .get("page_size")
    .and_then(Value::as_i64)
    .unwrap_or(endpoint.max_page_size)
    .saturating_mul(request_limit)
    .max(1);
  let record_limit = request.record_limit.unwrap_or(default_record_limit);
  let budget_limit_micros = request.budget_limit_micros.unwrap_or(35_000_000);
  let cost = estimate_plan_cost(1, 1, request_limit);

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
    "record_limit": record_limit,
    "request_limit": request_limit,
    "budget_limit": {
      "currency": "USD",
      "amount_micros": budget_limit_micros
    },
    "cost_estimate": cost.cost_estimate_json,
    "missing_fields": validation.missing_fields,
    "confidence": if validation.valid { 1.0 } else { 0.4 },
    "requires_user_confirmation": true
  });
  let plan_validation = validate_collection_plan_v2(&plan_json);

  Ok(CollectionPlanDraftView {
    source: "form_generated".to_string(),
    schema_version: 2,
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
