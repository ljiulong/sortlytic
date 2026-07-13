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
      "supports_region": true,
      "max_page_size": 100,
      "max_request_count": 200
    })
  );

  let error = list_platform_data_types("youtube")
    .expect_err("unsupported platform should keep the original error");
  assert_eq!(error.message, "MVP 只支持 TikTok、抖音、小红书");
}

#[test]
fn complete_comment_plan_passes_authoritative_validation() {
  let result = validate_collection_plan(&complete_comment_plan());

  assert!(result.valid, "unexpected errors: {:?}", result.errors);
  assert!(result.errors.is_empty());
}

#[test]
fn invalid_plan_reports_unverified_region_and_missing_execution_params() {
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
  assert!(result
    .errors
    .iter()
    .any(|error| error.contains("time_range")));
}

#[test]
fn form_plan_status_uses_authoritative_whole_plan_validation() {
  let plan = generate_form_collection_plan(FormCollectionPlanRequest {
    platform: "xiaohongshu".to_string(),
    data_type: "comments".to_string(),
    params: serde_json::json!({
      "item_id": "note-1",
      "region": "CN"
    }),
    request_limit: Some(1),
  })
  .expect("plan should generate for user correction");

  assert_eq!(plan.validation_status, "needs_review");
  assert!(plan
    .validation_errors_json
    .as_array()
    .is_some_and(|errors| errors
      .iter()
      .filter_map(Value::as_str)
      .any(|error| error.contains("time_range"))));
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
