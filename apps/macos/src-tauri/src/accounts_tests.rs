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
