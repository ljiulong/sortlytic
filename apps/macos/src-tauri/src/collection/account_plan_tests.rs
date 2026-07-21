use super::*;

fn request(platform: &str, source: &str) -> AccountFormCollectionPlanRequest {
  AccountFormCollectionPlanRequest {
    platform: platform.to_string(),
    account_source: source.to_string(),
    selected_fields: Vec::new(),
    enrichment_policy: "auto_costed".to_string(),
    params: serde_json::json!({ "source_input": "seed" }),
    age_range: None,
    gender_filter: None,
    request_limit: Some(1),
    record_limit: Some(1),
    budget_limit_micros: Some(1_000_000),
  }
}

#[test]
fn materializes_discovery_minimal_enrichment_and_cost_breakdown() {
  let mut request = request("tiktok", "content_search_authors");
  request.selected_fields = ["avatar_url", "country_region", "last_posted_at"]
    .map(ToString::to_string)
    .to_vec();
  request.params = serde_json::json!({ "keyword": "新能源汽车" });
  request.request_limit = Some(2);
  request.record_limit = Some(40);
  let plan = generate_account_collection_plan(request).unwrap();

  assert_eq!(plan.schema_version, 4);
  assert_eq!(
    plan.validation_status, "valid",
    "{:?}",
    plan.validation_errors_json
  );
  assert_eq!(
    plan.plan_json["steps"][0]["operation_key"],
    "discover.content_search_authors"
  );
  assert_eq!(
    plan.plan_json["steps"][1]["operation_key"],
    "enrich.profile"
  );
  assert_eq!(
    plan.plan_json["steps"][2]["operation_key"],
    "enrich.account_country"
  );
  assert_eq!(
    plan.plan_json["steps"][3]["operation_key"],
    "enrich.account_posts"
  );
  assert_eq!(plan.cost_estimate_json["discovery_request_count"], 2);
  assert_eq!(plan.cost_estimate_json["enrichment_request_count"], 120);
  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 122);
}

#[test]
fn direct_account_reuses_the_discovery_profile_response() {
  let mut request = request("douyin", "direct_account");
  request.selected_fields = ["avatar_url", "followers_count", "country_region"]
    .map(ToString::to_string)
    .to_vec();
  let plan = generate_account_collection_plan(request).unwrap();

  assert_eq!(
    plan.validation_status, "valid",
    "{:?}",
    plan.validation_errors_json
  );
  assert_eq!(plan.plan_json["steps"].as_array().unwrap().len(), 1);
  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 1);
}

#[test]
fn rejects_xiaohongshu_region_filter_without_reliable_evidence() {
  let mut plan_request = request("xiaohongshu", "user_search");
  plan_request.params = serde_json::json!({
    "keyword": "宠物园区",
    "region": "CN"
  });
  let error = generate_account_collection_plan(plan_request)
    .expect_err("unsupported region filter must fail before plan creation");

  assert!(error.message.contains("无法可靠筛选地区"));
}

#[test]
fn adds_local_time_evidence_without_sending_an_unsupported_remote_param() {
  let mut plan_request = request("xiaohongshu", "user_search");
  plan_request.params = serde_json::json!({
    "keyword": "宠物园区",
    "time_range": "7"
  });
  let plan = generate_account_collection_plan(plan_request)
    .expect("local time evidence should generate")
    .plan_json;

  assert_eq!(plan["time_range"], "7");
  assert!(plan["selected_fields"]
    .as_array()
    .is_some_and(|fields| fields.contains(&serde_json::json!("last_posted_at"))));
  assert_eq!(
    plan["steps"][0]["params"],
    serde_json::json!({ "keyword": "宠物园区" })
  );
  assert!(plan["steps"].as_array().is_some_and(|steps| steps
    .iter()
    .any(|step| step["operation_key"] == "enrich.account_posts")));
}

#[test]
fn rejects_invalid_or_unsupported_top_level_evidence_filters() {
  let mut plan_request = request("tiktok", "user_search");
  plan_request.params = serde_json::json!({
    "keyword": "pet supplies",
    "region": "GB",
    "time_range": "30"
  });
  let plan = generate_account_collection_plan(plan_request)
    .expect("valid evidence filters should generate")
    .plan_json;

  for (field, invalid, expected_error) in [
    ("region", serde_json::json!("UK"), "大写 ISO 两位代码"),
    (
      "time_range",
      serde_json::json!("近 30 天"),
      "1、7、30 或 180",
    ),
    ("time_range", serde_json::json!(999), "1、7、30 或 180"),
  ] {
    let mut candidate = plan.clone();
    candidate[field] = invalid;
    let validation = validate_collection_plan_v4(&candidate);
    assert!(!validation.valid, "invalid {field} must be rejected");
    assert!(validation
      .errors
      .iter()
      .any(|error| error.contains(expected_error)));
  }

  let mut unsupported = generate_account_collection_plan(request("xiaohongshu", "user_search"))
    .unwrap()
    .plan_json;
  unsupported["region"] = serde_json::json!("CN");
  let validation = validate_collection_plan_v4(&unsupported);
  assert!(!validation.valid);
  assert!(validation
    .errors
    .iter()
    .any(|error| error.contains("无法可靠筛选地区")));
}

#[test]
fn douyin_search_reuses_direct_fields_and_deduplicates_extended_profile() {
  let mut request = request("douyin", "user_search");
  request.params = serde_json::json!({ "keyword": "汽车" });
  request.selected_fields = ["gender", "age", "live_status", "live_level"]
    .map(ToString::to_string)
    .to_vec();
  let plan = generate_account_collection_plan(request).unwrap();

  assert_eq!(plan.plan_json["steps"].as_array().unwrap().len(), 2);
  assert_eq!(plan.plan_json["steps"][0]["params"]["keyword"], "汽车");
  assert!(plan.plan_json["steps"][0]["params"]
    .get("source_input")
    .is_none());
  assert_eq!(
    plan.plan_json["steps"][1]["operation_key"],
    "enrich.extended_demographics"
  );
  assert_eq!(
    plan.plan_json["steps"][1]["input_binding"]["account_id"],
    "secure_user_id"
  );
  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 2);
}

#[test]
fn rejects_unsupported_source_field_and_tampered_operation() {
  assert!(generate_account_collection_plan(request("xiaohongshu", "followers")).is_err());
  let mut unsupported_field = request("tiktok", "user_search");
  unsupported_field.selected_fields = vec!["gender".to_string()];
  assert!(generate_account_collection_plan(unsupported_field).is_err());

  let mut valid_request = request("xiaohongshu", "user_search");
  valid_request.selected_fields = vec!["following_count".to_string()];
  let mut plan = generate_account_collection_plan(valid_request)
    .unwrap()
    .plan_json;
  plan["steps"][1]["operation_key"] = serde_json::json!("enrich.account_country");
  let validation = validate_collection_plan_v4(&plan);
  assert!(!validation.valid);
  assert!(validation
    .errors
    .iter()
    .any(|error| error.contains("未声明操作")));
}

#[test]
fn rejects_tampered_step_roles_keys_bindings_and_output_selection() {
  let mut plan_request = request("douyin", "user_search");
  plan_request.params = serde_json::json!({ "keyword": "汽车" });
  plan_request.selected_fields = vec!["age".to_string()];
  let plan = generate_account_collection_plan(plan_request)
    .expect("plan should generate")
    .plan_json;

  let tampered = [
    (
      "discovery role",
      serde_json::json!("enrichment"),
      "/steps/0/role",
    ),
    (
      "duplicate step key",
      serde_json::json!("discover"),
      "/steps/1/step_key",
    ),
    (
      "enrichment role",
      serde_json::json!("discovery"),
      "/steps/1/role",
    ),
    (
      "dependency",
      serde_json::json!("other"),
      "/steps/1/depends_on_step_key",
    ),
    (
      "input binding",
      serde_json::json!("platform_user_id"),
      "/steps/1/input_binding/account_id",
    ),
    (
      "params binding",
      serde_json::json!("platform_user_id"),
      "/steps/1/params/account_id",
    ),
    (
      "output selection",
      serde_json::json!(false),
      "/steps/1/output_selected",
    ),
  ];

  for (label, value, pointer) in tampered {
    let mut candidate = plan.clone();
    *candidate
      .pointer_mut(pointer)
      .unwrap_or_else(|| panic!("{label} pointer should exist")) = value;
    let validation = validate_collection_plan_v4(&candidate);
    assert!(!validation.valid, "tampered {label} must be rejected");
  }
}

#[test]
fn rejects_tampered_output_rules_cost_breakdown_and_source_capacity() {
  let mut plan_request = request("douyin", "user_search");
  plan_request.params = serde_json::json!({ "keyword": "汽车" });
  plan_request.selected_fields = vec!["age".to_string()];
  let plan = generate_account_collection_plan(plan_request)
    .expect("plan should generate")
    .plan_json;

  let tampered = [
    (
      "output entity",
      serde_json::json!("content"),
      "/output_rules/entity",
    ),
    (
      "required fields",
      serde_json::json!([]),
      "/output_rules/required_fields",
    ),
    (
      "dedupe key",
      serde_json::json!(["platform"]),
      "/output_rules/dedupe_key",
    ),
    (
      "fallback key",
      serde_json::json!(["account_handle"]),
      "/output_rules/fallback_dedupe_key",
    ),
    (
      "unselected label",
      serde_json::json!("未提供"),
      "/output_rules/unselected_value_label",
    ),
    (
      "missing label",
      serde_json::json!("未知"),
      "/output_rules/missing_value_label",
    ),
    (
      "evidence",
      serde_json::json!(false),
      "/output_rules/evidence_required",
    ),
    (
      "discovery cost",
      serde_json::json!(99),
      "/cost_estimate/discovery_request_count",
    ),
    (
      "enrichment cost",
      serde_json::json!(99),
      "/cost_estimate/enrichment_request_count",
    ),
    (
      "operation cost",
      serde_json::json!(99),
      "/cost_estimate/enrichment_operation_count",
    ),
    (
      "cost confirmation",
      serde_json::json!(false),
      "/cost_estimate/requires_confirmation",
    ),
    (
      "source capacity",
      serde_json::json!(10_000),
      "/request_limit",
    ),
  ];

  for (label, value, pointer) in tampered {
    let mut candidate = plan.clone();
    *candidate
      .pointer_mut(pointer)
      .unwrap_or_else(|| panic!("{label} pointer should exist")) = value;
    let validation = validate_collection_plan_v4(&candidate);
    assert!(!validation.valid, "tampered {label} must be rejected");
  }

  let mut inflated_discovery = plan;
  inflated_discovery["request_limit"] = serde_json::json!(2);
  inflated_discovery["steps"][0]["request_limit"] = serde_json::json!(2);
  inflated_discovery["cost_estimate"]["discovery_request_count"] = serde_json::json!(2);
  inflated_discovery["cost_estimate"]["request_count_estimate"] = serde_json::json!(3);
  let validation = validate_collection_plan_v4(&inflated_discovery);
  assert!(
    !validation.valid,
    "discovery requests above the record capacity must be rejected"
  );
}
