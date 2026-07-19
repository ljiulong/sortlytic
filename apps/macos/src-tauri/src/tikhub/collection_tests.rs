use super::*;

#[test]
fn maps_every_mvp_platform_and_data_type_to_official_endpoint() {
  let cases = [
    (
      "tiktok",
      "keyword_search",
      RequestMethod::Get,
      "/api/v1/tiktok/app/v3/fetch_video_search_result",
    ),
    (
      "tiktok",
      "comments",
      RequestMethod::Get,
      "/api/v1/tiktok/app/v3/fetch_video_comments",
    ),
    (
      "tiktok",
      "account_profile",
      RequestMethod::Get,
      "/api/v1/tiktok/app/v3/handler_user_profile",
    ),
    (
      "tiktok",
      "account_posts",
      RequestMethod::Get,
      "/api/v1/tiktok/app/v3/fetch_user_post_videos",
    ),
    (
      "tiktok",
      "item_detail",
      RequestMethod::Get,
      "/api/v1/tiktok/app/v3/fetch_one_video",
    ),
    (
      "douyin",
      "keyword_search",
      RequestMethod::Post,
      "/api/v1/douyin/search/fetch_video_search_v2",
    ),
    (
      "douyin",
      "comments",
      RequestMethod::Get,
      "/api/v1/douyin/app/v3/fetch_video_comments",
    ),
    (
      "douyin",
      "account_profile",
      RequestMethod::Get,
      "/api/v1/douyin/app/v3/handler_user_profile",
    ),
    (
      "douyin",
      "account_posts",
      RequestMethod::Get,
      "/api/v1/douyin/app/v3/fetch_user_post_videos",
    ),
    (
      "douyin",
      "item_detail",
      RequestMethod::Get,
      "/api/v1/douyin/app/v3/fetch_one_video",
    ),
    (
      "xiaohongshu",
      "keyword_search",
      RequestMethod::Get,
      "/api/v1/xiaohongshu/app_v2/search_notes",
    ),
    (
      "xiaohongshu",
      "comments",
      RequestMethod::Get,
      "/api/v1/xiaohongshu/app_v2/get_note_comments",
    ),
    (
      "xiaohongshu",
      "account_profile",
      RequestMethod::Get,
      "/api/v1/xiaohongshu/app_v2/get_user_info",
    ),
    (
      "xiaohongshu",
      "account_posts",
      RequestMethod::Get,
      "/api/v1/xiaohongshu/app_v2/get_user_posted_notes",
    ),
    (
      "xiaohongshu",
      "item_detail",
      RequestMethod::Get,
      "/api/v1/xiaohongshu/app_v2/get_image_note_detail",
    ),
  ];

  for (platform, data_type, method, path) in cases {
    let request = build_collection_request(platform, data_type, &params_for(data_type), None)
      .expect("supported request should build");

    assert_eq!(request.method(), method, "{platform}.{data_type}");
    assert_eq!(request.paths().first().map(String::as_str), Some(path));
  }
}

#[test]
fn maps_business_params_to_platform_specific_names_and_pagination() {
  let tiktok_comments = build_collection_request(
    "tiktok",
    "comments",
    &serde_json::json!({ "item_id": "video-1", "region": "US", "time_range": "近 7 天", "page_size": 50 }),
    Some(&cursor_for("tiktok.comments", serde_json::json!(20))),
  )
  .expect("TikTok comments request should build");
  assert!(tiktok_comments
    .query()
    .contains(&("aweme_id".to_string(), "video-1".to_string())));
  assert!(tiktok_comments
    .query()
    .contains(&("cursor".to_string(), "20".to_string())));
  assert!(tiktok_comments
    .query()
    .contains(&("count".to_string(), "50".to_string())));

  let douyin_search = build_collection_request(
    "douyin",
    "keyword_search",
    &serde_json::json!({ "keyword": "汽车", "region": "CN", "time_range": "近 30 天" }),
    Some(&cursor_for(
      "douyin.keyword_search",
      serde_json::json!({ "cursor": 10, "search_id": "search-1", "backtrace": "trace-1" }),
    )),
  )
  .expect("Douyin search request should build");
  let body = douyin_search.body().expect("Douyin search uses JSON body");
  assert_eq!(body["keyword"], "汽车");
  assert_eq!(body["cursor"], 10);
  assert_eq!(body["search_id"], "search-1");
  assert!(
    body.get("publish_time").is_none(),
    "抖音 API 不支持 30 天枚举，应由本地记录时间二次过滤"
  );

  let douyin_half_year = build_collection_request(
    "douyin",
    "keyword_search",
    &serde_json::json!({
      "keyword": "汽车",
      "region": "CN",
      "time_range": "近 180 天"
    }),
    None,
  )
  .expect("Douyin half-year search request should build");
  assert_eq!(
    douyin_half_year
      .body()
      .expect("Douyin search uses JSON body")["publish_time"],
    "180"
  );

  let xhs_profile = build_collection_request(
    "xiaohongshu",
    "account_profile",
    &serde_json::json!({ "account_id": "user-1", "region": "CN" }),
    None,
  )
  .expect("Xiaohongshu profile request should build");
  assert!(xhs_profile
    .query()
    .contains(&("user_id".to_string(), "user-1".to_string())));
}

#[test]
fn maps_and_parses_account_post_pagination_for_all_platforms() {
  for (platform, account_param, cursor_param) in [
    ("tiktok", "sec_user_id", "max_cursor"),
    ("douyin", "sec_user_id", "max_cursor"),
    ("xiaohongshu", "user_id", "cursor"),
  ] {
    let endpoint_key = format!("{platform}.account_posts");
    let request = build_collection_request(
      platform,
      "account_posts",
      &params_for("account_posts"),
      Some(&cursor_for(&endpoint_key, serde_json::json!(20))),
    )
    .expect("账号作品请求应支持续页");

    assert!(request
      .query()
      .contains(&(account_param.to_string(), "account-1".to_string())));
    assert!(request
      .query()
      .contains(&(cursor_param.to_string(), "20".to_string())));

    let page = parse_collection_page(
      &request,
      serde_json::json!({
        "code": 200,
        "data": {
          "aweme_list": [{ "aweme_id": "post-1" }],
          "cursor": 40,
          "has_more": true
        }
      }),
    )
    .expect("账号作品响应应解析为作品记录");

    assert_eq!(page.records.len(), 1);
    assert_eq!(
      page.next_cursor,
      Some(cursor_for(&endpoint_key, serde_json::json!(40)))
    );
  }
}

#[test]
fn parses_records_and_next_cursor_without_treating_wrapper_as_data() {
  let request = build_collection_request("tiktok", "comments", &params_for("comments"), None)
    .expect("request should build");
  let page = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "request_id": "req-1",
      "data": {
        "comments": [
          { "cid": "comment-1", "text": "第一条" },
          { "cid": "comment-2", "text": "第二条" }
        ],
        "cursor": 20,
        "has_more": 1
      }
    }),
  )
  .expect("response should parse");

  assert_eq!(page.records.len(), 2);
  assert_eq!(page.records[0]["cid"], "comment-1");
  assert_eq!(
    page.next_cursor,
    Some(cursor_for("tiktok.comments", serde_json::json!(20)))
  );
  assert!(page.has_more);
}

#[test]
fn prioritizes_tiktok_search_items_over_empty_compatibility_array() {
  let request = build_collection_request(
    "tiktok",
    "keyword_search",
    &params_for("keyword_search"),
    None,
  )
  .expect("TikTok search request should build");
  let page = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": {
        "aweme_list": [],
        "search_item_list": [{
          "aweme_info": {
            "aweme_id": "video-from-search-items",
            "author": { "uid": "author-1" }
          }
        }],
        "cursor": 20,
        "has_more": 0
      }
    }),
  )
  .expect("real TikTok search response should parse");

  assert_eq!(page.records.len(), 1);
  assert_eq!(page.records[0]["aweme_id"], "video-from-search-items");
  assert_eq!(page.records[0]["author"]["uid"], "author-1");
}

#[test]
fn unwraps_xiaohongshu_app_v2_search_envelope() {
  let request = build_collection_request(
    "xiaohongshu",
    "keyword_search",
    &params_for("keyword_search"),
    None,
  )
  .expect("request should build");
  let page = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": {
        "code": 0,
        "data": {
          "items": [{
            "mix_track_id": "track-1",
            "note": {
              "id": "note-1",
              "title": "真实同型笔记",
              "user": {
                "userid": "user-1",
                "red_id": "red-1",
                "nickname": "作者一"
              }
            }
          }]
        },
        "success": true,
        "search_id": "search-1",
        "search_session_id": "session-1",
        "page": 1,
        "next_page": 2
      }
    }),
  )
  .expect("App V2 nested search response should parse");

  assert_eq!(page.records.len(), 1);
  assert_eq!(page.records[0]["id"], "note-1");
  assert_eq!(page.records[0]["user"]["userid"], "user-1");
  assert!(page.has_more);
  assert_eq!(
    page.next_cursor,
    Some(serde_json::json!({
      "endpoint_key": "xiaohongshu.keyword_search",
      "value": {
        "page": 2,
        "search_id": "search-1",
        "search_session_id": "session-1"
      }
    }))
  );
}

#[test]
fn xiaohongshu_detail_has_video_fallback() {
  let request = build_collection_request(
    "xiaohongshu",
    "item_detail",
    &params_for("item_detail"),
    None,
  )
  .expect("request should build");

  assert_eq!(
    request.paths(),
    vec![
      "/api/v1/xiaohongshu/app_v2/get_image_note_detail".to_string(),
      "/api/v1/xiaohongshu/app_v2/get_video_note_detail".to_string(),
    ]
  );
}

#[test]
fn rejects_failed_tikhub_wrapper_with_retry_metadata() {
  let request = build_collection_request("tiktok", "comments", &params_for("comments"), None)
    .expect("request should build");
  let error = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 429,
      "message": "request rate limit reached",
      "data": null
    }),
  )
  .expect_err("business error must not be treated as records");

  assert_eq!(error.code, AppErrorCode::TikhubRateLimit);
  assert!(error.retryable);
  assert!(!error.message.contains("Bearer"));
}

#[test]
fn unwraps_douyin_search_records_and_keeps_continuation_metadata() {
  let request = build_collection_request(
    "douyin",
    "keyword_search",
    &params_for("keyword_search"),
    None,
  )
  .expect("request should build");
  let page = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": {
        "business_data": [
          {
            "type": 1,
            "data": {
              "aweme_info": { "aweme_id": "video-1" }
            }
          }
        ],
        "cursor": 10,
        "search_id": "search-1",
        "backtrace": "trace-1",
        "has_more": true
      }
    }),
  )
  .expect("search response should parse");

  assert_eq!(
    page.records,
    vec![serde_json::json!({ "aweme_id": "video-1" })]
  );
  assert_eq!(
    page.next_cursor,
    Some(serde_json::json!({
      "endpoint_key": "douyin.keyword_search",
      "value": {
        "cursor": 10,
        "search_id": "search-1",
        "backtrace": "trace-1"
      }
    }))
  );
}

#[test]
fn ignores_empty_optional_search_continuation_metadata() {
  let request = build_collection_request(
    "douyin",
    "keyword_search",
    &params_for("keyword_search"),
    None,
  )
  .expect("request should build");
  let page = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": {
        "business_data": [],
        "cursor": 10,
        "search_id": "",
        "backtrace": null,
        "has_more": true
      }
    }),
  )
  .expect("empty optional metadata should normalize to absence");

  assert_eq!(
    page.next_cursor,
    Some(serde_json::json!({
      "endpoint_key": "douyin.keyword_search",
      "value": { "cursor": 10 }
    }))
  );
}

#[test]
fn rejects_missing_required_business_parameter() {
  let error = build_collection_request(
    "xiaohongshu",
    "comments",
    &serde_json::json!({ "region": "CN", "time_range": "近 7 天" }),
    None,
  )
  .expect_err("missing note identifier must be rejected before HTTP");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("item_id"));
}

#[test]
fn rejects_non_integer_page_size_instead_of_silently_using_default() {
  let error = build_collection_request(
    "tiktok",
    "keyword_search",
    &serde_json::json!({
      "keyword": "汽车",
      "region": "US",
      "time_range": "近 7 天",
      "page_size": "50"
    }),
    None,
  )
  .expect_err("invalid page size type must be rejected before HTTP");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("page_size"));
}

#[test]
fn rejects_page_size_for_endpoints_that_do_not_send_it_to_tikhub() {
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
    let error = build_collection_request(platform, data_type, &params, None)
      .expect_err("unsupported page_size must fail before building a request");

    assert!(error.message.contains("page_size") && error.message.contains("白名单"));
  }
}

#[test]
fn rejects_boolean_cursor_instead_of_requesting_cursor_true() {
  let error = build_collection_request(
    "tiktok",
    "comments",
    &params_for("comments"),
    Some(&cursor_for("tiktok.comments", serde_json::json!(true))),
  )
  .expect_err("boolean cursor must not reach provider query parameters");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("游标"));
}

#[test]
fn rejects_scalar_xiaohongshu_comment_cursor() {
  let error = build_collection_request(
    "xiaohongshu",
    "comments",
    &params_for("comments"),
    Some(&cursor_for("xiaohongshu.comments", serde_json::json!(20))),
  )
  .expect_err("Xiaohongshu comments require cursor and index together");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("游标"));
}

#[test]
fn rejects_endpoint_specific_cursor_field_type_mismatches() {
  for cursor in [
    cursor_for(
      "douyin.keyword_search",
      serde_json::json!({ "cursor": "10" }),
    ),
    cursor_for(
      "douyin.keyword_search",
      serde_json::json!({ "cursor": 10, "search_id": 123 }),
    ),
  ] {
    let error = build_collection_request(
      "douyin",
      "keyword_search",
      &params_for("keyword_search"),
      Some(&cursor),
    )
    .expect_err("Douyin cursor and search ID types must match the provider schema");
    assert_eq!(error.code, AppErrorCode::ValidationError);
  }

  let error = build_collection_request(
    "xiaohongshu",
    "comments",
    &params_for("comments"),
    Some(&cursor_for(
      "xiaohongshu.comments",
      serde_json::json!({ "cursor": "cursor-1", "index": 1.5 }),
    )),
  )
  .expect_err("Xiaohongshu comment index must be a non-negative integer");
  assert_eq!(error.code, AppErrorCode::ValidationError);
}

#[test]
fn rejects_cursor_envelope_from_a_different_endpoint() {
  let source_request =
    build_collection_request("tiktok", "comments", &params_for("comments"), None)
      .expect("source request should build");
  let source_page = parse_collection_page(
    &source_request,
    serde_json::json!({
      "code": 200,
      "data": { "comments": [], "cursor": 20, "has_more": true }
    }),
  )
  .expect("source response should parse");
  let cursor = source_page
    .next_cursor
    .as_ref()
    .expect("source page should expose a cursor");

  for (platform, data_type) in [("douyin", "comments"), ("tiktok", "keyword_search")] {
    let error = build_collection_request(platform, data_type, &params_for(data_type), Some(cursor))
      .expect_err("cursor endpoint identity must match the target request");
    assert_eq!(error.code, AppErrorCode::ValidationError);
  }
}

#[test]
fn rejects_search_cursor_object_without_primary_page_or_cursor() {
  let error = build_collection_request(
    "xiaohongshu",
    "keyword_search",
    &params_for("keyword_search"),
    Some(&cursor_for(
      "xiaohongshu.keyword_search",
      serde_json::json!({ "search_id": "search-1" }),
    )),
  )
  .expect_err("search metadata without a page cursor must not restart page one");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("游标"));
}

#[test]
fn keeps_source_constraints_for_local_post_filtering() {
  let request = build_collection_request(
    "douyin",
    "keyword_search",
    &serde_json::json!({
      "keyword": "汽车",
      "region": "CN",
      "time_range": "近 30 天"
    }),
    None,
  )
  .expect("request should build");

  assert_eq!(request.source_params()["region"], "CN");
  assert_eq!(request.source_params()["time_range"], "近 30 天");
  assert!(request.source_params().get("page_size").is_none());
}

#[test]
fn stops_pagination_when_remote_page_number_would_overflow() {
  let request = build_collection_request(
    "xiaohongshu",
    "keyword_search",
    &params_for("keyword_search"),
    Some(&cursor_for(
      "xiaohongshu.keyword_search",
      serde_json::json!(i64::MAX),
    )),
  )
  .expect("request should build");
  let error = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": {
        "items": [],
        "page": i64::MAX,
        "has_more": true
      }
    }),
  )
  .expect_err("overflowing continuation must fail instead of truncating silently");

  assert_eq!(error.code, AppErrorCode::TikhubRequestError);
  assert!(error.message.contains("续页游标"));
}

#[test]
fn keeps_xiaohongshu_comment_cursor_and_index_together() {
  let request = build_collection_request(
    "xiaohongshu",
    "comments",
    &params_for("comments"),
    Some(&cursor_for(
      "xiaohongshu.comments",
      serde_json::json!({ "cursor": "cursor-1", "index": 20 }),
    )),
  )
  .expect("request should build");
  assert!(request
    .query()
    .contains(&("cursor".to_string(), "cursor-1".to_string())));
  assert!(request
    .query()
    .contains(&("index".to_string(), "20".to_string())));

  let page = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": {
        "comments": [],
        "cursor": "cursor-2",
        "index": 40,
        "has_more": true
      }
    }),
  )
  .expect("response should parse");
  assert_eq!(
    page.next_cursor,
    Some(cursor_for(
      "xiaohongshu.comments",
      serde_json::json!({ "cursor": "cursor-2", "index": 40 }),
    ))
  );
}

#[test]
fn rejects_unknown_success_shape_instead_of_reporting_empty_success() {
  let request = build_collection_request(
    "tiktok",
    "keyword_search",
    &params_for("keyword_search"),
    None,
  )
  .expect("request should build");
  let error = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": { "unexpected_records": [] }
    }),
  )
  .expect_err("unknown response shape must not look like a successful empty page");

  assert_eq!(error.code, AppErrorCode::TikhubRequestError);
  assert!(error.message.contains("响应结构"));
}

#[test]
fn rejects_invalid_has_more_and_incomplete_douyin_continuation() {
  let request = build_collection_request(
    "douyin",
    "keyword_search",
    &params_for("keyword_search"),
    None,
  )
  .expect("request should build");
  let invalid_flag = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": { "business_data": [], "has_more": "perhaps" }
    }),
  )
  .expect_err("invalid has_more must be a protocol error");
  assert!(invalid_flag.message.contains("响应结构"));

  let missing_cursor = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": {
        "business_data": [],
        "search_id": "search-1",
        "has_more": true
      }
    }),
  )
  .expect_err("continuation metadata without cursor must fail on this page");
  assert!(missing_cursor.message.contains("续页游标"));
}

#[test]
fn rejects_continuation_that_does_not_advance() {
  let request = build_collection_request(
    "tiktok",
    "comments",
    &params_for("comments"),
    Some(&cursor_for("tiktok.comments", serde_json::json!(20))),
  )
  .expect("request should build");
  let error = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": { "comments": [], "cursor": 20, "has_more": true }
    }),
  )
  .expect_err("unchanged cursor must not request the same page again");

  assert!(error.message.contains("没有前进"));
}

#[test]
fn rejects_semantically_stalled_cursor_across_json_shapes() {
  let request = build_collection_request(
    "tiktok",
    "comments",
    &params_for("comments"),
    Some(&cursor_for(
      "tiktok.comments",
      serde_json::json!({ "cursor": 20 }),
    )),
  )
  .expect("legacy cursor object should normalize");
  let error = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": { "comments": [], "cursor": 20, "has_more": true }
    }),
  )
  .expect_err("object and scalar forms of the same cursor must compare equal");

  assert!(error.message.contains("没有前进"));
}

#[test]
fn rejects_non_boolean_integer_has_more() {
  let request = build_collection_request("tiktok", "comments", &params_for("comments"), None)
    .expect("request should build");
  let error = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": { "comments": [], "cursor": 20, "has_more": 2 }
    }),
  )
  .expect_err("has_more only accepts boolean or 0/1");

  assert!(error.message.contains("has_more"));
}

#[test]
fn rejects_primitive_detail_payload() {
  let request = build_collection_request("tiktok", "item_detail", &params_for("item_detail"), None)
    .expect("request should build");
  let error = parse_collection_page(&request, serde_json::json!({ "code": 200, "data": true }))
    .expect_err("primitive detail payload is not a record object");

  assert!(error.message.contains("响应结构"));
}

#[test]
fn rejects_primitive_elements_in_record_array() {
  let request = build_collection_request("tiktok", "comments", &params_for("comments"), None)
    .expect("request should build");
  let error = parse_collection_page(
    &request,
    serde_json::json!({
      "code": 200,
      "data": { "comments": [true, "not-a-record"], "has_more": false }
    }),
  )
  .expect_err("record arrays may only contain objects");

  assert!(error.message.contains("响应结构"));
}

#[test]
fn rejects_empty_detail_data_so_xiaohongshu_can_try_video_fallback() {
  let request = build_collection_request(
    "xiaohongshu",
    "item_detail",
    &params_for("item_detail"),
    None,
  )
  .expect("request should build");
  let error = parse_collection_page(&request, serde_json::json!({ "code": 200, "data": null }))
    .expect_err("empty image detail must allow the sender to try video detail");

  assert_eq!(error.code, AppErrorCode::TikhubRequestError);
  assert!(error.message.contains("响应结构"));
  assert!(collection::should_try_video_fallback(&request, 0, &error));

  let server_error = parse_collection_page(
    &request,
    serde_json::json!({ "code": 500, "message": "temporary outage", "data": null }),
  )
  .expect_err("server error should fail");
  assert!(
    !collection::should_try_video_fallback(&request, 0, &server_error),
    "网络、服务端和通用业务错误不得产生额外计费请求"
  );
}

fn cursor_for(endpoint_key: &str, value: Value) -> Value {
  serde_json::json!({
    "endpoint_key": endpoint_key,
    "value": value
  })
}

fn params_for(data_type: &str) -> Value {
  match data_type {
    "keyword_search" => serde_json::json!({
      "keyword": "汽车",
      "region": "US",
      "time_range": "近 7 天"
    }),
    "comments" => serde_json::json!({
      "item_id": "item-1",
      "region": "US",
      "time_range": "近 7 天"
    }),
    "account_profile" => serde_json::json!({ "account_id": "account-1", "region": "US" }),
    "account_posts" => serde_json::json!({ "account_id": "account-1" }),
    _ => serde_json::json!({ "item_id": "item-1", "region": "US" }),
  }
}
