use super::*;

#[test]
fn capability_contract_preserves_order_serialization_and_errors() {
  let platforms = list_supported_platforms();
  assert_eq!(
    platforms
      .iter()
      .map(|platform| (platform.platform.as_str(), platform.display_name.as_str()))
      .collect::<Vec<_>>(),
    vec![
      ("tiktok", "TikTok"),
      ("douyin", "抖音"),
      ("xiaohongshu", "小红书")
    ]
  );

  for platform in &platforms {
    assert_eq!(
      platform.data_types,
      [
        "keyword_search",
        "comments",
        "account_profile",
        "account_posts",
        "item_detail"
      ]
    );
  }

  let comments = list_platform_data_types(" tiktok ")
    .expect("trimmed supported platform should keep working")
    .remove(1);
  assert_eq!(
    serde_json::to_value(comments).expect("capability should serialize"),
    serde_json::json!({
      "platform": "tiktok",
      "data_type": "comments",
      "display_name": "评论采集",
      "endpoint_key": "tiktok.comments",
      "required_params": ["item_id"],
      "optional_params": ["region", "time_range", "page_size"],
      "pagination_mode": "cursor",
      "region_filter": "local",
      "time_range_filter": "local",
      "max_page_size": 100,
      "max_request_count": 200
    })
  );

  let serialized = serde_json::to_value(
    list_platform_data_types("tiktok").expect("platform capabilities should load"),
  )
  .expect("capabilities should serialize");
  assert!(serialized
    .as_array()
    .expect("capabilities should be an array")
    .iter()
    .all(|capability| capability.get("supports_region").is_none()));

  let error = list_platform_data_types("youtube")
    .expect_err("unsupported platform should keep the original error");
  assert_eq!(error.message, "MVP 只支持 TikTok、抖音、小红书");
}

#[test]
fn filter_and_pagination_capabilities_match_execution_paths() {
  let expected = [
    (
      "tiktok",
      "keyword_search",
      PaginationMode::Cursor,
      FilterExecution::Provider,
      FilterExecution::Provider,
    ),
    (
      "tiktok",
      "comments",
      PaginationMode::Cursor,
      FilterExecution::Local,
      FilterExecution::Local,
    ),
    (
      "douyin",
      "keyword_search",
      PaginationMode::Cursor,
      FilterExecution::Local,
      FilterExecution::Provider,
    ),
    (
      "douyin",
      "comments",
      PaginationMode::Cursor,
      FilterExecution::Local,
      FilterExecution::Local,
    ),
    (
      "xiaohongshu",
      "keyword_search",
      PaginationMode::Cursor,
      FilterExecution::Local,
      FilterExecution::Provider,
    ),
    (
      "xiaohongshu",
      "comments",
      PaginationMode::Cursor,
      FilterExecution::Local,
      FilterExecution::Local,
    ),
  ];

  for (platform, data_type, pagination, region_filter, time_range_filter) in expected {
    let capability = list_platform_data_types(platform)
      .expect("supported platform should expose capabilities")
      .into_iter()
      .find(|capability| capability.data_type == data_type)
      .expect("endpoint capability should be registered");

    assert_eq!(capability.pagination_mode, pagination);
    assert_eq!(capability.region_filter, region_filter);
    assert_eq!(capability.time_range_filter, time_range_filter);
  }
}

#[test]
fn single_endpoints_expose_a_single_request_contract() {
  for platform in ["tiktok", "douyin", "xiaohongshu"] {
    let capabilities =
      list_platform_data_types(platform).expect("supported platform should expose capabilities");

    for data_type in ["account_profile", "item_detail"] {
      let capability = capabilities
        .iter()
        .find(|capability| capability.data_type == data_type)
        .expect("single endpoint should be registered");
      let serialized =
        serde_json::to_value(capability).expect("single capability should serialize");

      assert_eq!(serialized["pagination_mode"], "single");
      assert_eq!(serialized["region_filter"], "unsupported");
      assert_eq!(serialized["time_range_filter"], "unsupported");
      assert_eq!(serialized["max_request_count"], 1);
      assert!(capability.optional_params.is_empty());
    }
  }
}

#[test]
fn v1_can_read_legacy_single_endpoint_region_without_advertising_it() {
  let result = validate_collection_params(
    "tiktok",
    "account_profile",
    serde_json::json!({ "account_id": "account-1", "region": "US" }),
  )
  .expect("legacy v1 params should remain readable");

  assert!(result.valid, "unexpected errors: {:?}", result.errors);
}

#[test]
fn page_size_must_be_a_positive_bounded_integer() {
  for invalid_page_size in [
    serde_json::json!("20"),
    serde_json::json!(20.5),
    serde_json::json!(0),
    serde_json::json!(-1),
    serde_json::json!(51),
  ] {
    let result = validate_collection_params(
      "tiktok",
      "keyword_search",
      serde_json::json!({
        "keyword": "car",
        "page_size": invalid_page_size
      }),
    )
    .expect("parameter validation should run");

    assert!(
      !result.valid,
      "invalid page_size should be rejected: {:?}",
      result.normalized_params["page_size"]
    );
    assert!(result
      .errors
      .iter()
      .any(|error| error.contains("page_size")));
  }
}

#[test]
fn endpoints_without_provider_page_size_reject_the_parameter() {
  for (platform, data_type, params) in [
    (
      "douyin",
      "keyword_search",
      serde_json::json!({ "keyword": "汽车", "page_size": 20 }),
    ),
    (
      "xiaohongshu",
      "keyword_search",
      serde_json::json!({ "keyword": "汽车", "page_size": 20 }),
    ),
    (
      "xiaohongshu",
      "comments",
      serde_json::json!({ "item_id": "note-1", "page_size": 20 }),
    ),
  ] {
    let capability = list_platform_data_types(platform)
      .expect("supported platform should expose capabilities")
      .into_iter()
      .find(|capability| capability.data_type == data_type)
      .expect("endpoint capability should be registered");
    let result = validate_collection_params(platform, data_type, params)
      .expect("parameter validation should run");

    assert!(!capability
      .optional_params
      .iter()
      .any(|param| param == "page_size"));
    assert!(
      !result.valid,
      "{platform}.{data_type} must reject page_size"
    );
    assert!(result
      .errors
      .iter()
      .any(|error| error.contains("page_size") && error.contains("白名单")));
  }
}

#[test]
fn complete_tiktok_keyword_plan_passes_v2_validation() {
  let result = validate_collection_plan_v2(&complete_tiktok_keyword_plan_v2());

  assert!(result.valid, "unexpected errors: {:?}", result.errors);
  assert!(result.errors.is_empty());
}

#[test]
fn v2_requires_integer_record_request_and_budget_limits() {
  for (field, expected_error) in [
    ("record_limit", "record_limit"),
    ("budget_limit", "budget_limit"),
  ] {
    let mut plan = complete_tiktok_keyword_plan_v2();
    plan
      .as_object_mut()
      .expect("plan should be an object")
      .remove(field);

    let result = validate_collection_plan_v2(&plan);

    assert!(!result.valid, "missing {field} should be rejected");
    assert!(result
      .errors
      .iter()
      .any(|error| error.contains(expected_error)));
  }

  let cases = [
    ("record_limit", Value::Null, "record_limit"),
    ("record_limit", serde_json::json!(1.5), "record_limit"),
    ("record_limit", serde_json::json!(0), "record_limit"),
    ("request_limit", serde_json::json!(1.5), "request_limit"),
    ("budget_limit", Value::Null, "budget_limit"),
    (
      "budget_limit",
      serde_json::json!({ "currency": "CNY", "amount_micros": 1 }),
      "currency",
    ),
    (
      "budget_limit",
      serde_json::json!({ "currency": "USD", "amount_micros": 1.5 }),
      "amount_micros",
    ),
    (
      "budget_limit",
      serde_json::json!({ "currency": "USD", "amount_micros": 0 }),
      "amount_micros",
    ),
  ];

  for (field, value, expected_error) in cases {
    let mut plan = complete_tiktok_keyword_plan_v2();
    plan[field] = value;

    let result = validate_collection_plan_v2(&plan);

    assert!(!result.valid, "{field} should be rejected");
    assert!(
      result
        .errors
        .iter()
        .any(|error| error.contains(expected_error)),
      "missing {expected_error} error: {:?}",
      result.errors
    );
  }
}

#[test]
fn single_endpoint_rejects_more_than_one_request() {
  let mut plan = complete_account_profile_plan_v2();
  plan["request_limit"] = serde_json::json!(2);

  let result = validate_collection_plan_v2(&plan);

  assert!(!result.valid);
  assert!(result
    .errors
    .iter()
    .any(|error| error.contains("single") && error.contains("request_limit")));
}

#[test]
fn local_filters_fail_closed_until_a_production_filter_exists() {
  let mut plan = complete_comment_plan();
  plan["record_limit"] = serde_json::json!(1200);
  plan["budget_limit"] = serde_json::json!({ "currency": "USD", "amount_micros": 35_000_000 });

  let result = validate_collection_plan_v2(&plan);

  assert!(!result.valid);
  assert!(result
    .errors
    .iter()
    .any(|error| error.contains("region") && error.contains("本地过滤器尚未接通")));
  assert!(result
    .errors
    .iter()
    .any(|error| error.contains("time_range") && error.contains("本地过滤器尚未接通")));
}

#[test]
fn omitted_local_filters_do_not_block_an_otherwise_valid_plan() {
  let mut plan = complete_comment_plan();
  plan["region"] = Value::Null;
  plan["time_range"] = Value::Null;
  plan["record_limit"] = serde_json::json!(1200);
  plan["budget_limit"] = serde_json::json!({ "currency": "USD", "amount_micros": 35_000_000 });
  plan["steps"][0]["params"] = serde_json::json!({ "item_id": "note-1" });

  let result = validate_collection_plan_v2(&plan);

  assert!(result.valid, "{:?}", result.errors);
}

#[test]
fn unsupported_filter_constraints_are_rejected() {
  let mut plan = complete_account_profile_plan_v2();
  plan["region"] = serde_json::json!("US");

  let top_level_result = validate_collection_plan_v2(&plan);

  assert!(!top_level_result.valid);
  assert!(top_level_result
    .errors
    .iter()
    .any(|error| error.contains("region") && error.contains("不支持")));

  plan["region"] = Value::Null;
  plan["steps"][0]["params"]["region"] = serde_json::json!("US");
  let step_result = validate_collection_plan_v2(&plan);

  assert!(!step_result.valid);
  assert!(step_result
    .errors
    .iter()
    .any(|error| error.contains("region") && error.contains("不支持")));
}

#[test]
fn provider_time_ranges_match_the_current_adapter_mapping() {
  let mut plan = complete_tiktok_keyword_plan_v2();
  plan["steps"][0]["params"]["time_range"] = serde_json::json!("近 30 天");
  assert!(validate_collection_plan_v2(&plan).valid);

  plan["steps"][0]["params"]["time_range"] = serde_json::json!("近 90 天");
  let invalid = validate_collection_plan_v2(&plan);
  assert!(!invalid.valid);
  assert!(invalid
    .errors
    .iter()
    .any(|error| error.contains("time_range") && error.contains("1/7/30/180")));

  for platform in ["douyin", "xiaohongshu"] {
    let mut plan = complete_tiktok_keyword_plan_v2();
    plan["platforms"] = serde_json::json!([platform]);
    plan["steps"][0]["platform"] = serde_json::json!(platform);
    plan["steps"][0]["endpoint_key"] = serde_json::json!(format!("{platform}.keyword_search"));

    let result = validate_collection_plan_v2(&plan);

    assert!(!result.valid);
    assert!(
      result
        .errors
        .iter()
        .any(|error| error.contains("time_range") && error.contains("1/7/180")),
      "{platform} should reject the unsupported 30-day provider mapping: {:?}",
      result.errors
    );
  }
}

#[test]
fn complete_comment_plan_passes_authoritative_validation() {
  let result = validate_collection_plan(&complete_comment_plan());

  assert!(result.valid, "unexpected errors: {:?}", result.errors);
  assert!(result.errors.is_empty());
}

#[test]
fn invalid_plan_reports_unverified_region_and_missing_required_target() {
  let result = validate_collection_plan(&serde_json::json!({
    "platforms": ["xiaohongshu"],
    "data_types": ["comments"],
    "region": {
      "value": "CN",
      "source": "natural_language",
      "validation_status": "unverified"
    },
    "time_range": null,
    "steps": [{
      "endpoint_key": "xiaohongshu.comments",
      "platform": "xiaohongshu",
      "data_type": "comments",
      "params": { "region": "CN" }
    }],
    "request_limit": 1,
    "missing_fields": [],
    "requires_user_confirmation": true
  }));

  assert!(!result.valid);
  assert!(result
    .errors
    .iter()
    .any(|error| error.contains("region 尚未验证")));
  assert!(result.errors.iter().any(|error| error.contains("item_id")));
}

#[test]
fn v3_form_plan_accepts_declared_local_region_filter() {
  let plan = generate_form_collection_plan(FormCollectionPlanRequest {
    platform: "xiaohongshu".to_string(),
    data_type: Some("comments".to_string()),
    data_types: Vec::new(),
    params: serde_json::json!({
      "item_id": "note-1",
      "region": "CN"
    }),
    age_range: None,
    request_limit: Some(1),
    record_limit: None,
    budget_limit_micros: None,
  })
  .expect("plan should generate for user correction");

  assert_eq!(
    plan.validation_status, "valid",
    "{:?}",
    plan.validation_errors_json
  );
  assert_eq!(plan.plan_json["steps"][0]["params"]["item_id"], "note-1");
  assert!(plan.plan_json["steps"][0]["depends_on_step_key"].is_null());
}

fn complete_comment_plan() -> Value {
  serde_json::json!({
    "platforms": ["xiaohongshu"],
    "data_types": ["comments"],
    "region": "CN",
    "time_range": "2026-07-01/2026-07-07",
    "steps": [{
      "endpoint_key": "xiaohongshu.comments",
      "platform": "xiaohongshu",
      "data_type": "comments",
      "params": {
        "item_id": "note-1",
        "region": "CN",
        "time_range": "2026-07-01/2026-07-07"
      }
    }],
    "request_limit": 1,
    "missing_fields": [],
    "requires_user_confirmation": true
  })
}

fn complete_tiktok_keyword_plan_v2() -> Value {
  serde_json::json!({
    "platforms": ["tiktok"],
    "data_types": ["keyword_search"],
    "region": "US",
    "time_range": "近 30 天",
    "steps": [{
      "endpoint_key": "tiktok.keyword_search",
      "platform": "tiktok",
      "data_type": "keyword_search",
      "params": {
        "keyword": "car",
        "region": "US",
        "time_range": "近 30 天",
        "page_size": 50
      }
    }],
    "record_limit": 1200,
    "request_limit": 24,
    "budget_limit": {
      "currency": "USD",
      "amount_micros": 35_000_000
    },
    "missing_fields": [],
    "requires_user_confirmation": true
  })
}

fn complete_account_profile_plan_v2() -> Value {
  serde_json::json!({
    "platforms": ["tiktok"],
    "data_types": ["account_profile"],
    "region": null,
    "time_range": null,
    "steps": [{
      "endpoint_key": "tiktok.account_profile",
      "platform": "tiktok",
      "data_type": "account_profile",
      "params": {
        "account_id": "account-1"
      }
    }],
    "record_limit": 1,
    "request_limit": 1,
    "budget_limit": {
      "currency": "USD",
      "amount_micros": 1
    },
    "missing_fields": [],
    "requires_user_confirmation": true
  })
}
