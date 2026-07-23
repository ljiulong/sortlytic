use std::collections::BTreeSet;

use rusqlite::Connection;
use serde_json::{json, Value};

use super::*;
use crate::tasks::{
  cancel_task, confirm_collection_plan, create_collection_task, enqueue_task, save_collection_plan,
  CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

#[test]
fn explicit_age_range_is_inclusive_and_unknown_age_is_excluded() {
  let range = AgeRange { min: 0, max: 130 };
  for age in [
    json!({ "user_id": "u-0", "age": 0 }),
    json!({ "user_id": "u-130", "age": 130 }),
    json!({ "user_id": "u-18", "age": "18" }),
  ] {
    let account =
      normalize_account("tiktok", SourceKind::CommentAuthor, &age).expect("明确年龄记录应归一化");
    assert!(range.includes(account.age));
  }
  for value in [
    json!({ "user_id": "u-none" }),
    json!({ "user_id": "u-unknown-sentinel", "age": -1 }),
    json!({ "user_id": "u-format", "age": "18岁" }),
    json!({ "user_id": "u-over", "age": 131 }),
  ] {
    let account = normalize_account("tiktok", SourceKind::CommentAuthor, &value)
      .expect("缺少身份外的字段不应阻止测试夹具归一化");
    assert!(!range.includes(account.age));
  }
  assert!(AgeRange { min: 18, max: 18 }.includes(Some(18)));
}

#[test]
fn duplicate_accounts_merge_before_age_filter_and_keep_high_confidence_fields() {
  let mut accounts = AccountAccumulator::default();
  accounts.upsert(
    normalize_account(
      "xiaohongshu",
      SourceKind::CommentAuthor,
      &json!({ "user_id": "u-1", "nickname": "评论昵称", "fans": 5 }),
    )
    .expect("评论作者应归一化"),
  );
  accounts.upsert(
    normalize_account(
      "xiaohongshu",
      SourceKind::ContentAuthor,
      &json!({ "user_id": "u-1", "nickname": "内容昵称", "age": 25 }),
    )
    .expect("内容作者应归一化"),
  );
  accounts.upsert(
    normalize_account(
      "xiaohongshu",
      SourceKind::AccountProfile,
      &json!({ "user_id": "u-1", "nickname": "公开资料昵称", "fans": 99 }),
    )
    .expect("账号资料应归一化"),
  );

  let output = accounts.into_filtered(Some(AgeRange { min: 18, max: 30 }), 10);
  assert_eq!(output.len(), 1);
  assert_eq!(output[0].username.as_deref(), Some("公开资料昵称"));
  assert_eq!(output[0].followers_count, Some(99));
  assert_eq!(output[0].age, Some(25));
}

#[test]
fn identity_prefers_platform_user_id_then_normalized_account() {
  let stable = normalize_account(
    "douyin",
    SourceKind::AccountProfile,
    &json!({ "sec_user_id": "SEC-1", "unique_id": "@Car Owner" }),
  )
  .expect("稳定 ID 应归一化");
  let fallback = normalize_account(
    "douyin",
    SourceKind::AccountProfile,
    &json!({ "unique_id": " @Car Owner " }),
  )
  .expect("账号名应作为回退身份");

  assert_eq!(stable.identity_key, "id:SEC-1");
  assert_eq!(fallback.identity_key, "account:carowner");
}

#[test]
fn normalizes_xiaohongshu_app_v2_user_identity_fields() {
  let account = normalize_account(
    "xiaohongshu",
    SourceKind::ContentAuthor,
    &json!({
      "id": "note-1",
      "user": {
        "userid": "user-1",
        "red_id": "red-1",
        "nickname": "作者一"
      }
    }),
  )
  .expect("小红书 App V2 的用户字段应归一化");

  assert_eq!(account.identity_key, "id:user-1");
  assert_eq!(account.platform_user_id.as_deref(), Some("user-1"));
  assert_eq!(account.account.as_deref(), Some("red-1"));
  assert_eq!(account.username.as_deref(), Some("作者一"));
}

#[test]
fn explicit_gender_values_are_normalized_without_inference() {
  for (value, expected) in [
    (json!("男"), "male"),
    (json!("female"), "female"),
    (json!("other"), "other"),
  ] {
    let account = normalize_account(
      "tiktok",
      SourceKind::AccountProfile,
      &json!({ "user_id": format!("user-{expected}"), "gender": value }),
    )
    .expect("明确性别应归一化");
    assert_eq!(account.gender.as_deref(), Some(expected));
  }
  let unknown = normalize_account(
    "tiktok",
    SourceKind::AccountProfile,
    &json!({ "user_id": "unknown", "gender": "猜测为女性" }),
  )
  .expect("异常性别不应阻止账号归一化");
  assert_eq!(unknown.gender, None);
  let unknown_zero = normalize_account(
    "douyin",
    SourceKind::AccountProfile,
    &json!({ "user_id": "unknown-zero", "gender": 0 }),
  )
  .expect("性别 0 是平台未知哨兵值");
  assert_eq!(unknown_zero.gender, None);
}

#[test]
fn explicit_zero_statistics_are_preserved() {
  let account = normalize_account(
    "douyin",
    SourceKind::AccountProfile,
    &json!({ "user_id": "zero-stats", "follower_count": 0, "aweme_count": 0 }),
  )
  .expect("统计值 0 是合法业务值");

  assert_eq!(account.followers_count, Some(0));
  assert_eq!(account.posts_count, Some(0));
}

#[test]
fn full_public_account_catalog_records_value_path_endpoint_and_time() {
  let account = normalize_account_with_evidence(
    "douyin",
    SourceKind::FieldEnrichment,
    "douyin.extended_demographics",
    "2026-07-20T08:00:00+08:00",
    &json!({
      "user_id": "user-1",
      "sec_user_id": "secure-1",
      "avatar_url": "https://example.com/avatar.jpg",
      "profile_url": "https://example.com/user-1",
      "bio": "公开简介",
      "website_url": "https://example.com",
      "verified": false,
      "verification_reason": "企业认证",
      "account_type": "enterprise",
      "is_private": false,
      "language": "zh-Hans",
      "country": "CN",
      "profile_tags": ["汽车", "科技"],
      "gender": 1,
      "age": 28,
      "followers_count": 0,
      "following_count": 0,
      "friends_count": 0,
      "posts_count": 0,
      "likes_received_count": 0,
      "liked_content_count": 0,
      "account_created_at": 0,
      "last_posted_at": 0,
      "live_status": 0,
      "live_room_id": "room-1",
      "username_modified_at": 0,
      "nickname_modified_at": 0,
      "commerce_status": false,
      "commerce_category": "auto",
      "seller_status": false,
      "organization_status": false,
      "comments_permission": "everyone",
      "duet_permission": "everyone",
      "stitch_permission": "everyone",
      "download_permission": "everyone",
      "favorites_visibility": "public",
      "following_visibility": "public",
      "playlist_visibility": "public",
      "live_level": 0,
      "live_badge": "车友"
    }),
  )
  .expect("完整公开账号资料应归一化");

  let expected = [
    "secure_user_id",
    "avatar_url",
    "profile_url",
    "bio",
    "website_url",
    "verification_status",
    "verification_reason",
    "account_type",
    "private_account",
    "language",
    "country_region",
    "profile_tags",
    "gender",
    "age",
    "followers_count",
    "following_count",
    "friends_count",
    "posts_count",
    "likes_received_count",
    "liked_content_count",
    "account_created_at",
    "last_posted_at",
    "live_status",
    "live_room_id",
    "username_modified_at",
    "nickname_modified_at",
    "commerce_status",
    "commerce_category",
    "seller_status",
    "organization_status",
    "comments_permission",
    "duet_permission",
    "stitch_permission",
    "download_permission",
    "favorites_visibility",
    "following_visibility",
    "playlist_visibility",
    "live_level",
    "live_badge",
  ];
  let expected = expected.into_iter().collect::<BTreeSet<_>>();
  let actual = account
    .account_fields
    .keys()
    .map(String::as_str)
    .collect::<BTreeSet<_>>();
  assert!(expected.is_subset(&actual));
  assert_eq!(account.account_fields["followers_count"], 0);
  assert_eq!(account.account_fields["live_status"], false);
  assert!(expected.iter().all(|field| {
    let evidence = &account.field_evidence[*field];
    evidence.endpoint_key == "douyin.extended_demographics"
      && evidence.collected_at == "2026-07-20T08:00:00+08:00"
      && evidence.raw_path.starts_with('/')
  }));
}

#[test]
fn higher_priority_zero_value_replaces_older_value_with_matching_evidence() {
  let mut account = normalize_account_with_evidence(
    "tiktok",
    SourceKind::CommentAuthor,
    "tiktok.comments",
    "2026-07-20T08:00:00+08:00",
    &json!({ "user_id": "user-1", "followers_count": 9 }),
  )
  .unwrap();
  account.merge(
    normalize_account_with_evidence(
      "tiktok",
      SourceKind::FieldEnrichment,
      "tiktok.account_profile",
      "2026-07-20T08:01:00+08:00",
      &json!({ "user_id": "user-1", "followers_count": 0 }),
    )
    .unwrap(),
  );

  assert_eq!(account.followers_count, Some(0));
  assert_eq!(account.account_fields["followers_count"], 0);
  assert_eq!(
    account.field_evidence["followers_count"].endpoint_key,
    "tiktok.account_profile"
  );
}

#[test]
fn persisted_gender_filter_runs_after_account_merge() {
  let connection = account_connection();
  set_gender_filter(&connection, &["female"]);
  for (data_type, records) in [
    (
      "comments",
      vec![
        json!({ "user_id": "u-1", "nickname": "待补全" }),
        json!({ "user_id": "u-2", "nickname": "男性", "gender": "男" }),
      ],
    ),
    (
      "account_profile",
      vec![json!({
        "user_id": "u-1",
        "nickname": "女性公开资料",
        "gender": "女"
      })],
    ),
  ] {
    persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-1".to_string(),
        platform: "tiktok".to_string(),
        data_type: data_type.to_string(),
        records,
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: "2026-07-16T08:00:00+00:00".to_string(),
      },
    )
    .expect("性别筛选账号应合并");
  }

  let output = connection
    .query_row(
      "SELECT COUNT(*), username, gender FROM collected_account WHERE output_included = 1",
      [],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .expect("性别筛选结果应查询");
  assert_eq!(output.0, 1);
  assert_eq!(output.1.as_deref(), Some("女性公开资料"));
  assert_eq!(output.2.as_deref(), Some("female"));
}

#[test]
fn stale_worker_fence_rejects_account_mutations() {
  let connection = account_connection();
  let now = chrono::Utc::now();
  connection
    .execute(
      "INSERT INTO task_worker_lease (
         id, owner_id, lease_expires_at, created_at, updated_at, generation
       ) VALUES ('task_worker', 'replacement-owner', ?1, ?2, ?2, 2)",
      rusqlite::params![now.timestamp_millis() + 120_000, now.to_rfc3339()],
    )
    .expect("replacement lease should be installed");
  let stale = crate::tasks::WorkerFence::new("stale-owner".to_string(), 1)
    .expect("stale fence should be valid");

  persist_account_observations_with_fence(
    &connection,
    AccountObservationInput {
      task_run_id: "run-stale".to_string(),
      platform: "tiktok".to_string(),
      data_type: "comments".to_string(),
      records: vec![json!({
        "user_id": "stale-account",
        "nickname": "旧代账号"
      })],
      output_selected: true,
      age_range: None,
      record_limit: 10,
      collected_at: "2026-07-20T08:00:00+08:00".to_string(),
    },
    &stale,
  )
  .expect_err("a stale generation must not merge or output accounts");

  assert_eq!(
    connection
      .query_row("SELECT COUNT(*) FROM collected_account", [], |row| {
        row.get::<_, i64>(0)
      })
      .expect("account count should query"),
    0
  );
}

#[test]
fn cancelled_run_rejects_account_mutations_with_a_current_worker_fence() {
  let root = std::env::temp_dir().join(format!("account-cancel-fence-{}", uuid::Uuid::new_v4()));
  create_workspace("账号取消栅栏测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "取消后账号不得落库".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should create");
  let draft = crate::collection::generate_form_collection_plan(
    crate::collection::FormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      data_type: None,
      data_types: vec!["comments".to_string()],
      params: json!({ "item_id": "video-cancelled" }),
      age_range: None,
      request_limit: Some(1),
      record_limit: Some(1),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("plan should generate");
  let plan = save_collection_plan(
    &root,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: draft.validation_status,
      validation_errors_json: Some(draft.validation_errors_json),
      cost_estimate_json: Some(draft.cost_estimate_json),
    },
  )
  .expect("plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should confirm");
  let run = enqueue_task(&root, &task.id).expect("task should enqueue");
  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  connection
    .execute(
      "UPDATE collection_task SET status = 'running' WHERE id = ?1",
      [&task.id],
    )
    .expect("task should enter the running fixture state");
  connection
    .execute(
      "UPDATE task_run SET status = 'running' WHERE id = ?1",
      [&run.id],
    )
    .expect("run should enter the running fixture state");
  let now = chrono::Utc::now();
  connection
    .execute(
      "INSERT INTO task_worker_lease (
         id, owner_id, lease_expires_at, created_at, updated_at, generation
       ) VALUES ('task_worker', 'current-owner', ?1, ?2, ?2, 1)",
      rusqlite::params![now.timestamp_millis() + 120_000, now.to_rfc3339()],
    )
    .expect("current lease should install");
  let current = crate::tasks::WorkerFence::new("current-owner".to_string(), 1)
    .expect("current fence should construct");

  cancel_task(&root, &task.id).expect("running task should cancel");
  let error = persist_account_observations_with_fence(
    &connection,
    AccountObservationInput {
      task_run_id: run.id.clone(),
      platform: "tiktok".to_string(),
      data_type: "comments".to_string(),
      records: vec![json!({
        "user_id": "cancelled-account",
        "nickname": "取消后账号"
      })],
      output_selected: true,
      age_range: None,
      record_limit: 1,
      collected_at: "2026-07-24T00:00:00+08:00".to_string(),
    },
    &current,
  )
  .expect_err("a cancelled run must reject account persistence even with a current fence");
  assert!(
    error.message.contains("运行中的任务"),
    "cancellation rejection should explain the running-state invariant"
  );
  assert_eq!(
    connection
      .query_row(
        "SELECT COUNT(*) FROM collected_account WHERE task_run_id = ?1",
        [&run.id],
        |row| row.get::<_, i64>(0),
      )
      .expect("account count should query"),
    0
  );

  drop(connection);
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn persisted_region_and_time_filters_require_explicit_merged_evidence() {
  for (days, cutoff) in [
    (1, "2026-07-19T00:00:00Z"),
    (7, "2026-07-13T00:00:00Z"),
    (30, "2026-06-20T00:00:00Z"),
    (180, "2026-01-21T00:00:00Z"),
  ] {
    let connection = account_connection();
    set_account_filters(&connection, "GB", days);
    let observed_at = "2026-07-20T00:00:00Z";

    let discovered = persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-1".to_string(),
        platform: "tiktok".to_string(),
        data_type: "comments".to_string(),
        records: vec![json!({ "user_id": "qualified", "unique_id": "qualified" })],
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: observed_at.to_string(),
      },
    )
    .expect("候选账号应先留存");
    assert_eq!(discovered.output_count, 0, "缺少地区和时间证据不得输出");

    let region_enriched = persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-1".to_string(),
        platform: "tiktok".to_string(),
        data_type: "account_country".to_string(),
        records: vec![json!({
          "user_id": "qualified",
          "unique_id": "qualified",
          "country_region": "gb"
        })],
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: observed_at.to_string(),
      },
    )
    .expect("明确地区应合并");
    assert_eq!(region_enriched.output_count, 0, "缺少时间证据仍不得输出");

    let time_enriched = persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-1".to_string(),
        platform: "tiktok".to_string(),
        data_type: "account_posts".to_string(),
        records: vec![json!({
          "user_id": "qualified",
          "unique_id": "qualified",
          "last_posted_at": cutoff
        })],
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: observed_at.to_string(),
      },
    )
    .expect("精确边界时间应合并");
    assert_eq!(time_enriched.output_count, 1, "{days} 天边界应包含在结果内");

    persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-1".to_string(),
        platform: "tiktok".to_string(),
        data_type: "account_profile".to_string(),
        records: vec![
          json!({ "user_id": "wrong-region", "country_region": "US", "last_posted_at": observed_at }),
          json!({ "user_id": "missing-region", "last_posted_at": observed_at }),
          json!({ "user_id": "invalid-region", "country_region": "United Kingdom", "last_posted_at": observed_at }),
          json!({ "user_id": "future-post", "country_region": "GB", "last_posted_at": "2026-07-21T00:00:00Z" }),
          json!({ "user_id": "invalid-post", "country_region": "GB", "last_posted_at": "recently" })
        ],
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: observed_at.to_string(),
      },
    )
    .expect("不合格证据仍应留存用于审计");

    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM collected_account WHERE output_included = 1",
          [],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      1,
      "未知、异常、未来或不匹配证据不得通过"
    );
  }
}

#[test]
fn persisted_accounts_merge_before_age_filter_and_apply_output_limit() {
  let connection = account_connection();
  let first = persist_account_observations(
    &connection,
    AccountObservationInput {
      task_run_id: "run-1".to_string(),
      platform: "tiktok".to_string(),
      data_type: "comments".to_string(),
      records: vec![json!({
        "user_id": "u-1",
        "unique_id": "car-owner",
        "nickname": "评论昵称"
      })],
      output_selected: true,
      age_range: Some(AgeRange { min: 18, max: 30 }),
      record_limit: 1,
      collected_at: "2026-07-16T08:00:00+00:00".to_string(),
    },
  )
  .expect("未知年龄账号应先合并但不输出");
  assert_eq!(first.output_count, 0);

  let enriched = persist_account_observations(
    &connection,
    AccountObservationInput {
      task_run_id: "run-1".to_string(),
      platform: "tiktok".to_string(),
      data_type: "account_profile".to_string(),
      records: vec![json!({
        "user_id": "u-1",
        "unique_id": "car-owner",
        "nickname": "公开资料昵称",
        "age": "25"
      })],
      output_selected: true,
      age_range: Some(AgeRange { min: 18, max: 30 }),
      record_limit: 1,
      collected_at: "2026-07-16T08:01:00+00:00".to_string(),
    },
  )
  .expect("明确年龄应使合并账号进入输出");
  assert_eq!(enriched.output_count, 1);

  persist_account_observations(
    &connection,
    AccountObservationInput {
      task_run_id: "run-1".to_string(),
      platform: "tiktok".to_string(),
      data_type: "comments".to_string(),
      records: vec![json!({ "user_id": "u-2", "nickname": "第二个", "age": 20 })],
      output_selected: true,
      age_range: Some(AgeRange { min: 18, max: 30 }),
      record_limit: 1,
      collected_at: "2026-07-16T08:02:00+00:00".to_string(),
    },
  )
  .expect("达到硬上限后仍可留存证据");

  let rows = connection
    .prepare(
      "SELECT identity_key, username, age, output_included
       FROM collected_account ORDER BY identity_key",
    )
    .expect("账号查询应准备")
    .query_map([], |row| {
      Ok((
        row.get::<_, String>(0)?,
        row.get::<_, Option<String>>(1)?,
        row.get::<_, Option<i64>>(2)?,
        row.get::<_, i64>(3)?,
      ))
    })
    .expect("账号应查询")
    .collect::<Result<Vec<_>, _>>()
    .expect("账号行应解析");
  assert_eq!(rows.len(), 2);
  assert_eq!(rows[0].1.as_deref(), Some("公开资料昵称"));
  assert_eq!(rows[0].2, Some(25));
  assert_eq!(rows.iter().filter(|row| row.3 == 1).count(), 1);
}

#[test]
fn persisted_stable_id_consolidates_a_fallback_account_identity() {
  let connection = account_connection();
  for (data_type, record) in [
    (
      "comments",
      json!({ "unique_id": " @Car Owner ", "nickname": "评论昵称", "age": 22 }),
    ),
    (
      "account_profile",
      json!({
        "sec_user_id": "SEC-1",
        "unique_id": "carowner",
        "nickname": "公开资料昵称",
        "age": 22
      }),
    ),
  ] {
    persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-1".to_string(),
        platform: "douyin".to_string(),
        data_type: data_type.to_string(),
        records: vec![record],
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: "2026-07-16T08:00:00+00:00".to_string(),
      },
    )
    .expect("账号观测应持久化");
  }

  let account = connection
    .query_row(
      "SELECT COUNT(*), identity_key, username FROM collected_account",
      [],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .expect("合并账号应查询");
  assert_eq!(account.0, 1);
  assert_eq!(account.1, "id:SEC-1");
  assert_eq!(account.2.as_deref(), Some("公开资料昵称"));
}

#[test]
fn persisted_account_fields_keep_zero_and_latest_field_evidence() {
  let connection = account_connection();
  for (data_type, follower_count, collected_at) in [
    ("comments", 9, "2026-07-20T08:00:00+08:00"),
    ("account_profile", 0, "2026-07-20T08:01:00+08:00"),
  ] {
    persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-evidence".to_string(),
        platform: "tiktok".to_string(),
        data_type: data_type.to_string(),
        records: vec![json!({
          "user_id": "user-evidence",
          "unique_id": "account-evidence",
          "nickname": "证据账号",
          "follower_count": follower_count
        })],
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: collected_at.to_string(),
      },
    )
    .unwrap();
  }

  let (fields, evidence, data_source) = connection
    .query_row(
      "SELECT account_fields_json, field_evidence_json, data_source
       FROM collected_account WHERE task_run_id = 'run-evidence'",
      [],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
        ))
      },
    )
    .unwrap();
  let fields: Value = serde_json::from_str(&fields).unwrap();
  let evidence: Value = serde_json::from_str(&evidence).unwrap();
  assert_eq!(fields["followers_count"], 0);
  assert_eq!(fields["platform"], "tiktok");
  assert_eq!(fields["display_name"], "证据账号");
  assert_eq!(fields["account_handle"], "account-evidence");
  assert_eq!(fields["platform_user_id"], "user-evidence");
  assert_eq!(fields["data_source"], "tiktok.account_profile");
  assert_eq!(evidence["platform_user_id"]["raw_path"], "/user_id");
  assert_eq!(evidence["display_name"]["raw_path"], "/nickname");
  assert_eq!(evidence["account_handle"]["raw_path"], "/unique_id");
  assert_eq!(
    evidence["data_source"]["endpoint_key"],
    "tiktok.account_profile"
  );
  assert!(data_source.contains("tiktok.account_profile"));
  assert!(data_source.contains("tiktok.comments"));
  assert_eq!(
    evidence["followers_count"]["endpoint_key"],
    "tiktok.account_profile"
  );
  assert_eq!(
    evidence["followers_count"]["collected_at"],
    "2026-07-20T08:01:00+08:00"
  );
}

#[test]
fn account_posts_only_contribute_the_latest_post_time() {
  let connection = account_connection();
  for (data_type, record, collected_at) in [
    (
      "account_profile",
      json!({
        "user_id": "user-posts",
        "nickname": "可靠资料昵称",
        "signature": "可靠资料简介",
        "create_time": 100
      }),
      "2026-07-20T08:00:00+08:00",
    ),
    (
      "account_posts",
      json!({
        "aweme_id": "post-old",
        "create_time": 200,
        "author": {
          "user_id": "user-posts",
          "nickname": "作品快照昵称",
          "signature": "作品快照简介"
        }
      }),
      "2026-07-20T08:01:00+08:00",
    ),
    (
      "account_posts",
      json!({
        "aweme_id": "post-new",
        "create_time": 300,
        "author": {
          "user_id": "user-posts",
          "nickname": "另一作品快照昵称",
          "signature": "另一作品快照简介"
        }
      }),
      "2026-07-20T08:02:00+08:00",
    ),
  ] {
    persist_account_observations(
      &connection,
      AccountObservationInput {
        task_run_id: "run-account-posts".to_string(),
        platform: "douyin".to_string(),
        data_type: data_type.to_string(),
        records: vec![record],
        output_selected: true,
        age_range: None,
        record_limit: 10,
        collected_at: collected_at.to_string(),
      },
    )
    .unwrap();
  }

  let (username, profile_text, last_posted_at, fields, evidence) = connection
    .query_row(
      "SELECT username, profile_text, last_posted_at, account_fields_json, field_evidence_json
       FROM collected_account WHERE task_run_id = 'run-account-posts'",
      [],
      |row| {
        Ok((
          row.get::<_, Option<String>>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, Option<String>>(2)?,
          row.get::<_, String>(3)?,
          row.get::<_, String>(4)?,
        ))
      },
    )
    .unwrap();
  let fields: Value = serde_json::from_str(&fields).unwrap();
  let evidence: Value = serde_json::from_str(&evidence).unwrap();

  assert_eq!(username.as_deref(), Some("可靠资料昵称"));
  assert_eq!(profile_text.as_deref(), Some("可靠资料简介"));
  assert_eq!(last_posted_at.as_deref(), Some("300"));
  assert_eq!(fields["account_created_at"], 100);
  assert_eq!(fields["last_posted_at"], 300);
  assert_eq!(
    evidence["account_created_at"]["endpoint_key"],
    "douyin.account_profile"
  );
  assert_eq!(
    evidence["last_posted_at"]["endpoint_key"],
    "douyin.account_posts"
  );
  assert_eq!(evidence["last_posted_at"]["raw_path"], "/create_time");
}

#[test]
fn persisted_user_search_is_a_supported_account_observation() {
  let connection = account_connection();
  let result = persist_account_observations(
    &connection,
    AccountObservationInput {
      task_run_id: "run-user-search".to_string(),
      platform: "douyin".to_string(),
      data_type: "user_search".to_string(),
      records: vec![json!({ "uid": "user-search-1", "gender": 0 })],
      output_selected: true,
      age_range: None,
      record_limit: 10,
      collected_at: "2026-07-20T08:00:00+08:00".to_string(),
    },
  )
  .unwrap();

  assert_eq!(result.observed_count, 1);
  assert_eq!(result.output_count, 1);
  assert_eq!(
    connection
      .query_row(
        "SELECT gender FROM collected_account WHERE task_run_id = 'run-user-search'",
        [],
        |row| row.get::<_, Option<String>>(0),
      )
      .unwrap(),
    None
  );
}

fn account_connection() -> Connection {
  let connection = Connection::open_in_memory().expect("内存数据库应创建");
  connection
    .execute_batch(
      "CREATE TABLE collected_account (
        id TEXT PRIMARY KEY,
        task_run_id TEXT NOT NULL,
        platform TEXT NOT NULL,
        identity_key TEXT NOT NULL,
        username TEXT,
        account TEXT,
        platform_user_id TEXT,
        profile_text TEXT,
        country_region TEXT,
        region_source TEXT,
        region_confidence TEXT,
        gender TEXT,
        age INTEGER,
        followers_count INTEGER,
        posts_count INTEGER,
        last_posted_at TEXT,
        profile_url TEXT,
        data_source TEXT NOT NULL,
        collected_at TEXT NOT NULL,
        notes TEXT,
        merged_record_json TEXT NOT NULL,
        source_priority_json TEXT NOT NULL,
        account_fields_json TEXT NOT NULL DEFAULT '{}',
        field_evidence_json TEXT NOT NULL DEFAULT '{}',
        output_included INTEGER NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE (task_run_id, platform, identity_key)
      );
      CREATE TABLE collection_plan (id TEXT PRIMARY KEY, plan_json TEXT NOT NULL);
      CREATE TABLE task_run (id TEXT PRIMARY KEY, plan_id TEXT);
      CREATE TABLE task_worker_lease (
        id TEXT PRIMARY KEY,
        owner_id TEXT NOT NULL,
        lease_expires_at INTEGER NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        generation INTEGER NOT NULL
      );",
    )
    .expect("账号表应创建");
  connection
}

fn set_gender_filter(connection: &Connection, genders: &[&str]) {
  connection
    .execute(
      "INSERT INTO collection_plan (id, plan_json) VALUES ('plan-1', ?1)",
      [serde_json::json!({ "gender_filter": genders }).to_string()],
    )
    .expect("性别计划应插入");
  connection
    .execute(
      "INSERT INTO task_run (id, plan_id) VALUES ('run-1', 'plan-1')",
      [],
    )
    .expect("性别运行应插入");
}

fn set_account_filters(connection: &Connection, region: &str, time_range_days: i64) {
  connection
    .execute(
      "INSERT INTO collection_plan (id, plan_json) VALUES ('plan-1', ?1)",
      [serde_json::json!({
        "schema_version": 4,
        "region": region,
        "time_range": time_range_days.to_string()
      })
      .to_string()],
    )
    .expect("地区和时间计划应插入");
  connection
    .execute(
      "INSERT INTO task_run (id, plan_id) VALUES ('run-1', 'plan-1')",
      [],
    )
    .expect("地区和时间运行应插入");
}
