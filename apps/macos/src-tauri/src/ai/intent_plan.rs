use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::CollectionIntentV1;
use crate::collection::{
  generate_account_collection_plan, get_account_collection_capabilities, AccountFieldAvailability,
  AccountFormCollectionPlanRequest, AccountSourceInputKind, AgeRangeInput, CollectionPlanDraftView,
  FilterExecution, PaginationMode,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IntentPlanBuildResult {
  pub intent: CollectionIntentV1,
  pub missing_fields: Vec<String>,
  pub issues: Vec<String>,
  pub validation_status: String,
  pub collection_plan: Option<CollectionPlanDraftView>,
}

pub(crate) fn build_collection_plan_from_intent(
  mut intent: CollectionIntentV1,
) -> IntentPlanBuildResult {
  let missing_fields = recompute_missing_fields(&intent);
  intent.missing_fields.clone_from(&missing_fields);
  if !missing_fields.is_empty() {
    return needs_review(intent, missing_fields, Vec::new());
  }

  let platform = intent.platform.as_deref().expect("缺失字段已拦截");
  let account_source = intent.account_source.as_deref().expect("缺失字段已拦截");
  let source_input = intent.source_input.as_deref().expect("缺失字段已拦截");
  let region_code = intent.region_code.as_deref().expect("缺失字段已拦截");
  let capability = match get_account_collection_capabilities(platform) {
    Ok(capability) => capability,
    Err(error) => return needs_review(intent, missing_fields, vec![error.message]),
  };
  let Some(source) = capability
    .account_sources
    .iter()
    .find(|source| source.key == account_source)
  else {
    return needs_review(
      intent,
      missing_fields,
      vec!["当前平台不支持所选账号来源，请更换来源后重新生成计划".to_string()],
    );
  };
  let mut issues = Vec::new();
  if source.input_kind == AccountSourceInputKind::Keyword
    && intent
      .query_locale
      .as_deref()
      .and_then(|locale| locale.rsplit_once('-'))
      .is_none_or(|(_, locale_region)| locale_region != region_code)
  {
    issues.push("检索语言地区必须与目标地区一致，请修改目标语言或地区".to_string());
  }
  if source.region_filter == FilterExecution::Unsupported {
    issues.push("当前平台或账号来源无法可靠筛选地区，请移除地区条件或更换来源".to_string());
  }
  if let Some(days) = intent.time_range_days {
    if source.time_range_filter == FilterExecution::Unsupported {
      issues.push("当前平台或账号来源无法可靠筛选时间，请移除时间条件或更换来源".to_string());
    } else if !source.time_ranges.contains(&days.to_string()) {
      issues.push(format!(
        "当前账号来源不支持 {days} 天时间范围，请选择来源支持的范围"
      ));
    }
  }
  let mut selected_fields = intent.selected_fields.clone();
  if !selected_fields
    .iter()
    .any(|field| field == "country_region")
  {
    selected_fields.push("country_region".to_string());
  }
  if intent.time_range_days.is_some()
    && !selected_fields
      .iter()
      .any(|field| field == "last_posted_at")
  {
    selected_fields.push("last_posted_at".to_string());
  }
  if intent.age_range.is_some() && !selected_fields.iter().any(|field| field == "age") {
    selected_fields.push("age".to_string());
  }
  if intent
    .gender_filter
    .as_ref()
    .is_some_and(|values| !values.is_empty())
    && !selected_fields.iter().any(|field| field == "gender")
  {
    selected_fields.push("gender".to_string());
  }
  selected_fields = dedupe(selected_fields);
  for field_key in &selected_fields {
    if let Some(field) = capability
      .fields
      .iter()
      .find(|field| field.key == *field_key)
    {
      if field.availability == AccountFieldAvailability::Unsupported {
        issues.push(format!(
          "当前平台不支持字段 {}：{}",
          field.display_name,
          field
            .missing_reason
            .as_deref()
            .unwrap_or("没有可靠证据来源")
        ));
      }
    }
  }
  let record_limit = intent.record_limit.expect("缺失字段已拦截");
  let request_limit = request_limit_for(source.pagination_mode, source.max_page_size, record_limit);
  let maximum_records = source
    .max_page_size
    .saturating_mul(source.max_request_count);
  if source.pagination_mode == PaginationMode::Single && record_limit != 1 {
    issues.push("单账号或单作品来源的最大记录数必须为 1".to_string());
  } else if record_limit > maximum_records {
    issues.push(format!(
      "最大记录数超过当前来源可安全执行的 {maximum_records} 条上限"
    ));
  }
  issues.sort();
  issues.dedup();
  if !issues.is_empty() {
    intent.selected_fields = selected_fields;
    return needs_review(intent, missing_fields, issues);
  }

  let plan = generate_account_collection_plan(AccountFormCollectionPlanRequest {
    platform: platform.to_string(),
    account_source: account_source.to_string(),
    selected_fields: selected_fields.clone(),
    enrichment_policy: "auto_costed".to_string(),
    params: json!({
      "source_input": source_input,
      "region": region_code,
      "time_range": intent.time_range_days.map(|days| days.to_string())
    }),
    age_range: intent.age_range.as_ref().map(|range| AgeRangeInput {
      min: range.min,
      max: range.max,
    }),
    gender_filter: intent.gender_filter.clone(),
    request_limit: Some(request_limit),
    record_limit: Some(record_limit),
    budget_limit_micros: intent.budget_limit_micros,
  });
  intent.selected_fields = selected_fields;
  match plan {
    Ok(plan) if plan.validation_status == "valid" => IntentPlanBuildResult {
      intent,
      missing_fields,
      issues: Vec::new(),
      validation_status: "valid".to_string(),
      collection_plan: Some(plan),
    },
    Ok(plan) => {
      let issues = plan
        .validation_errors_json
        .as_array()
        .map(|values| {
          values
            .iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect()
        })
        .unwrap_or_else(|| vec!["后端生成的计划需要修正".to_string()]);
      needs_review(intent, missing_fields, issues)
    }
    Err(error) => needs_review(intent, missing_fields, vec![error.message]),
  }
}

fn recompute_missing_fields(intent: &CollectionIntentV1) -> Vec<String> {
  let mut missing = BTreeSet::new();
  if intent.platform.is_none() {
    missing.insert("platform".to_string());
  }
  if intent.account_source.is_none() {
    missing.insert("account_source".to_string());
  }
  if intent
    .source_input
    .as_deref()
    .is_none_or(|value| value.trim().is_empty())
  {
    missing.insert("source_input".to_string());
  }
  if intent.region_code.is_none() {
    missing.insert("region_code".to_string());
  }
  let needs_query_locale = intent
    .account_source
    .as_deref()
    .is_none_or(|source| matches!(source, "user_search" | "content_search_authors"));
  if needs_query_locale && intent.query_locale.is_none() {
    missing.insert("query_locale".to_string());
  }
  if intent.record_limit.is_none() {
    missing.insert("record_limit".to_string());
  }
  if intent.budget_limit_micros.is_none() {
    missing.insert("budget_limit_micros".to_string());
  }
  missing.into_iter().collect()
}

fn request_limit_for(mode: PaginationMode, page_size: i64, record_limit: i64) -> i64 {
  match mode {
    PaginationMode::Single => 1,
    PaginationMode::Cursor => record_limit
      .saturating_add(page_size - 1)
      .saturating_div(page_size)
      .max(1),
  }
}

fn dedupe(values: Vec<String>) -> Vec<String> {
  let mut seen = BTreeSet::new();
  values
    .into_iter()
    .filter(|value| seen.insert(value.clone()))
    .collect()
}

fn needs_review(
  mut intent: CollectionIntentV1,
  missing_fields: Vec<String>,
  issues: Vec<String>,
) -> IntentPlanBuildResult {
  intent.missing_fields.clone_from(&missing_fields);
  IntentPlanBuildResult {
    intent,
    missing_fields,
    issues,
    validation_status: "needs_review".to_string(),
    collection_plan: None,
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;

  use super::build_collection_plan_from_intent;
  use crate::ai::{CollectionIntentV1, IntentAgeRange};

  fn british_tiktok_intent() -> CollectionIntentV1 {
    CollectionIntentV1 {
      schema_version: 1,
      platform: Some("tiktok".to_string()),
      account_source: Some("user_search".to_string()),
      source_input: Some("pet supplies".to_string()),
      query_locale: Some("en-GB".to_string()),
      region_code: Some("GB".to_string()),
      selected_fields: vec!["bio".to_string(), "followers_count".to_string()],
      time_range_days: None,
      age_range: None,
      gender_filter: None,
      record_limit: Some(10),
      budget_limit_micros: Some(100_000),
      missing_fields: vec!["platform".to_string()],
      confidence: 0.95,
    }
  }

  #[test]
  fn builds_a_whitelisted_plan_and_adds_region_evidence() {
    let result = build_collection_plan_from_intent(british_tiktok_intent());
    let plan = result.collection_plan.expect("完整意图必须生成确定性计划");

    assert_eq!(result.validation_status, "valid");
    assert!(result.missing_fields.is_empty());
    assert!(result.issues.is_empty());
    assert_eq!(result.intent.missing_fields, Vec::<String>::new());
    assert_eq!(plan.plan_json["platforms"], json!(["tiktok"]));
    assert_eq!(plan.plan_json["account_source"], "user_search");
    assert_eq!(plan.plan_json["region"], "GB");
    assert_eq!(
      plan.plan_json["steps"][0]["endpoint_key"],
      "tiktok.user_search"
    );
    assert_eq!(
      plan.plan_json["steps"][0]["params"]["keyword"],
      "pet supplies"
    );
    assert!(plan.plan_json["steps"][0]["params"].get("region").is_none());
    assert!(plan.plan_json["selected_fields"]
      .as_array()
      .is_some_and(|fields| fields.contains(&json!("country_region"))));
  }

  #[test]
  fn recomputes_missing_fields_instead_of_trusting_the_model() {
    let mut intent = british_tiktok_intent();
    intent.platform = None;
    intent.account_source = None;
    intent.source_input = None;
    intent.query_locale = None;
    intent.region_code = None;
    intent.budget_limit_micros = None;
    intent.missing_fields = vec![];

    let result = build_collection_plan_from_intent(intent);

    assert_eq!(result.validation_status, "needs_review");
    assert!(result.collection_plan.is_none());
    for field in [
      "platform",
      "account_source",
      "source_input",
      "query_locale",
      "region_code",
      "budget_limit_micros",
    ] {
      assert!(result.missing_fields.contains(&field.to_string()));
      assert!(result.intent.missing_fields.contains(&field.to_string()));
    }
  }

  #[test]
  fn blocks_a_region_filter_when_the_source_has_no_reliable_evidence() {
    let mut intent = british_tiktok_intent();
    intent.platform = Some("xiaohongshu".to_string());
    intent.query_locale = Some("en-GB".to_string());

    let result = build_collection_plan_from_intent(intent);

    assert_eq!(result.validation_status, "needs_review");
    assert!(result.collection_plan.is_none());
    assert!(result
      .issues
      .iter()
      .any(|issue| issue.contains("无法可靠筛选地区")));
  }

  #[test]
  fn preserves_direct_account_input_without_translation() {
    let source_input = "https://www.tiktok.com/@PetBrandUK";
    let mut intent = british_tiktok_intent();
    intent.account_source = Some("direct_account".to_string());
    intent.source_input = Some(source_input.to_string());
    intent.query_locale = None;
    intent.record_limit = Some(1);

    let result = build_collection_plan_from_intent(intent);
    let plan = result.collection_plan.expect("直接账号应生成计划");

    assert_eq!(result.intent.source_input.as_deref(), Some(source_input));
    assert_eq!(
      plan.plan_json["steps"][0]["params"]["account_id"],
      "PetBrandUK"
    );
  }

  #[test]
  fn adds_time_age_and_gender_evidence_before_building_the_plan() {
    let mut intent = british_tiktok_intent();
    intent.platform = Some("douyin".to_string());
    intent.query_locale = Some("zh-CN".to_string());
    intent.region_code = Some("CN".to_string());
    intent.source_input = Some("宠物用品".to_string());
    intent.time_range_days = Some(7);
    intent.age_range = Some(IntentAgeRange { min: 21, max: 45 });
    intent.gender_filter = Some(vec!["female".to_string()]);

    let result = build_collection_plan_from_intent(intent);
    let plan = result.collection_plan.expect("证据字段可用时必须生成计划");
    let fields = plan.plan_json["selected_fields"]
      .as_array()
      .expect("结果字段");

    for field in ["country_region", "last_posted_at", "age", "gender"] {
      assert!(fields.contains(&json!(field)), "缺少证据字段 {field}");
    }
    assert_eq!(plan.plan_json["time_range"], "7");
    assert_eq!(plan.plan_json["age_range"], json!({ "min": 21, "max": 45 }));
    assert_eq!(plan.plan_json["gender_filter"], json!(["female"]));
  }

  #[test]
  fn does_not_add_unrequested_profile_fields_or_hidden_cost() {
    let mut intent = british_tiktok_intent();
    intent.selected_fields = Vec::new();

    let result = build_collection_plan_from_intent(intent);
    let plan = result.collection_plan.expect("地区证据可用时必须生成计划");
    let fields = plan.plan_json["selected_fields"]
      .as_array()
      .expect("结果字段");

    assert_eq!(fields, &vec![json!("country_region")]);
    assert!(!fields.contains(&json!("avatar_url")));
    assert!(!fields.contains(&json!("followers_count")));
    assert_eq!(
      plan.plan_json["cost_estimate"]["enrichment_operation_count"],
      1
    );
  }
}
