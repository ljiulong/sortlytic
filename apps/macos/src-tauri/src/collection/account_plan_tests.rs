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
  assert_eq!(plan.plan_json["steps"][1]["operation_key"], "enrich.profile");
  assert_eq!(
    plan.plan_json["steps"][2]["operation_key"],
    "enrich.account_country"
  );
  assert_eq!(
    plan.plan_json["steps"][3]["operation_key"],
    "enrich.account_posts"
  );
  assert_eq!(plan.cost_estimate_json["discovery_request_count"], 1);
  assert_eq!(plan.cost_estimate_json["enrichment_request_count"], 120);
  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 121);
}

#[test]
fn direct_account_reuses_the_discovery_profile_response() {
  let mut request = request("douyin", "direct_account");
  request.selected_fields = ["avatar_url", "followers_count"]
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
    ("discovery role", serde_json::json!("enrichment"), "/steps/0/role"),
    ("duplicate step key", serde_json::json!("discover"), "/steps/1/step_key"),
    ("enrichment role", serde_json::json!("discovery"), "/steps/1/role"),
    ("dependency", serde_json::json!("other"), "/steps/1/depends_on_step_key"),
    ("input binding", serde_json::json!("platform_user_id"), "/steps/1/input_binding/account_id"),
    ("params binding", serde_json::json!("platform_user_id"), "/steps/1/params/account_id"),
    ("output selection", serde_json::json!(false), "/steps/1/output_selected"),
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
