use std::path::Path;

use serde_json::Value;

use crate::ai::run_collection_prompt_regression;
use crate::collection::validate_collection_plan_v4;

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
    "collection_plan_v4" => evaluate_collection_case(
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
      "collection_plan_v4",
      "input_json.text",
      "schema_version",
      "entity",
      "platforms",
      "account_source",
      "selected_fields",
      "enrichment_policy",
      "region",
      "steps",
      "operation_key",
      "record_limit",
      "request_limit",
      "budget_limit",
      "output_rules",
      "cost_estimate",
      "missing_fields",
      "requires_user_confirmation",
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
  compare_string_array_rule(
    &output,
    "platforms",
    &case.expected_rules_json,
    "expected_platforms",
    rule_errors,
  );
  compare_string_rule(
    &output,
    "account_source",
    &case.expected_rules_json,
    "expected_account_source",
    rule_errors,
  );
  compare_string_array_rule(
    &output,
    "selected_fields",
    &case.expected_rules_json,
    "expected_selected_fields",
    rule_errors,
  );
  compare_string_array_rule(
    &output,
    "missing_fields",
    &case.expected_rules_json,
    "expected_missing_fields",
    rule_errors,
  );

  let validation = validate_collection_plan_v4(&output);
  if let Some(expected_valid) = case
    .expected_rules_json
    .get("expected_plan_valid")
    .and_then(Value::as_bool)
  {
    if validation.valid != expected_valid {
      rule_errors.push(format!(
        "计划校验结果应为 {expected_valid}，实际为 {}",
        validation.valid
      ));
    }
  }
  for expected_error in case
    .expected_rules_json
    .get("expected_error_contains")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
  {
    if !validation
      .errors
      .iter()
      .any(|error| error.contains(expected_error))
    {
      rule_errors.push(format!("计划校验错误未包含 {expected_error}"));
    }
  }
  if !validation.valid {
    schema_errors.extend(validation.errors);
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
      "{output_field} 应为 {:?}，实际为 {:?}",
      expected, actual
    ));
  }
}

fn compare_string_rule(
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
      "{output_field} 应为 {:?}，实际为 {:?}",
      expected, actual
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
