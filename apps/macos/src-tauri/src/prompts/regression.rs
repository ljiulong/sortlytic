use std::path::Path;

use serde_json::Value;

use crate::ai::intent_plan::build_collection_plan_from_intent;
use crate::ai::{run_collection_prompt_regression, CollectionIntentV1};

use super::{PromptRegressionCaseView, PromptVersionView};

pub(super) struct PromptCaseEvaluation {
  pub schema_valid: bool,
  pub rules_valid: bool,
  pub error_summary: Option<String>,
  pub provider_id: Option<String>,
  pub model_id: Option<String>,
}

pub(super) fn evaluate_prompt_case(
  root_path: &Path,
  version: &PromptVersionView,
  case: &PromptRegressionCaseView,
) -> PromptCaseEvaluation {
  let mut schema_errors = Vec::new();
  let mut rule_errors = Vec::new();
  let (provider_id, model_id) = match case.expected_schema_id.as_str() {
    "collection_intent_v1" => evaluate_collection_case(
      root_path,
      version,
      case,
      &mut schema_errors,
      &mut rule_errors,
    ),
    "analysis_summary_v1" | "sentiment_v1" => {
      evaluate_analysis_case(version, case, &mut schema_errors, &mut rule_errors);
      (None, None)
    }
    schema_id => {
      schema_errors.push(format!("不支持的回归 Schema：{schema_id}"));
      (None, None)
    }
  };

  let schema_valid = schema_errors.is_empty();
  let rules_valid = rule_errors.is_empty();
  let error_summary = (!schema_valid || !rules_valid).then(|| {
    schema_errors
      .into_iter()
      .chain(rule_errors)
      .collect::<Vec<_>>()
      .join("；")
  });

  PromptCaseEvaluation {
    schema_valid,
    rules_valid,
    error_summary,
    provider_id,
    model_id,
  }
}

fn evaluate_collection_case(
  root_path: &Path,
  version: &PromptVersionView,
  case: &PromptRegressionCaseView,
  schema_errors: &mut Vec<String>,
  rule_errors: &mut Vec<String>,
) -> (Option<String>, Option<String>) {
  let Some(text) = case
    .input_json
    .get("text")
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|text| !text.is_empty())
  else {
    schema_errors.push("input_json.text 必须是非空字符串".to_string());
    return (None, None);
  };

  require_terms(
    &version.content,
    &[
      "json",
      "collection_intent_v1",
      "input_json.text",
      "schema_version",
      "platform",
      "account_source",
      "source_input",
      "query_locale",
      "region_code",
      "selected_fields",
      "time_range_days",
      "record_limit",
      "budget_limit_micros",
      "missing_fields",
      "endpoint_key",
      "翻译",
      "url",
      "证据",
      "不得猜测",
    ],
    rule_errors,
  );
  if !rule_errors.is_empty() {
    schema_errors.push("候选提示词静态约束未通过，未执行真实模型回归".to_string());
    return (None, None);
  }

  let response = match run_collection_prompt_regression(root_path, &version.content, text) {
    Ok(response) => response,
    Err(error) => {
      schema_errors.push(format!("真实模型回归失败：{}", error.message));
      return (None, None);
    }
  };
  let output = response.output_json.clone();
  for (output_field, rule_field) in [
    ("platform", "expected_platform"),
    ("account_source", "expected_account_source"),
    ("source_input", "expected_source_input"),
    ("query_locale", "expected_query_locale"),
    ("region_code", "expected_region_code"),
  ] {
    if case.expected_rules_json.get(rule_field).is_some() {
      compare_nullable_string_rule(
        &output,
        output_field,
        &case.expected_rules_json,
        rule_field,
        rule_errors,
      );
    }
  }
  for (output_field, rule_field) in [
    ("selected_fields", "expected_selected_fields"),
    ("missing_fields", "expected_missing_fields"),
  ] {
    if case.expected_rules_json.get(rule_field).is_some() {
      compare_string_array_rule(
        &output,
        output_field,
        &case.expected_rules_json,
        rule_field,
        rule_errors,
      );
    }
  }
  if case
    .expected_rules_json
    .get("source_input_ascii_letters")
    .and_then(Value::as_bool)
    == Some(true)
  {
    let valid = output
      .get("source_input")
      .and_then(Value::as_str)
      .is_some_and(|value| {
        !value.trim().is_empty()
          && value.is_ascii()
          && value
            .chars()
            .any(|character| character.is_ascii_alphabetic())
      });
    if !valid {
      rule_errors.push("source_input 应为非空英文检索词".to_string());
    }
  }
  for expected in string_array(case.expected_rules_json.get("expected_missing_contains")) {
    if !string_array(output.get("missing_fields")).contains(&expected) {
      rule_errors.push(format!("missing_fields 应包含 {expected}"));
    }
  }

  let parsed = serde_json::from_value::<CollectionIntentV1>(output).expect("AI 回归入口已校验意图");
  let built = build_collection_plan_from_intent(parsed);
  let plan_valid = built.validation_status == "valid" && built.collection_plan.is_some();
  if let Some(expected_valid) = case
    .expected_rules_json
    .get("expected_plan_valid")
    .and_then(Value::as_bool)
  {
    if plan_valid != expected_valid {
      rule_errors.push(format!(
        "计划校验结果应为 {expected_valid}，实际为 {plan_valid}"
      ));
    }
  }

  (Some(response.provider_id), Some(response.model_id))
}

fn evaluate_analysis_case(
  version: &PromptVersionView,
  case: &PromptRegressionCaseView,
  schema_errors: &mut Vec<String>,
  rule_errors: &mut Vec<String>,
) {
  let Some(records) = case.input_json.get("records").and_then(Value::as_array) else {
    schema_errors.push("input_json.records 必须是数组".to_string());
    return;
  };
  for (index, record) in records.iter().enumerate() {
    if record
      .get("id")
      .and_then(Value::as_str)
      .map(str::trim)
      .filter(|id| !id.is_empty())
      .is_none()
    {
      schema_errors.push(format!("input_json.records[{index}].id 不能为空"));
    }
  }

  require_terms(
    &version.content,
    &["json", "input_json.records", "source_record_ids"],
    rule_errors,
  );
  let records_empty = records.is_empty();
  if case
    .expected_rules_json
    .get("records_empty")
    .and_then(Value::as_bool)
    != Some(records_empty)
  {
    rule_errors.push("records_empty 预期与输入不一致".to_string());
  }
  if records_empty
    && (!version.content.contains("records 为空")
      || !(version.content.contains("空结果") || version.content.contains("不得编造")))
  {
    rule_errors.push("提示词未定义空 records 的无证据处理规则".to_string());
  }
}

fn compare_string_array_rule(
  output: &Value,
  output_field: &str,
  rules: &Value,
  rule_field: &str,
  errors: &mut Vec<String>,
) {
  let actual = string_array(output.get(output_field));
  let expected = string_array(rules.get(rule_field));
  if actual != expected {
    errors.push(format!(
      "{output_field} 应为 {expected:?}，实际为 {actual:?}"
    ));
  }
}

fn compare_nullable_string_rule(
  output: &Value,
  output_field: &str,
  rules: &Value,
  rule_field: &str,
  errors: &mut Vec<String>,
) {
  let actual = output.get(output_field).and_then(Value::as_str);
  let expected = rules.get(rule_field).and_then(Value::as_str);
  if actual != expected {
    errors.push(format!(
      "{output_field} 应为 {expected:?}，实际为 {actual:?}"
    ));
  }
}

fn string_array(value: Option<&Value>) -> Vec<String> {
  value
    .and_then(Value::as_array)
    .map(|values| {
      values
        .iter()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
    })
    .unwrap_or_default()
}

fn require_terms(content: &str, terms: &[&str], errors: &mut Vec<String>) {
  let normalized = content.to_lowercase();
  let missing = terms
    .iter()
    .filter(|term| !normalized.contains(*term))
    .copied()
    .collect::<Vec<_>>();
  if !missing.is_empty() {
    errors.push(format!("提示词缺少约束：{}", missing.join(", ")));
  }
}
