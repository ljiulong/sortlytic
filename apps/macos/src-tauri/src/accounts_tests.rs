use rusqlite::Connection;
use serde_json::json;

use super::*;

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
    (json!(0), "other"),
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
        output_included INTEGER NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE (task_run_id, platform, identity_key)
      );
      CREATE TABLE collection_plan (id TEXT PRIMARY KEY, plan_json TEXT NOT NULL);
      CREATE TABLE task_run (id TEXT PRIMARY KEY, plan_id TEXT);",
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
