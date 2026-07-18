use serde::Deserialize;
use serde_json::{json, Value};

pub(super) fn collection_plan_schema() -> Value {
  let input_binding_schema = input_binding_schema();
  let params_schema = params_schema();
  let output_rules_schema = output_rules_schema();
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "schema_version": { "type": "integer", "const": 3 },
      "platforms": {
        "type": "array",
        "items": { "type": "string", "enum": ["tiktok", "douyin", "xiaohongshu"] }
      },
      "data_types": { "type": "array", "items": { "$ref": "#/$defs/data_type" } },
      "internal_data_types": { "type": "array", "items": { "$ref": "#/$defs/data_type" } },
      "region": {
        "anyOf": [
          { "type": "string" },
          { "type": "null" },
          {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "value": { "type": "string" },
              "validation_status": { "type": "string", "enum": ["verified", "unverified"] }
            },
            "required": ["value", "validation_status"]
          }
        ]
      },
      "keywords": { "type": "array", "items": { "type": "string" } },
      "accounts": { "type": "array", "items": { "type": "string" } },
      "time_range": { "type": ["string", "null"] },
      "age_range": {
        "anyOf": [
          { "type": "null" },
          {
            "type": "object",
            "additionalProperties": false,
            "properties": {
              "min": { "type": "integer", "minimum": 0, "maximum": 130 },
              "max": { "type": "integer", "minimum": 0, "maximum": 130 }
            },
            "required": ["min", "max"]
          }
        ]
      },
      "gender_filter": {
        "anyOf": [
          { "type": "null" },
          {
            "type": "array",
            "items": { "type": "string", "enum": ["male", "female", "other"] }
          }
        ]
      },
      "steps": {
        "type": "array",
        "minItems": 1,
        "items": {
          "type": "object",
          "additionalProperties": false,
          "properties": {
            "step_key": { "type": "string" },
            "role": { "type": "string", "enum": ["entry", "target"] },
            "depends_on_step_key": { "type": ["string", "null"] },
            "input_binding": input_binding_schema,
            "endpoint_key": { "type": "string" },
            "platform": { "type": "string", "enum": ["tiktok", "douyin", "xiaohongshu"] },
            "data_type": { "$ref": "#/$defs/data_type" },
            "params": params_schema,
            "request_limit": { "type": "integer", "minimum": 1 },
            "output_selected": { "type": "boolean" }
          },
          "required": [
            "step_key", "role", "depends_on_step_key", "input_binding", "endpoint_key",
            "platform", "data_type", "params", "request_limit", "output_selected"
          ]
        }
      },
      "record_limit": { "type": "integer", "minimum": 1 },
      "request_limit": { "type": "integer", "minimum": 1 },
      "budget_limit": {
        "type": "object",
        "additionalProperties": false,
        "properties": {
          "currency": { "type": "string", "const": "USD" },
          "amount_micros": { "type": "integer", "minimum": 1 }
        },
        "required": ["currency", "amount_micros"]
      },
      "output_rules": output_rules_schema,
      "missing_fields": { "type": "array", "items": { "type": "string" } },
      "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
      "requires_user_confirmation": { "type": "boolean", "const": true }
    },
    "required": [
      "schema_version", "platforms", "data_types", "internal_data_types", "region",
      "keywords", "accounts", "time_range", "age_range", "gender_filter", "steps",
      "record_limit", "request_limit", "budget_limit", "output_rules", "missing_fields",
      "confidence", "requires_user_confirmation"
    ],
    "$defs": {
      "data_type": {
        "type": "string",
        "enum": ["keyword_search", "comments", "account_profile", "account_posts", "item_detail"]
      }
    }
  })
}

fn input_binding_schema() -> Value {
  json!({
    "anyOf": [
      { "type": "null" },
      {
        "type": "object",
        "additionalProperties": false,
        "properties": { "item_id": { "type": "string", "const": "item_id" } },
        "required": ["item_id"]
      },
      {
        "type": "object",
        "additionalProperties": false,
        "properties": { "account_id": { "type": "string", "const": "account_id" } },
        "required": ["account_id"]
      }
    ]
  })
}

fn params_schema() -> Value {
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "keyword": { "type": ["string", "null"] },
      "item_id": { "type": ["string", "null"] },
      "account_id": { "type": ["string", "null"] },
      "region": { "type": ["string", "null"] },
      "time_range": { "type": ["string", "null"] },
      "page_size": {
        "anyOf": [
          { "type": "integer", "minimum": 1 },
          { "type": "null" }
        ]
      }
    },
    "required": ["keyword", "item_id", "account_id", "region", "time_range", "page_size"]
  })
}

fn output_rules_schema() -> Value {
  json!({
    "type": "object",
    "additionalProperties": false,
    "properties": {
      "entity": { "type": "string", "const": "account" },
      "dedupe_key": { "type": "array", "items": { "type": "string" } },
      "fallback_dedupe_key": { "type": "array", "items": { "type": "string" } },
      "selected_data_types": { "type": "array", "items": { "$ref": "#/$defs/data_type" } }
    },
    "required": ["entity", "dedupe_key", "fallback_dedupe_key", "selected_data_types"]
  })
}

pub(crate) fn validate_collection_plan_schema(plan: &Value) -> Vec<String> {
  let parsed = match serde_json::from_value::<strict_contract::CollectionPlanV3>(plan.clone()) {
    Ok(parsed) => parsed,
    Err(error) => return vec![format!("模型输出不符合 collection_plan_v3 Schema：{error}")],
  };
  let mut errors = Vec::new();
  if parsed.schema_version != 3 {
    errors.push("collection_plan_v3.schema_version 必须为 3".to_string());
  }
  if parsed.steps.is_empty() {
    errors.push("collection_plan_v3.steps 至少需要一个步骤".to_string());
  }
  if parsed.record_limit == 0 || parsed.request_limit == 0 {
    errors.push("collection_plan_v3 的记录数与请求数上限必须大于 0".to_string());
  }
  if parsed.budget_limit.amount_micros == 0 {
    errors.push("collection_plan_v3 的预算上限必须大于 0".to_string());
  }
  if !(0.0..=1.0).contains(&parsed.confidence) {
    errors.push("collection_plan_v3.confidence 必须位于 0 到 1 之间".to_string());
  }
  if !parsed.requires_user_confirmation {
    errors.push("collection_plan_v3.requires_user_confirmation 必须为 true".to_string());
  }
  if let Some(age_range) = parsed.age_range.0 {
    if age_range.min > age_range.max || age_range.max > 130 {
      errors.push("collection_plan_v3.age_range 必须是 0 到 130 的有效闭区间".to_string());
    }
  }
  for (index, step) in parsed.steps.iter().enumerate() {
    if step.request_limit == 0 {
      errors.push(format!(
        "collection_plan_v3.steps[{index}].request_limit 必须大于 0"
      ));
    }
    if step.params.page_size.0 == Some(0) {
      errors.push(format!(
        "collection_plan_v3.steps[{index}].params.page_size 必须大于 0"
      ));
    }
  }
  errors
}

#[allow(dead_code)]
mod strict_contract {
  use super::Deserialize;

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct CollectionPlanV3 {
    pub schema_version: i64,
    pub platforms: Vec<Platform>,
    pub data_types: Vec<DataType>,
    pub internal_data_types: Vec<DataType>,
    pub region: Nullable<Region>,
    pub keywords: Vec<String>,
    pub accounts: Vec<String>,
    pub time_range: Nullable<String>,
    pub age_range: Nullable<AgeRange>,
    pub gender_filter: Nullable<Vec<Gender>>,
    pub steps: Vec<Step>,
    pub record_limit: u64,
    pub request_limit: u64,
    pub budget_limit: BudgetLimit,
    pub output_rules: OutputRules,
    pub missing_fields: Vec<String>,
    pub confidence: f64,
    pub requires_user_confirmation: bool,
  }

  #[derive(Deserialize)]
  pub(super) struct Nullable<T>(pub Option<T>);

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum Platform {
    Tiktok,
    Douyin,
    Xiaohongshu,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum DataType {
    KeywordSearch,
    Comments,
    AccountProfile,
    AccountPosts,
    ItemDetail,
  }

  #[derive(Deserialize)]
  #[serde(untagged)]
  pub(super) enum Region {
    Value(String),
    Verified(VerifiedRegion),
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct VerifiedRegion {
    pub value: String,
    pub validation_status: RegionValidationStatus,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum RegionValidationStatus {
    Verified,
    Unverified,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct AgeRange {
    pub min: u64,
    pub max: u64,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum Gender {
    Male,
    Female,
    Other,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct Step {
    pub step_key: String,
    pub role: StepRole,
    pub depends_on_step_key: Nullable<String>,
    pub input_binding: Nullable<InputBinding>,
    pub endpoint_key: String,
    pub platform: Platform,
    pub data_type: DataType,
    pub params: Params,
    pub request_limit: u64,
    pub output_selected: bool,
  }

  #[derive(Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(super) enum StepRole {
    Entry,
    Target,
  }

  #[derive(Deserialize)]
  #[serde(untagged)]
  pub(super) enum InputBinding {
    Item(ItemBinding),
    Account(AccountBinding),
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct ItemBinding {
    pub item_id: ItemBindingValue,
  }

  #[derive(Deserialize)]
  pub(super) enum ItemBindingValue {
    #[serde(rename = "item_id")]
    ItemId,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct AccountBinding {
    pub account_id: AccountBindingValue,
  }

  #[derive(Deserialize)]
  pub(super) enum AccountBindingValue {
    #[serde(rename = "account_id")]
    AccountId,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct Params {
    pub keyword: Nullable<String>,
    pub item_id: Nullable<String>,
    pub account_id: Nullable<String>,
    pub region: Nullable<String>,
    pub time_range: Nullable<String>,
    pub page_size: Nullable<u64>,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct BudgetLimit {
    pub currency: Currency,
    pub amount_micros: u64,
  }

  #[derive(Deserialize)]
  pub(super) enum Currency {
    #[serde(rename = "USD")]
    Usd,
  }

  #[derive(Deserialize)]
  #[serde(deny_unknown_fields)]
  pub(super) struct OutputRules {
    pub entity: OutputEntity,
    pub dedupe_key: Vec<String>,
    pub fallback_dedupe_key: Vec<String>,
    pub selected_data_types: Vec<DataType>,
  }

  #[derive(Deserialize)]
  pub(super) enum OutputEntity {
    #[serde(rename = "account")]
    Account,
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeSet;

  use super::*;

  #[test]
  fn every_object_is_closed_and_requires_all_declared_properties() {
    assert_strict_object_schemas(&collection_plan_schema(), "$schema");
  }

  fn assert_strict_object_schemas(schema: &Value, path: &str) {
    let is_object = schema.get("type").is_some_and(|value| {
      value.as_str() == Some("object")
        || value
          .as_array()
          .is_some_and(|types| types.iter().any(|value| value.as_str() == Some("object")))
    });
    if is_object {
      assert_eq!(
        schema.get("additionalProperties").and_then(Value::as_bool),
        Some(false),
        "{path} must reject additional properties"
      );
      let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{path} must define properties"));
      let required = schema
        .get("required")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{path} must require every property"));
      let property_names = properties
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
      let required_names = required
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
      assert_eq!(
        required_names, property_names,
        "{path} required fields must be exhaustive"
      );
    }
    match schema {
      Value::Object(object) => {
        for (key, value) in object {
          assert_strict_object_schemas(value, &format!("{path}/{key}"));
        }
      }
      Value::Array(values) => {
        for (index, value) in values.iter().enumerate() {
          assert_strict_object_schemas(value, &format!("{path}/{index}"));
        }
      }
      _ => {}
    }
  }
}
