use serde_json::Value;

use crate::accounts::{normalize_account_with_evidence, FieldEvidence, SourceKind};

use super::{normalize_timestamp, NormalizedInput};

pub(super) fn is_supported_account_data_type(platform: &str, data_type: &str) -> bool {
  let Ok(capability) = crate::collection::get_account_collection_capabilities(platform) else {
    return false;
  };
  capability.account_sources.iter().any(|source| {
    source
      .endpoint_key
      .rsplit_once('.')
      .is_some_and(|(_, value)| value == data_type)
  }) || capability.fields.iter().any(|field| {
    field
      .required_operation_keys
      .iter()
      .any(|operation| operation_data_type(operation).is_some_and(|value| value == data_type))
  })
}

pub(super) fn normalize_account_fields(input: &NormalizedInput, raw: &Value) -> (Value, Value) {
  let source_kind = match input.data_type.as_str() {
    "comments" => SourceKind::CommentAuthor,
    "keyword_search" | "item_detail" => SourceKind::ContentAuthor,
    "user_search" => SourceKind::UserSearch,
    "followers" | "followings" | "similar_accounts" => SourceKind::Relationship,
    "account_profile" => SourceKind::AccountProfile,
    "account_posts" | "extended_demographics" | "account_country" => SourceKind::FieldEnrichment,
    _ => return (serde_json::json!({}), serde_json::json!({})),
  };
  let (account_payload, prefix) = account_payload(&input.data_type, raw);
  let endpoint_key = format!("{}.{}", input.platform, input.data_type);
  let Ok(mut account) = normalize_account_with_evidence(
    &input.platform,
    source_kind,
    &endpoint_key,
    &input.collected_at,
    account_payload,
  ) else {
    return (serde_json::json!({}), serde_json::json!({}));
  };
  if !prefix.is_empty() {
    for evidence in account.field_evidence.values_mut() {
      evidence.raw_path = format!("{prefix}{}", evidence.raw_path);
    }
  }
  for (field, value, paths) in [
    (
      "platform_user_id",
      account.platform_user_id.clone(),
      &[
        "/platform_user_id",
        "/user_id",
        "/userid",
        "/uid",
        "/sec_user_id",
        "/sec_uid",
      ][..],
    ),
    (
      "display_name",
      account.username.clone(),
      &["/nickname", "/display_name", "/name"][..],
    ),
    (
      "account_handle",
      account.account.clone(),
      &["/unique_id", "/account", "/username", "/red_id"][..],
    ),
  ] {
    let Some(value) = value else {
      continue;
    };
    account
      .account_fields
      .insert(field.to_string(), Value::String(value));
    if let Some(raw_path) = paths
      .iter()
      .find(|path| account_payload.pointer(path).is_some())
    {
      account.field_evidence.insert(
        field.to_string(),
        FieldEvidence {
          endpoint_key: endpoint_key.clone(),
          raw_path: format!("{prefix}{raw_path}"),
          collected_at: input.collected_at.clone(),
        },
      );
    }
  }
  if input.data_type == "account_posts" {
    if let Some((posted_at, raw_path)) = first_value_with_path(
      raw,
      &[
        "/create_time",
        "/create_timestamp",
        "/publish_time",
        "/published_at",
      ],
    )
    .and_then(|(value, path)| normalize_timestamp(value).map(|value| (value, path)))
    {
      account
        .account_fields
        .insert("last_posted_at".to_string(), Value::String(posted_at));
      account.field_evidence.insert(
        "last_posted_at".to_string(),
        FieldEvidence {
          endpoint_key,
          raw_path: raw_path.to_string(),
          collected_at: input.collected_at.clone(),
        },
      );
    }
  }
  (
    serde_json::to_value(account.account_fields).unwrap_or_else(|_| serde_json::json!({})),
    serde_json::to_value(account.field_evidence).unwrap_or_else(|_| serde_json::json!({})),
  )
}

fn operation_data_type(operation: &str) -> Option<&'static str> {
  match operation {
    "enrich.profile" => Some("account_profile"),
    "enrich.extended_demographics" => Some("extended_demographics"),
    "enrich.account_country" => Some("account_country"),
    "enrich.account_posts" => Some("account_posts"),
    _ => None,
  }
}

fn account_payload<'a>(data_type: &str, raw: &'a Value) -> (&'a Value, &'static str) {
  if matches!(data_type, "extended_demographics" | "account_country") {
    return (raw, "");
  }
  for pointer in [
    "/author",
    "/user",
    "/user_info",
    "/note/user",
    "/note_card/user",
    "/aweme_detail/author",
    "/aweme/author",
  ] {
    if let Some(value) = raw.pointer(pointer).filter(|value| value.is_object()) {
      return (value, pointer);
    }
  }
  (raw, "")
}

fn first_value_with_path<'a>(
  value: &'a Value,
  paths: &[&'static str],
) -> Option<(&'a Value, &'static str)> {
  paths
    .iter()
    .find_map(|path| value.pointer(path).map(|value| (value, *path)))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::records::prepare_record;

  fn input(platform: &str, data_type: &str) -> NormalizedInput {
    NormalizedInput {
      task_id: "task-1".to_string(),
      task_run_id: "run-1".to_string(),
      platform: platform.to_string(),
      data_type: data_type.to_string(),
      collected_at: "2026-07-20T08:00:00+08:00".to_string(),
      records: Vec::new(),
    }
  }

  #[test]
  fn normalized_observation_keeps_account_values_and_evidence() {
    let prepared = prepare_record(
      &input("douyin", "user_search"),
      &serde_json::json!({
        "uid": "user-1",
        "nickname": "用户一",
        "unique_id": "account-1",
        "follower_count": 0,
        "gender": 0,
        "age": -1,
        "live_status": 0
      }),
    )
    .unwrap();

    assert_eq!(
      prepared.normalized.account_fields_json["followers_count"],
      0
    );
    assert_eq!(
      prepared.normalized.account_fields_json["platform_user_id"],
      "user-1"
    );
    assert_eq!(
      prepared.normalized.account_fields_json["display_name"],
      "用户一"
    );
    assert_eq!(
      prepared.normalized.account_fields_json["account_handle"],
      "account-1"
    );
    assert_eq!(
      prepared.normalized.account_fields_json["live_status"],
      false
    );
    assert!(prepared
      .normalized
      .account_fields_json
      .get("gender")
      .is_none());
    assert!(prepared.normalized.account_fields_json.get("age").is_none());
    assert_eq!(
      prepared.normalized.field_evidence_json["followers_count"]["endpoint_key"],
      "douyin.user_search"
    );
    assert_eq!(
      prepared.normalized.field_evidence_json["followers_count"]["raw_path"],
      "/follower_count"
    );
    assert_eq!(
      prepared.normalized.field_evidence_json["account_handle"]["raw_path"],
      "/unique_id"
    );
  }

  #[test]
  fn account_posts_record_exposes_last_posted_at_with_raw_path() {
    let prepared = prepare_record(
      &input("tiktok", "account_posts"),
      &serde_json::json!({
        "aweme_id": "post-1",
        "create_time": 1_700_000_000,
        "author": { "uid": "user-1" }
      }),
    )
    .unwrap();

    assert!(prepared.normalized.account_fields_json["last_posted_at"]
      .as_str()
      .is_some_and(|value| value.starts_with("2023-")));
    assert_eq!(
      prepared.normalized.field_evidence_json["last_posted_at"]["raw_path"],
      "/create_time"
    );
  }

  #[test]
  fn capability_catalog_recognizes_all_v4_internal_data_types() {
    for (platform, data_types) in [
      (
        "tiktok",
        &[
          "user_search",
          "followers",
          "followings",
          "similar_accounts",
          "account_country",
        ][..],
      ),
      (
        "douyin",
        &[
          "user_search",
          "followers",
          "followings",
          "extended_demographics",
        ][..],
      ),
      ("xiaohongshu", &["user_search"][..]),
    ] {
      assert!(data_types
        .iter()
        .all(|data_type| is_supported_account_data_type(platform, data_type)));
    }
  }
}
