use serde_json::{Map, Value};

use super::TikHubCollectionRequest;
use crate::domain::{AppError, AppErrorStage, AppResult};

pub(super) fn normalize_input_cursor(
  platform: &str,
  data_type: &str,
  cursor: Option<&Value>,
) -> AppResult<Option<Value>> {
  let Some(cursor) = cursor.filter(|value| !value.is_null()) else {
    return Ok(None);
  };
  let expected_endpoint = endpoint_key(platform, data_type);
  let envelope = cursor.as_object();
  let endpoint_matches = envelope
    .and_then(|value| value.get("endpoint_key"))
    .and_then(Value::as_str)
    == expected_endpoint;
  let value = envelope.and_then(|value| value.get("value"));
  if !endpoint_matches {
    return Err(cursor_validation_error());
  }
  canonical_cursor_value(platform, data_type, value.unwrap_or(&Value::Null))
    .map(Some)
    .ok_or_else(cursor_validation_error)
}

pub(super) fn normalize_provider_cursor(
  platform: &str,
  data_type: &str,
  cursor: &Value,
) -> AppResult<Value> {
  let endpoint_key = endpoint_key(platform, data_type).ok_or_else(cursor_validation_error)?;
  let value =
    canonical_cursor_value(platform, data_type, cursor).ok_or_else(cursor_validation_error)?;
  Ok(serde_json::json!({
    "endpoint_key": endpoint_key,
    "value": value
  }))
}

fn canonical_cursor_value(platform: &str, data_type: &str, cursor: &Value) -> Option<Value> {
  match (platform.trim(), data_type.trim()) {
    ("tiktok", "keyword_search" | "user_search") => canonical_scalar(cursor, &["offset", "cursor"]),
    ("tiktok", "followers" | "followings") => canonical_tiktok_relation(cursor),
    ("tiktok", "similar_accounts") => canonical_text_cursor(cursor, &["page_token"]),
    ("tiktok" | "douyin", "account_posts") => canonical_scalar(cursor, &["max_cursor", "cursor"]),
    ("tiktok" | "douyin", "comments") => canonical_scalar(cursor, &["cursor"]),
    ("douyin", "keyword_search" | "user_search") => canonical_douyin_search(cursor),
    ("douyin", "followers" | "followings") => canonical_douyin_relation(cursor),
    ("xiaohongshu", "keyword_search" | "user_search") => canonical_xiaohongshu_search(cursor),
    ("xiaohongshu", "comments") => canonical_xiaohongshu_comments(cursor),
    ("xiaohongshu", "account_posts") => canonical_text_or_integer(cursor, &["cursor"]),
    _ => None,
  }
}

fn endpoint_key(platform: &str, data_type: &str) -> Option<&'static str> {
  match (platform.trim(), data_type.trim()) {
    ("tiktok", "keyword_search") => Some("tiktok.keyword_search"),
    ("tiktok", "user_search") => Some("tiktok.user_search"),
    ("tiktok", "followers") => Some("tiktok.followers"),
    ("tiktok", "followings") => Some("tiktok.followings"),
    ("tiktok", "similar_accounts") => Some("tiktok.similar_accounts"),
    ("tiktok", "comments") => Some("tiktok.comments"),
    ("tiktok", "account_posts") => Some("tiktok.account_posts"),
    ("douyin", "keyword_search") => Some("douyin.keyword_search"),
    ("douyin", "user_search") => Some("douyin.user_search"),
    ("douyin", "followers") => Some("douyin.followers"),
    ("douyin", "followings") => Some("douyin.followings"),
    ("douyin", "comments") => Some("douyin.comments"),
    ("douyin", "account_posts") => Some("douyin.account_posts"),
    ("xiaohongshu", "keyword_search") => Some("xiaohongshu.keyword_search"),
    ("xiaohongshu", "user_search") => Some("xiaohongshu.user_search"),
    ("xiaohongshu", "comments") => Some("xiaohongshu.comments"),
    ("xiaohongshu", "account_posts") => Some("xiaohongshu.account_posts"),
    _ => None,
  }
}

fn cursor_validation_error() -> AppError {
  AppError::validation(
    "续页游标的 endpoint 身份、类型或字段不匹配",
    AppErrorStage::Collection,
  )
}

fn canonical_scalar(cursor: &Value, object_keys: &[&str]) -> Option<Value> {
  let number = match cursor {
    Value::Number(_) => nonnegative_integer(cursor)?,
    Value::Object(object)
      if object.len() == 1 && object.keys().all(|key| object_keys.contains(&key.as_str())) =>
    {
      object_keys
        .iter()
        .find_map(|key| object.get(*key))
        .and_then(nonnegative_integer)?
    }
    _ => return None,
  };
  Some(Value::from(number))
}

fn canonical_text_or_integer(cursor: &Value, object_keys: &[&str]) -> Option<Value> {
  let cursor = match cursor {
    Value::Object(object)
      if object.len() == 1 && object.keys().all(|key| object_keys.contains(&key.as_str())) =>
    {
      object_keys.iter().find_map(|key| object.get(*key))?
    }
    value => value,
  };
  if let Some(number) = nonnegative_integer(cursor) {
    return Some(Value::from(number));
  }
  nonempty_string(cursor).map(Value::String)
}

fn canonical_text_cursor(cursor: &Value, object_keys: &[&str]) -> Option<Value> {
  let cursor = match cursor {
    Value::Object(object)
      if object.len() == 1 && object.keys().all(|key| object_keys.contains(&key.as_str())) =>
    {
      object_keys.iter().find_map(|key| object.get(*key))?
    }
    value => value,
  };
  nonempty_string(cursor).map(Value::String)
}

fn canonical_tiktok_relation(cursor: &Value) -> Option<Value> {
  let object = cursor.as_object()?;
  if !has_only_keys(object, &["min_time", "page_token"]) {
    return None;
  }
  let mut normalized = Map::from_iter([(
    "min_time".to_string(),
    Value::from(nonnegative_integer(object.get("min_time")?)?),
  )]);
  copy_optional_string(object, "page_token", &mut normalized)?;
  Some(Value::Object(normalized))
}

fn canonical_douyin_relation(cursor: &Value) -> Option<Value> {
  let object = cursor.as_object()?;
  if object.len() != 1 || !has_only_keys(object, &["max_time"]) {
    return None;
  }
  let max_time = object.get("max_time")?;
  let max_time = nonempty_string(max_time)
    .or_else(|| nonnegative_integer(max_time).map(|value| value.to_string()))?;
  Some(serde_json::json!({ "max_time": max_time }))
}

fn canonical_douyin_search(cursor: &Value) -> Option<Value> {
  let object = cursor.as_object()?;
  if !has_only_keys(object, &["cursor", "search_id", "backtrace"]) {
    return None;
  }
  let mut normalized = Map::new();
  normalized.insert(
    "cursor".to_string(),
    Value::from(nonnegative_integer(object.get("cursor")?)?),
  );
  copy_optional_string(object, "search_id", &mut normalized)?;
  copy_optional_string(object, "backtrace", &mut normalized)?;
  Some(Value::Object(normalized))
}

fn canonical_xiaohongshu_search(cursor: &Value) -> Option<Value> {
  let mut normalized = Map::new();
  if let Some(page) = positive_integer(cursor) {
    normalized.insert("page".to_string(), Value::from(page));
    return Some(Value::Object(normalized));
  }

  let object = cursor.as_object()?;
  if !has_only_keys(
    object,
    &["page", "cursor", "search_id", "search_session_id"],
  ) {
    return None;
  }
  let primary = ["page", "cursor"]
    .iter()
    .filter_map(|key| object.get(*key))
    .collect::<Vec<_>>();
  if primary.len() != 1 {
    return None;
  }
  normalized.insert(
    "page".to_string(),
    Value::from(positive_integer(primary[0])?),
  );
  copy_optional_string(object, "search_id", &mut normalized)?;
  copy_optional_string(object, "search_session_id", &mut normalized)?;
  Some(Value::Object(normalized))
}

fn canonical_xiaohongshu_comments(cursor: &Value) -> Option<Value> {
  let object = cursor.as_object()?;
  if object.len() != 2 || !has_only_keys(object, &["cursor", "index"]) {
    return None;
  }
  Some(serde_json::json!({
    "cursor": nonempty_string(object.get("cursor")?)?,
    "index": nonnegative_integer(object.get("index")?)?
  }))
}

fn has_only_keys(object: &Map<String, Value>, allowed: &[&str]) -> bool {
  !object.is_empty() && object.keys().all(|key| allowed.contains(&key.as_str()))
}

fn copy_optional_string(
  source: &Map<String, Value>,
  key: &str,
  target: &mut Map<String, Value>,
) -> Option<()> {
  match source.get(key) {
    None | Some(Value::Null) => {}
    Some(Value::String(text)) if text.trim().is_empty() => {}
    Some(value) => {
      target.insert(key.to_string(), Value::String(nonempty_string(value)?));
    }
  }
  Some(())
}

fn nonnegative_integer(value: &Value) -> Option<i64> {
  value.as_i64().filter(|number| *number >= 0)
}

fn positive_integer(value: &Value) -> Option<i64> {
  value.as_i64().filter(|number| *number > 0)
}

fn nonempty_string(value: &Value) -> Option<String> {
  value
    .as_str()
    .map(str::trim)
    .filter(|text| !text.is_empty())
    .map(ToString::to_string)
}

pub(super) fn cursor_primary(cursor: Option<&Value>, keys: &[&str]) -> Option<Value> {
  let cursor = cursor?;
  if let Some(object) = cursor.as_object() {
    return keys.iter().find_map(|key| object.get(*key).cloned());
  }
  (!cursor.is_null()).then(|| cursor.clone())
}

pub(super) fn copy_cursor_field(
  cursor: Option<&Value>,
  key: &str,
  target: &mut Map<String, Value>,
) {
  if let Some(value) = cursor.and_then(|value| value.get(key)).cloned() {
    target.insert(key.to_string(), value);
  }
}

pub(super) fn copy_cursor_query(
  cursor: Option<&Value>,
  key: &str,
  request: &mut TikHubCollectionRequest,
) {
  if let Some(value) = cursor
    .and_then(|value| value.get(key))
    .and_then(value_to_text)
  {
    request.query.push((key.to_string(), value));
  }
}

pub(super) fn value_to_text(value: &Value) -> Option<String> {
  match value {
    Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
    Value::Number(value) => Some(value.to_string()),
    _ => None,
  }
}

pub(super) fn extract_next_cursor(
  request: &TikHubCollectionRequest,
  data: &Value,
  response: &Value,
) -> Option<Value> {
  if request.data_type == "comments" && request.platform == "xiaohongshu" {
    return required_continuation_object(data, response, &["cursor", "index"]);
  }
  if matches!(request.data_type.as_str(), "keyword_search" | "user_search")
    && request.platform == "douyin"
  {
    return continuation_object(data, response, &["cursor", "search_id", "backtrace"]);
  }
  if matches!(request.data_type.as_str(), "keyword_search" | "user_search")
    && request.platform == "xiaohongshu"
  {
    let mut cursor = Map::new();
    let current_page = request_query_i64(request, "page").unwrap_or(1);
    cursor.insert(
      "page".to_string(),
      Value::from(current_page.checked_add(1)?),
    );
    for key in ["search_id", "search_session_id"] {
      if let Some(value) = continuation_field(data, response, key)
        .cloned()
        .or_else(|| request_query_value(request, key))
      {
        cursor.insert(key.to_string(), value);
      }
    }
    return Some(Value::Object(cursor));
  }
  if request.platform == "tiktok"
    && matches!(request.data_type.as_str(), "followers" | "followings")
  {
    return continuation_object(data, response, &["min_time", "page_token"]);
  }
  if request.platform == "tiktok" && request.data_type == "similar_accounts" {
    return continuation_field(data, response, "next_page_token")
      .or_else(|| continuation_field(data, response, "page_token"))
      .cloned();
  }
  if request.platform == "douyin"
    && matches!(request.data_type.as_str(), "followers" | "followings")
  {
    return continuation_field(data, response, "max_time")
      .cloned()
      .map(|value| serde_json::json!({ "max_time": value }));
  }
  if matches!(request.data_type.as_str(), "keyword_search" | "user_search") {
    return continuation_field(data, response, "cursor")
      .or_else(|| continuation_field(data, response, "offset"))
      .cloned()
      .or_else(|| {
        let offset = request_query_i64(request, "offset")?;
        let count = request_query_i64(request, "count")?;
        Some(Value::from(offset.checked_add(count)?))
      });
  }
  if request.data_type == "account_posts" && request.platform != "xiaohongshu" {
    return continuation_field(data, response, "max_cursor")
      .or_else(|| continuation_field(data, response, "cursor"))
      .cloned();
  }
  continuation_field(data, response, "cursor").cloned()
}

fn required_continuation_object(data: &Value, response: &Value, keys: &[&str]) -> Option<Value> {
  let mut cursor = Map::new();
  for key in keys {
    cursor.insert(
      (*key).to_string(),
      continuation_field(data, response, key)?.clone(),
    );
  }
  Some(Value::Object(cursor))
}

fn continuation_object(data: &Value, response: &Value, keys: &[&str]) -> Option<Value> {
  let mut cursor = Map::new();
  for key in keys {
    if let Some(value) = continuation_field(data, response, key).cloned() {
      cursor.insert((*key).to_string(), value);
    }
  }
  (!cursor.is_empty()).then_some(Value::Object(cursor))
}

fn continuation_field<'a>(data: &'a Value, response: &'a Value, key: &str) -> Option<&'a Value> {
  data.get(key).or_else(|| response.get(key))
}

pub(super) fn boolish(value: &Value) -> Option<bool> {
  value.as_bool().or_else(|| match value.as_i64() {
    Some(0) => Some(false),
    Some(1) => Some(true),
    _ => match value.as_str()?.trim().to_ascii_lowercase().as_str() {
      "true" | "1" => Some(true),
      "false" | "0" => Some(false),
      _ => None,
    },
  })
}

fn request_query_i64(request: &TikHubCollectionRequest, key: &str) -> Option<i64> {
  request
    .query
    .iter()
    .find(|(candidate, _)| candidate == key)
    .and_then(|(_, value)| value.parse().ok())
}

fn request_query_value(request: &TikHubCollectionRequest, key: &str) -> Option<Value> {
  request
    .query
    .iter()
    .find(|(candidate, _)| candidate == key)
    .map(|(_, value)| Value::String(value.clone()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn relation_cursor_contracts_preserve_all_provider_pagination_fields() {
    assert_eq!(
      normalize_provider_cursor(
        "tiktok",
        "followers",
        &serde_json::json!({ "min_time": 10, "page_token": "next" }),
      )
      .unwrap(),
      serde_json::json!({
        "endpoint_key": "tiktok.followers",
        "value": { "min_time": 10, "page_token": "next" }
      })
    );
    assert_eq!(
      normalize_provider_cursor(
        "douyin",
        "followings",
        &serde_json::json!({ "max_time": 123 }),
      )
      .unwrap(),
      serde_json::json!({
        "endpoint_key": "douyin.followings",
        "value": { "max_time": "123" }
      })
    );
  }

  #[test]
  fn search_and_similar_cursor_contracts_reject_wrong_endpoint_or_shape() {
    assert!(normalize_provider_cursor("tiktok", "user_search", &Value::from(20)).is_ok());
    assert!(normalize_provider_cursor(
      "xiaohongshu",
      "user_search",
      &serde_json::json!({ "page": 2, "search_id": "search-1" }),
    )
    .is_ok());
    assert!(normalize_provider_cursor(
      "tiktok",
      "similar_accounts",
      &serde_json::json!({ "page_token": 2 }),
    )
    .is_err());
    assert!(normalize_input_cursor(
      "tiktok",
      "user_search",
      Some(&serde_json::json!({
        "endpoint_key": "tiktok.keyword_search",
        "value": 20
      })),
    )
    .is_err());
  }
}
