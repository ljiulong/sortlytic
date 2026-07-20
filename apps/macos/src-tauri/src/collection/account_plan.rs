use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::account_capabilities::source_covers_field;
use super::account_input::normalize_account_source_input;
use super::{
  get_account_collection_capabilities, validate_collection_params, AccountCollectionCapabilityView,
  AccountFieldAvailability, AccountSourceInputKind, AgeRangeInput, CollectionPlanDraftView,
  CollectionPlanValidationResult, PaginationMode,
};
use crate::domain::{AppError, AppErrorStage, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountFormCollectionPlanRequest {
  pub platform: String,
  pub account_source: String,
  #[serde(default)]
  pub selected_fields: Vec<String>,
  #[serde(default = "default_enrichment_policy")]
  pub enrichment_policy: String,
  pub params: Value,
  pub age_range: Option<AgeRangeInput>,
  pub gender_filter: Option<Vec<String>>,
  pub request_limit: Option<i64>,
  pub record_limit: Option<i64>,
  pub budget_limit_micros: Option<i64>,
}

pub fn generate_account_collection_plan(
  request: AccountFormCollectionPlanRequest,
) -> AppResult<CollectionPlanDraftView> {
  if request.enrichment_policy.trim() != "auto_costed" {
    return Err(validation_error("enrichment_policy 只能为 auto_costed"));
  }
  if request.request_limit.is_some_and(|value| value <= 0) {
    return Err(validation_error("request_limit 必须是大于 0 的整数"));
  }
  let capability = get_account_collection_capabilities(&request.platform)?;
  let source = capability
    .account_sources
    .iter()
    .find(|source| source.key == request.account_source.trim())
    .ok_or_else(|| validation_error("当前平台不支持所选账号来源"))?;
  let selected_fields = normalize_account_fields(&capability, &request.selected_fields)?;
  validate_requested_filters(
    request.age_range.as_ref(),
    request.gender_filter.as_deref(),
    &selected_fields,
  )?;
  let enrichment_operations =
    required_enrichment_operations(&capability, &selected_fields, request.account_source.trim());
  let source_params = normalize_account_source_params(
    &capability.platform,
    &source.key,
    &source.endpoint_key,
    source.input_kind,
    &request.params,
  )?;
  let requested_source_limit = request
    .request_limit
    .unwrap_or(1)
    .clamp(1, source.max_request_count);
  let default_record_limit = match source.pagination_mode {
    PaginationMode::Single => 1,
    PaginationMode::Cursor => source
      .max_page_size
      .saturating_mul(requested_source_limit)
      .max(1),
  };
  let record_limit = request.record_limit.unwrap_or(default_record_limit);
  if record_limit <= 0 {
    return Err(validation_error("record_limit 必须是大于 0 的整数"));
  }
  if source.pagination_mode == PaginationMode::Single && record_limit != 1 {
    return Err(validation_error(
      "单账号或单作品来源的 record_limit 必须为 1",
    ));
  }
  if record_limit > default_record_limit {
    return Err(validation_error(format!(
      "record_limit 超过当前 request_limit 最多可发现的 {default_record_limit} 条账号",
    )));
  }
  let budget_limit_micros = request.budget_limit_micros.unwrap_or(35_000_000);
  if budget_limit_micros <= 0 {
    return Err(validation_error(
      "budget_limit_micros 必须是大于 0 的整数微美元",
    ));
  }

  let discovery_request_count = match source.pagination_mode {
    PaginationMode::Single => 1,
    PaginationMode::Cursor => record_limit
      .saturating_add(source.max_page_size - 1)
      .saturating_div(source.max_page_size)
      .clamp(1, requested_source_limit),
  };
  let mut steps = vec![serde_json::json!({
    "step_key": "discover",
    "operation_key": format!("discover.{}", source.key),
    "role": "discovery",
    "depends_on_step_key": Value::Null,
    "input_binding": Value::Null,
    "endpoint_key": source.endpoint_key,
    "platform": capability.platform,
    "data_type": endpoint_data_type(&source.endpoint_key),
    "params": source_params,
    "request_limit": discovery_request_count,
    "output_selected": true
  })];
  for (index, operation_key) in enrichment_operations.iter().enumerate() {
    let endpoint_key = enrichment_endpoint_key(&capability.platform, operation_key)
      .expect("能力目录只能返回已注册的补全操作");
    let input_field = enrichment_input_field(&capability.platform, operation_key);
    steps.push(serde_json::json!({
      "step_key": format!("enrich_{}", index + 1),
      "operation_key": operation_key,
      "role": "enrichment",
      "depends_on_step_key": "discover",
      "input_binding": { "account_id": input_field },
      "endpoint_key": endpoint_key,
      "platform": capability.platform,
      "data_type": endpoint_data_type(&endpoint_key),
      "params": {
        "account_id": format!("$steps.discover.accounts[].{input_field}")
      },
      "request_limit": 1,
      "output_selected": true
    }));
  }

  let enrichment_request_count = record_limit.saturating_mul(enrichment_operations.len() as i64);
  let total_request_count = discovery_request_count.saturating_add(enrichment_request_count);
  let cost_estimate = serde_json::json!({
    "request_count_estimate": total_request_count,
    "discovery_request_count": discovery_request_count,
    "enrichment_request_count": enrichment_request_count,
    "enrichment_operation_count": enrichment_operations.len(),
    "requires_confirmation": true
  });
  let gender_filter = normalize_gender_filter(request.gender_filter.as_deref())?;
  let plan_json = serde_json::json!({
    "schema_version": 4,
    "entity": "account",
    "platforms": [capability.platform],
    "account_source": source.key,
    "selected_fields": selected_fields,
    "enrichment_policy": "auto_costed",
    "region": request.params.get("region").cloned().unwrap_or(Value::Null),
    "time_range": request.params.get("time_range").cloned().unwrap_or(Value::Null),
    "age_range": request.age_range.as_ref().map(age_range_json),
    "gender_filter": gender_filter,
    "steps": steps,
    "record_limit": record_limit,
    "request_limit": requested_source_limit,
    "budget_limit": {
      "currency": "USD",
      "amount_micros": budget_limit_micros
    },
    "output_rules": {
      "entity": "account",
      "required_fields": [
        "platform", "display_name", "account_handle", "platform_user_id",
        "data_source", "collected_at"
      ],
      "selected_fields": selected_fields,
      "dedupe_key": ["platform", "platform_user_id"],
      "fallback_dedupe_key": ["platform", "account_handle"],
      "unselected_value_label": "任务未设置",
      "missing_value_label": "未采集到",
      "evidence_required": true
    },
    "cost_estimate": cost_estimate,
    "missing_fields": [],
    "confidence": 1.0,
    "requires_user_confirmation": true
  });
  let validation = validate_collection_plan_v4(&plan_json);

  Ok(CollectionPlanDraftView {
    source: "form_generated".to_string(),
    schema_version: 4,
    plan_json,
    validation_status: if validation.valid {
      "valid".to_string()
    } else {
      "needs_review".to_string()
    },
    validation_errors_json: serde_json::json!(validation.errors),
    cost_estimate_json: cost_estimate,
  })
}

pub fn validate_collection_plan_v4(plan_json: &Value) -> CollectionPlanValidationResult {
  let mut errors = Vec::new();
  if plan_json.get("schema_version").and_then(Value::as_i64) != Some(4) {
    errors.push("schema_version 必须为 4".to_string());
  }
  if plan_json.get("entity").and_then(Value::as_str) != Some("account") {
    errors.push("entity 必须为 account".to_string());
  }
  let platforms = plan_json.get("platforms").and_then(Value::as_array);
  let platform = platforms
    .filter(|platforms| platforms.len() == 1)
    .and_then(|platforms| platforms[0].as_str());
  if platform.is_none() {
    errors.push("platforms 必须只包含一个受支持平台".to_string());
  }
  let capability = platform.and_then(|platform| {
    get_account_collection_capabilities(platform)
      .map_err(|error| errors.push(error.message))
      .ok()
  });
  let account_source = plan_json
    .get("account_source")
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty());
  if account_source.is_none() {
    errors.push("account_source 不能为空".to_string());
  }
  if plan_json.get("enrichment_policy").and_then(Value::as_str) != Some("auto_costed") {
    errors.push("enrichment_policy 只能为 auto_costed".to_string());
  }
  let selected_values = plan_json.get("selected_fields").and_then(Value::as_array);
  let selected_fields = selected_values.map(|values| {
    values
      .iter()
      .filter_map(Value::as_str)
      .map(ToString::to_string)
      .collect::<Vec<_>>()
  });
  if selected_fields.is_none()
    || selected_values.is_some_and(|values| values.iter().any(|value| value.as_str().is_none()))
  {
    errors.push("selected_fields 必须是字符串数组".to_string());
  }

  let mut expected_operations = Vec::new();
  let mut expected_endpoints = BTreeMap::new();
  let mut expected_source_params = Vec::new();
  let mut source_limits = None;
  if let (Some(capability), Some(account_source), Some(selected_fields)) =
    (&capability, account_source, &selected_fields)
  {
    if let Some(source) = capability
      .account_sources
      .iter()
      .find(|source| source.key == account_source)
    {
      let discovery = format!("discover.{account_source}");
      expected_source_params.push(source_param_key(source.input_kind));
      if capability.platform == "xiaohongshu"
        && source.input_kind != AccountSourceInputKind::Keyword
      {
        expected_source_params.push("share_text");
      }
      source_limits = Some((
        source.pagination_mode,
        source.max_page_size,
        source.max_request_count,
      ));
      expected_endpoints.insert(discovery.clone(), source.endpoint_key.clone());
      expected_operations.push(discovery);
      match normalize_account_fields(capability, selected_fields) {
        Ok(normalized_fields) => {
          if normalized_fields != *selected_fields {
            errors.push("selected_fields 不得包含空白或重复字段".to_string());
          }
          for operation in
            required_enrichment_operations(capability, &normalized_fields, account_source)
          {
            if let Some(endpoint) = enrichment_endpoint_key(&capability.platform, &operation) {
              expected_endpoints.insert(operation.clone(), endpoint);
              expected_operations.push(operation);
            }
          }
        }
        Err(error) => errors.push(error.message),
      }
    } else {
      errors.push("当前平台不支持 account_source".to_string());
    }
  }

  validate_plan_limits(plan_json, &mut errors);
  validate_plan_filters(plan_json, selected_fields.as_deref(), &mut errors);
  let record_limit = plan_json
    .get("record_limit")
    .and_then(Value::as_i64)
    .unwrap_or(0)
    .max(0);
  let request_limit = plan_json
    .get("request_limit")
    .and_then(Value::as_i64)
    .unwrap_or(0)
    .max(0);
  let expected_discovery_request_count =
    source_limits.map(|(pagination_mode, max_page_size, max_request_count)| {
      if request_limit > max_request_count {
        errors.push(format!(
          "request_limit 超过当前账号来源上限 {max_request_count}"
        ));
      }
      match pagination_mode {
        PaginationMode::Single => {
          if record_limit != 1 || request_limit != 1 {
            errors.push("单账号或单作品来源的记录和请求上限必须为 1".to_string());
          }
          1
        }
        PaginationMode::Cursor => {
          let capacity = max_page_size.saturating_mul(request_limit);
          if record_limit > capacity {
            errors.push(format!(
              "record_limit 超过当前请求上限最多可发现的 {capacity} 条账号"
            ));
          }
          record_limit
            .saturating_add(max_page_size - 1)
            .saturating_div(max_page_size)
            .clamp(1, request_limit.max(1))
        }
      }
    });
  let mut actual_operations = Vec::new();
  let mut actual_step_keys = BTreeSet::new();
  let mut discovery_request_count = 0_i64;
  let mut enrichment_request_count = 0_i64;
  let mut enrichment_operation_count = 0_i64;
  match plan_json.get("steps").and_then(Value::as_array) {
    Some(steps) if !steps.is_empty() => {
      for (index, step) in steps.iter().enumerate() {
        let prefix = format!("steps[{index}]");
        let Some(step) = step.as_object() else {
          errors.push(format!("{prefix} 必须是对象"));
          continue;
        };
        let step_key = step
          .get("step_key")
          .and_then(Value::as_str)
          .map(str::trim)
          .filter(|value| !value.is_empty());
        match step_key {
          Some(step_key) if actual_step_keys.insert(step_key.to_string()) => {}
          Some(_) => errors.push(format!("{prefix}.step_key 不能重复")),
          None => errors.push(format!("{prefix}.step_key 不能为空")),
        }
        let operation = step
          .get("operation_key")
          .and_then(Value::as_str)
          .map(str::trim)
          .filter(|value| !value.is_empty());
        let Some(operation) = operation else {
          errors.push(format!("{prefix}.operation_key 不能为空"));
          continue;
        };
        if actual_operations.iter().any(|value| value == operation) {
          errors.push(format!("{prefix}.operation_key 不能重复"));
        }
        actual_operations.push(operation.to_string());
        if let Some(expected_endpoint) = expected_endpoints.get(operation) {
          if step.get("endpoint_key").and_then(Value::as_str) != Some(expected_endpoint) {
            errors.push(format!("{prefix}.endpoint_key 与 operation_key 不匹配"));
          }
          if step.get("data_type").and_then(Value::as_str)
            != Some(endpoint_data_type(expected_endpoint))
          {
            errors.push(format!("{prefix}.data_type 与 operation_key 不匹配"));
          }
        }
        if step.get("platform").and_then(Value::as_str) != platform {
          errors.push(format!("{prefix}.platform 与顶层平台不一致"));
        }
        if let (Some(step_platform), Some(data_type), Some(params)) = (
          step.get("platform").and_then(Value::as_str),
          step.get("data_type").and_then(Value::as_str),
          step.get("params"),
        ) {
          match validate_collection_params(step_platform, data_type, params.clone()) {
            Ok(validation) => {
              for field in validation.missing_fields {
                errors.push(format!("{prefix}.params 缺少 {field}"));
              }
              for error in validation.errors {
                errors.push(format!("{prefix}.params：{error}"));
              }
            }
            Err(error) => errors.push(format!("{prefix}.params：{}", error.message)),
          }
        } else if step.get("params").is_none() {
          errors.push(format!("{prefix}.params 必须是对象"));
        }
        let step_request_limit = step
          .get("request_limit")
          .and_then(Value::as_i64)
          .filter(|limit| *limit > 0);
        if step_request_limit.is_none() {
          errors.push(format!("{prefix}.request_limit 必须是大于 0 的整数"));
        }
        if operation.starts_with("discover.") {
          if step_key != Some("discover") {
            errors.push(format!("{prefix}.step_key 必须为 discover"));
          }
          if step.get("role").and_then(Value::as_str) != Some("discovery") {
            errors.push(format!("{prefix}.role 必须为 discovery"));
          }
          if !step.get("depends_on_step_key").is_some_and(Value::is_null) {
            errors.push(format!("{prefix}.depends_on_step_key 必须为 null"));
          }
          if !step.get("input_binding").is_some_and(Value::is_null) {
            errors.push(format!("{prefix}.input_binding 必须为 null"));
          }
          discovery_request_count =
            discovery_request_count.saturating_add(step_request_limit.unwrap_or_default());
          if step
            .get("params")
            .and_then(Value::as_object)
            .is_none_or(|params| {
              !expected_source_params.iter().any(|key| {
                params
                  .get(*key)
                  .and_then(Value::as_str)
                  .is_some_and(|value| !value.trim().is_empty())
              })
            })
          {
            errors.push(format!("{prefix}.params 缺少账号来源输入"));
          }
        } else {
          enrichment_operation_count = enrichment_operation_count.saturating_add(1);
          if step.get("role").and_then(Value::as_str) != Some("enrichment") {
            errors.push(format!("{prefix}.role 必须为 enrichment"));
          }
          enrichment_request_count = enrichment_request_count
            .saturating_add(record_limit.saturating_mul(step_request_limit.unwrap_or_default()));
          if step.get("depends_on_step_key").and_then(Value::as_str) != Some("discover") {
            errors.push(format!("{prefix}.depends_on_step_key 必须引用 discover"));
          }
          if let Some(operation_index) = expected_operations
            .iter()
            .position(|expected| expected == operation)
          {
            let expected_step_key = format!("enrich_{operation_index}");
            if step_key != Some(expected_step_key.as_str()) {
              errors.push(format!("{prefix}.step_key 与 operation_key 不匹配"));
            }
          }
          if let Some(platform) = platform {
            let expected_input = enrichment_input_field(platform, operation);
            if step
              .get("input_binding")
              .and_then(|binding| binding.get("account_id"))
              .and_then(Value::as_str)
              != Some(expected_input)
            {
              errors.push(format!("{prefix}.input_binding 与 operation_key 不匹配"));
            }
            let expected_param = format!("$steps.discover.accounts[].{expected_input}");
            if step
              .get("params")
              .and_then(|params| params.get("account_id"))
              .and_then(Value::as_str)
              != Some(expected_param.as_str())
            {
              errors.push(format!("{prefix}.params.account_id 与输入绑定不匹配"));
            }
          }
        }
        if step.get("output_selected").and_then(Value::as_bool) != Some(true) {
          errors.push(format!("{prefix}.output_selected 必须为 true"));
        }
      }
    }
    _ => errors.push("steps 必须是非空数组".to_string()),
  }
  for operation in &expected_operations {
    if !actual_operations.contains(operation) {
      errors.push(format!("steps 缺少操作 {operation}"));
    }
  }
  for operation in &actual_operations {
    if !expected_operations.contains(operation) {
      errors.push(format!("steps 包含未声明操作 {operation}"));
    }
  }
  if actual_operations.first() != expected_operations.first() {
    errors.push("第一个步骤必须是当前账号来源的发现操作".to_string());
  }
  if actual_operations.len() == expected_operations.len()
    && actual_operations != expected_operations
  {
    errors.push("steps 必须按发现和补全依赖顺序排列".to_string());
  }
  if expected_discovery_request_count != Some(discovery_request_count) {
    errors.push("发现步骤请求数与来源容量和 record_limit 不一致".to_string());
  }
  let calculated_request_count = discovery_request_count.saturating_add(enrichment_request_count);
  let cost_estimate = plan_json.get("cost_estimate");
  for (field, expected) in [
    ("request_count_estimate", calculated_request_count),
    ("discovery_request_count", discovery_request_count),
    ("enrichment_request_count", enrichment_request_count),
    ("enrichment_operation_count", enrichment_operation_count),
  ] {
    if cost_estimate
      .and_then(|cost| cost.get(field))
      .and_then(Value::as_i64)
      != Some(expected)
    {
      errors.push(format!("cost_estimate.{field} 与步骤计价不一致"));
    }
  }
  if cost_estimate
    .and_then(|cost| cost.get("requires_confirmation"))
    .and_then(Value::as_bool)
    != Some(true)
  {
    errors.push("cost_estimate.requires_confirmation 必须为 true".to_string());
  }
  let expected_output_rules = serde_json::json!({
    "entity": "account",
    "required_fields": [
      "platform", "display_name", "account_handle", "platform_user_id",
      "data_source", "collected_at"
    ],
    "selected_fields": plan_json.get("selected_fields").cloned().unwrap_or(Value::Null),
    "dedupe_key": ["platform", "platform_user_id"],
    "fallback_dedupe_key": ["platform", "account_handle"],
    "unselected_value_label": "任务未设置",
    "missing_value_label": "未采集到",
    "evidence_required": true
  });
  if plan_json.get("output_rules") != Some(&expected_output_rules) {
    errors.push("output_rules 与账号输出契约不一致".to_string());
  }
  if plan_json
    .get("requires_user_confirmation")
    .and_then(Value::as_bool)
    != Some(true)
  {
    errors.push("requires_user_confirmation 必须为 true".to_string());
  }
  errors.sort();
  errors.dedup();
  CollectionPlanValidationResult {
    valid: errors.is_empty(),
    errors,
  }
}

fn default_enrichment_policy() -> String {
  "auto_costed".to_string()
}

fn normalize_account_fields(
  capability: &AccountCollectionCapabilityView,
  values: &[String],
) -> AppResult<Vec<String>> {
  let mut seen = BTreeSet::new();
  let mut normalized = Vec::new();
  for value in values {
    let key = value.trim();
    if key.is_empty() || !seen.insert(key.to_string()) {
      return Err(validation_error("selected_fields 不得包含空白或重复字段"));
    }
    let field = capability
      .fields
      .iter()
      .find(|field| field.key == key)
      .ok_or_else(|| validation_error(format!("未知结果字段 {key}")))?;
    if field.availability == AccountFieldAvailability::Unsupported {
      return Err(validation_error(format!(
        "当前平台不支持字段 {}：{}",
        field.display_name,
        field.missing_reason.as_deref().unwrap_or("没有可验证来源")
      )));
    }
    normalized.push(key.to_string());
  }
  Ok(normalized)
}

fn required_enrichment_operations(
  capability: &AccountCollectionCapabilityView,
  selected_fields: &[String],
  account_source: &str,
) -> Vec<String> {
  let mut operations = Vec::new();
  for key in selected_fields {
    if let Some(field) = capability.fields.iter().find(|field| field.key == *key) {
      if source_covers_field(&capability.platform, account_source, &field.key) {
        continue;
      }
      for operation in &field.required_operation_keys {
        if !operations.contains(operation) {
          operations.push(operation.clone());
        }
      }
    }
  }
  operations
}

fn enrichment_endpoint_key(platform: &str, operation_key: &str) -> Option<String> {
  let suffix = match operation_key {
    "enrich.profile" => "account_profile",
    "enrich.extended_demographics" => "extended_demographics",
    "enrich.account_country" => "account_country",
    "enrich.account_posts" => "account_posts",
    _ => return None,
  };
  Some(format!("{platform}.{suffix}"))
}

fn endpoint_data_type(endpoint_key: &str) -> &str {
  endpoint_key
    .split_once('.')
    .map_or(endpoint_key, |(_, suffix)| suffix)
}

fn enrichment_input_field(platform: &str, operation_key: &str) -> &'static str {
  match (platform, operation_key) {
    (_, "enrich.account_country") => "account_handle",
    ("tiktok", "enrich.profile") => "account_handle",
    ("douyin", "enrich.profile" | "enrich.extended_demographics") => "secure_user_id",
    ("tiktok" | "douyin", "enrich.account_posts") => "secure_user_id",
    _ => "platform_user_id",
  }
}

fn normalize_account_source_params(
  platform: &str,
  account_source: &str,
  endpoint_key: &str,
  input_kind: AccountSourceInputKind,
  params: &Value,
) -> AppResult<Value> {
  let params = params
    .as_object()
    .ok_or_else(|| validation_error("params 必须是对象"))?;
  let aliases: &[&str] = match input_kind {
    AccountSourceInputKind::Keyword => &["source_input", "keyword"],
    AccountSourceInputKind::Account => &["source_input", "account_id", "account"],
    AccountSourceInputKind::Item => &["source_input", "item_id"],
  };
  let source_input = aliases
    .iter()
    .find_map(|key| params.get(*key).and_then(Value::as_str))
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| validation_error("账号来源输入不能为空"))?;
  let source_input =
    normalize_account_source_input(platform, account_source, input_kind, source_input)?;
  let mut normalized = Map::from_iter([(
    source_input.key.as_str().to_string(),
    Value::String(source_input.value),
  )]);
  let endpoint = super::capabilities::endpoint_for(platform, endpoint_data_type(endpoint_key))?;
  for key in ["region", "time_range"] {
    let is_allowed =
      endpoint.required_params.contains(&key) || endpoint.optional_params.contains(&key);
    if is_allowed {
      if let Some(value) = params.get(key).cloned().filter(|value| !value.is_null()) {
        normalized.insert(key.to_string(), value);
      }
    }
  }
  Ok(Value::Object(normalized))
}

fn source_param_key(input_kind: AccountSourceInputKind) -> &'static str {
  match input_kind {
    AccountSourceInputKind::Keyword => "keyword",
    AccountSourceInputKind::Account => "account_id",
    AccountSourceInputKind::Item => "item_id",
  }
}

fn validate_requested_filters(
  age_range: Option<&AgeRangeInput>,
  gender_filter: Option<&[String]>,
  selected_fields: &[String],
) -> AppResult<()> {
  if let Some(age_range) = age_range {
    if !(0..=130).contains(&age_range.min) || age_range.min > age_range.max || age_range.max > 130 {
      return Err(validation_error(
        "age_range 必须是 0–130 内且 min <= max 的整数闭区间",
      ));
    }
    if !selected_fields.iter().any(|field| field == "age") {
      return Err(validation_error("启用 age_range 时必须选择 age 字段"));
    }
  }
  if gender_filter.is_some_and(|values| !values.is_empty())
    && !selected_fields.iter().any(|field| field == "gender")
  {
    return Err(validation_error(
      "启用 gender_filter 时必须选择 gender 字段",
    ));
  }
  normalize_gender_filter(gender_filter).map(|_| ())
}

fn normalize_gender_filter(values: Option<&[String]>) -> AppResult<Value> {
  let Some(values) = values.filter(|values| !values.is_empty()) else {
    return Ok(Value::Null);
  };
  let mut normalized = BTreeSet::new();
  for value in values {
    if !matches!(value.as_str(), "male" | "female" | "other") || !normalized.insert(value.clone()) {
      return Err(validation_error(
        "gender_filter 只能包含不重复的 male、female、other",
      ));
    }
  }
  Ok(serde_json::json!(normalized))
}

fn validate_plan_limits(plan_json: &Value, errors: &mut Vec<String>) {
  for field in ["record_limit", "request_limit"] {
    if plan_json
      .get(field)
      .and_then(Value::as_i64)
      .is_none_or(|value| value <= 0)
    {
      errors.push(format!("{field} 必须是大于 0 的整数"));
    }
  }
  let valid_budget = plan_json
    .get("budget_limit")
    .and_then(Value::as_object)
    .is_some_and(|budget| {
      budget.get("currency").and_then(Value::as_str) == Some("USD")
        && budget
          .get("amount_micros")
          .and_then(Value::as_i64)
          .is_some_and(|value| value > 0)
    });
  if !valid_budget {
    errors.push("budget_limit 必须包含 USD 正整数微美元上限".to_string());
  }
  if plan_json
    .get("cost_estimate")
    .and_then(Value::as_object)
    .is_none()
  {
    errors.push("cost_estimate 必须是对象".to_string());
  }
}

fn validate_plan_filters(
  plan_json: &Value,
  selected_fields: Option<&[String]>,
  errors: &mut Vec<String>,
) {
  if let Some(age_range) = plan_json.get("age_range").filter(|value| !value.is_null()) {
    let bounds = age_range
      .get("min")
      .and_then(Value::as_i64)
      .zip(age_range.get("max").and_then(Value::as_i64));
    if !bounds.is_some_and(|(min, max)| (0..=130).contains(&min) && min <= max && max <= 130) {
      errors.push("age_range 必须是 0–130 内且 min <= max 的整数闭区间".to_string());
    }
    if selected_fields.is_none_or(|fields| !fields.iter().any(|field| field == "age")) {
      errors.push("启用 age_range 时必须选择 age 字段".to_string());
    }
  }
  if let Some(filter) = plan_json
    .get("gender_filter")
    .filter(|value| !value.is_null())
  {
    let mut seen = BTreeSet::new();
    let valid = filter.as_array().is_some_and(|values| {
      !values.is_empty()
        && values.iter().all(|value| {
          value
            .as_str()
            .is_some_and(|value| matches!(value, "male" | "female" | "other") && seen.insert(value))
        })
    });
    if !valid {
      errors.push("gender_filter 只能包含不重复的 male、female、other".to_string());
    }
    if selected_fields.is_none_or(|fields| !fields.iter().any(|field| field == "gender")) {
      errors.push("启用 gender_filter 时必须选择 gender 字段".to_string());
    }
  }
}

fn age_range_json(age_range: &AgeRangeInput) -> Value {
  serde_json::json!({ "min": age_range.min, "max": age_range.max })
}

fn validation_error(message: impl Into<String>) -> AppError {
  AppError::validation(message, AppErrorStage::Collection)
}
