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
