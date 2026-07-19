use serde_json::{Map, Value};

use crate::collection::validate_collection_params;
use crate::domain::{AppError, AppErrorStage, AppResult};

use super::cursor::{
  copy_cursor_field, copy_cursor_query, cursor_primary, normalize_input_cursor, value_to_text,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestMethod {
  Get,
  Post,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TikHubCollectionRequest {
  pub(super) method: RequestMethod,
  pub(super) paths: Vec<String>,
  pub(super) query: Vec<(String, String)>,
  pub(super) body: Option<Value>,
  pub(super) platform: String,
  pub(super) data_type: String,
  pub(super) source_params: Value,
  pub(super) input_cursor: Option<Value>,
  pub(super) idempotency_key: Option<String>,
}

impl TikHubCollectionRequest {
  pub fn method(&self) -> RequestMethod {
    self.method
  }

  pub fn paths(&self) -> &[String] {
    &self.paths
  }

  pub fn query(&self) -> &[(String, String)] {
    &self.query
  }

  pub fn body(&self) -> Option<&Value> {
    self.body.as_ref()
  }

  pub fn source_params(&self) -> &Value {
    &self.source_params
  }

  pub fn idempotency_key(&self) -> Option<&str> {
    self.idempotency_key.as_deref()
  }

  pub fn with_idempotency_key(mut self, idempotency_key: String) -> AppResult<Self> {
    let idempotency_key = idempotency_key.trim();
    if idempotency_key.is_empty() || idempotency_key.len() > 128 {
      return Err(AppError::validation(
        "TikHub 幂等键不能为空且长度不能超过 128 个字符",
        AppErrorStage::Collection,
      ));
    }
    self.idempotency_key = Some(idempotency_key.to_string());
    Ok(self)
  }
}

pub fn build_collection_request(
  platform: &str,
  data_type: &str,
  params: &Value,
  cursor: Option<&Value>,
) -> AppResult<TikHubCollectionRequest> {
  let normalized_cursor = normalize_input_cursor(platform, data_type, cursor)?;
  let validation = validate_collection_params(platform, data_type, params.clone())?;
  if !validation.valid {
    let mut errors = validation.errors;
    errors.extend(
      validation
        .missing_fields
        .into_iter()
        .map(|field| format!("缺少必填参数 {field}")),
    );
    return Err(AppError::validation(
      errors.join("；"),
      AppErrorStage::Collection,
    ));
  }

  let params = validation.normalized_params;
  let mut request = TikHubCollectionRequest {
    method: RequestMethod::Get,
    paths: Vec::new(),
    query: Vec::new(),
    body: None,
    platform: platform.trim().to_string(),
    data_type: data_type.trim().to_string(),
    source_params: params.clone(),
    input_cursor: normalized_cursor.clone(),
    idempotency_key: None,
  };
  let cursor = normalized_cursor.as_ref();

  match (platform.trim(), data_type.trim()) {
    ("tiktok", "keyword_search") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/fetch_video_search_result".to_string());
      push_query(
        &mut request,
        "keyword",
        required_string(&params, "keyword")?,
      );
      push_query(
        &mut request,
        "offset",
        cursor_primary(cursor, &["offset", "cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
      if let Some(region) = params.get("region").and_then(Value::as_str) {
        push_query(&mut request, "region", region.to_string());
      }
      if let Some(publish_time) = relative_days(&params) {
        push_query(&mut request, "publish_time", publish_time);
      }
    }
    ("tiktok", "comments") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/fetch_video_comments".to_string());
      push_query(
        &mut request,
        "aweme_id",
        required_string(&params, "item_id")?,
      );
      push_query(
        &mut request,
        "cursor",
        cursor_primary(cursor, &["cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
    }
    ("tiktok", "account_profile") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/handler_user_profile".to_string());
      push_query(
        &mut request,
        "unique_id",
        required_string(&params, "account_id")?,
      );
    }
    ("tiktok", "account_posts") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/fetch_user_post_videos".to_string());
      push_query(
        &mut request,
        "sec_user_id",
        required_string(&params, "account_id")?,
      );
      push_query(
        &mut request,
        "max_cursor",
        cursor_primary(cursor, &["max_cursor", "cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
      push_query(&mut request, "sort_type", "0".to_string());
      if let Some(region) = params.get("region").and_then(Value::as_str) {
        push_query(&mut request, "region", region.to_string());
      }
    }
    ("tiktok", "item_detail") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/fetch_one_video".to_string());
      push_query(
        &mut request,
        "aweme_id",
        required_string(&params, "item_id")?,
      );
    }
    ("douyin", "keyword_search") => {
      request.method = RequestMethod::Post;
      request
        .paths
        .push("/api/v1/douyin/search/fetch_video_search_v2".to_string());
      let mut body = Map::new();
      body.insert(
        "keyword".to_string(),
        Value::String(required_string(&params, "keyword")?),
      );
      body.insert(
        "cursor".to_string(),
        cursor_primary(cursor, &["cursor"]).unwrap_or_else(|| Value::from(0)),
      );
      copy_cursor_field(cursor, "search_id", &mut body);
      copy_cursor_field(cursor, "backtrace", &mut body);
      if let Some(publish_time) = douyin_publish_time(&params) {
        body.insert("publish_time".to_string(), Value::String(publish_time));
      }
      request.body = Some(Value::Object(body));
    }
    ("douyin", "comments") => {
      request
        .paths
        .push("/api/v1/douyin/app/v3/fetch_video_comments".to_string());
      push_query(
        &mut request,
        "aweme_id",
        required_string(&params, "item_id")?,
      );
      push_query(
        &mut request,
        "cursor",
        cursor_primary(cursor, &["cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
    }
    ("douyin", "account_profile") => {
      request
        .paths
        .push("/api/v1/douyin/app/v3/handler_user_profile".to_string());
      push_query(
        &mut request,
        "sec_user_id",
        required_string(&params, "account_id")?,
      );
    }
    ("douyin", "account_posts") => {
      request
        .paths
        .push("/api/v1/douyin/app/v3/fetch_user_post_videos".to_string());
      push_query(
        &mut request,
        "sec_user_id",
        required_string(&params, "account_id")?,
      );
      push_query(
        &mut request,
        "max_cursor",
        cursor_primary(cursor, &["max_cursor", "cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
      push_query(&mut request, "sort_type", "0".to_string());
    }
    ("douyin", "item_detail") => {
      request
        .paths
        .push("/api/v1/douyin/app/v3/fetch_one_video".to_string());
      push_query(
        &mut request,
        "aweme_id",
        required_string(&params, "item_id")?,
      );
    }
    ("xiaohongshu", "keyword_search") => {
      request
        .paths
        .push("/api/v1/xiaohongshu/app_v2/search_notes".to_string());
      push_query(
        &mut request,
        "keyword",
        required_string(&params, "keyword")?,
      );
      push_query(
        &mut request,
        "page",
        cursor_primary(cursor, &["page", "cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "1".to_string()),
      );
      copy_cursor_query(cursor, "search_id", &mut request);
      copy_cursor_query(cursor, "search_session_id", &mut request);
      if let Some(time_filter) = xiaohongshu_time_filter(&params) {
        push_query(&mut request, "time_filter", time_filter);
      }
    }
    ("xiaohongshu", "comments") => {
      request
        .paths
        .push("/api/v1/xiaohongshu/app_v2/get_note_comments".to_string());
      push_query(
        &mut request,
        "note_id",
        required_string(&params, "item_id")?,
      );
      if let Some(cursor) =
        cursor_primary(cursor, &["cursor"]).and_then(|value| value_to_text(&value))
      {
        push_query(&mut request, "cursor", cursor);
      }
      push_query(
        &mut request,
        "index",
        cursor
          .and_then(|value| value.get("index"))
          .and_then(value_to_text)
          .unwrap_or_else(|| "0".to_string()),
      );
    }
    ("xiaohongshu", "account_profile") => {
      request
        .paths
        .push("/api/v1/xiaohongshu/app_v2/get_user_info".to_string());
      push_xiaohongshu_id_or_share(&mut request, &params, "user_id", "account_id")?;
    }
    ("xiaohongshu", "account_posts") => {
      request
        .paths
        .push("/api/v1/xiaohongshu/app_v2/get_user_posted_notes".to_string());
      push_query(
        &mut request,
        "user_id",
        required_string(&params, "account_id")?,
      );
      if let Some(cursor) =
        cursor_primary(cursor, &["cursor"]).and_then(|value| value_to_text(&value))
      {
        push_query(&mut request, "cursor", cursor);
      }
    }
    ("xiaohongshu", "item_detail") => {
      request.paths.extend([
        "/api/v1/xiaohongshu/app_v2/get_image_note_detail".to_string(),
        "/api/v1/xiaohongshu/app_v2/get_video_note_detail".to_string(),
      ]);
      push_xiaohongshu_id_or_share(&mut request, &params, "note_id", "item_id")?;
    }
    ("tiktok", "user_search") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/fetch_user_search_result".to_string());
      push_query(
        &mut request,
        "keyword",
        required_string(&params, "keyword")?,
      );
      push_query(
        &mut request,
        "offset",
        cursor_primary(cursor, &["offset", "cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
    }
    ("tiktok", "followers" | "followings") => {
      let path = if data_type.trim() == "followers" {
        "/api/v1/tiktok/app/v3/fetch_user_follower_list"
      } else {
        "/api/v1/tiktok/app/v3/fetch_user_following_list"
      };
      request.paths.push(path.to_string());
      push_tiktok_account_identifier(&mut request, required_string(&params, "account_id")?);
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
      push_query(
        &mut request,
        "min_time",
        cursor_primary(cursor, &["min_time"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(
        &mut request,
        "page_token",
        cursor_primary(cursor, &["page_token"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_default(),
      );
    }
    ("tiktok", "similar_accounts") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/fetch_similar_user_recommendations".to_string());
      push_query(
        &mut request,
        "sec_uid",
        required_string(&params, "account_id")?,
      );
      if let Some(page_token) =
        cursor_primary(cursor, &["page_token"]).and_then(|value| value_to_text(&value))
      {
        push_query(&mut request, "page_token", page_token);
      }
    }
    ("tiktok", "account_country") => {
      request
        .paths
        .push("/api/v1/tiktok/app/v3/fetch_user_country_by_username".to_string());
      push_query(
        &mut request,
        "username",
        required_string(&params, "account_id")?,
      );
    }
    ("douyin", "user_search") => {
      request.method = RequestMethod::Post;
      request
        .paths
        .push("/api/v1/douyin/search/fetch_user_search".to_string());
      let mut body = Map::from_iter([
        (
          "keyword".to_string(),
          Value::String(required_string(&params, "keyword")?),
        ),
        (
          "cursor".to_string(),
          cursor_primary(cursor, &["cursor"]).unwrap_or_else(|| Value::from(0)),
        ),
        ("douyin_user_fans".to_string(), Value::String(String::new())),
        ("douyin_user_type".to_string(), Value::String(String::new())),
        ("search_id".to_string(), Value::String(String::new())),
      ]);
      copy_cursor_field(cursor, "search_id", &mut body);
      request.body = Some(Value::Object(body));
    }
    ("douyin", "followers" | "followings") => {
      let path = if data_type.trim() == "followers" {
        "/api/v1/douyin/web/fetch_user_fans_list"
      } else {
        "/api/v1/douyin/web/fetch_user_following_list"
      };
      request.paths.push(path.to_string());
      push_query(
        &mut request,
        "sec_user_id",
        required_string(&params, "account_id")?,
      );
      push_query(
        &mut request,
        "max_time",
        cursor_primary(cursor, &["max_time"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "0".to_string()),
      );
      push_query(&mut request, "count", page_size(&params, 20)?.to_string());
      push_query(
        &mut request,
        "source_type",
        if cursor.is_none() { "2" } else { "1" }.to_string(),
      );
    }
    ("douyin", "extended_demographics") => {
      request
        .paths
        .push("/api/v1/douyin/web/handler_user_profile_v4".to_string());
      push_query(
        &mut request,
        "sec_user_id",
        required_string(&params, "account_id")?,
      );
    }
    ("xiaohongshu", "user_search") => {
      request
        .paths
        .push("/api/v1/xiaohongshu/app_v2/search_users".to_string());
      push_query(
        &mut request,
        "keyword",
        required_string(&params, "keyword")?,
      );
      push_query(
        &mut request,
        "page",
        cursor_primary(cursor, &["page", "cursor"])
          .and_then(|value| value_to_text(&value))
          .unwrap_or_else(|| "1".to_string()),
      );
      copy_cursor_query(cursor, "search_id", &mut request);
    }
    _ => {
      return Err(AppError::validation(
        "平台或数据类型不受支持",
        AppErrorStage::Collection,
      ));
    }
  }

  Ok(request)
}

fn required_string(params: &Value, key: &str) -> AppResult<String> {
  params
    .get(key)
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(ToString::to_string)
    .ok_or_else(|| AppError::validation(format!("缺少必填参数 {key}"), AppErrorStage::Collection))
}

fn page_size(params: &Value, default: i64) -> AppResult<i64> {
  let value = match params.get("page_size") {
    None => default,
    Some(value) => value.as_i64().ok_or_else(|| {
      AppError::validation("page_size 必须是大于 0 的整数", AppErrorStage::Collection)
    })?,
  };
  if value <= 0 {
    return Err(AppError::validation(
      "page_size 必须是大于 0 的整数",
      AppErrorStage::Collection,
    ));
  }
  Ok(value)
}

fn push_query(request: &mut TikHubCollectionRequest, key: &str, value: String) {
  request.query.push((key.to_string(), value));
}

fn push_tiktok_account_identifier(request: &mut TikHubCollectionRequest, account_id: String) {
  let key = if account_id
    .chars()
    .all(|character| character.is_ascii_digit())
  {
    "user_id"
  } else {
    "sec_user_id"
  };
  push_query(request, key, account_id);
}

fn push_xiaohongshu_id_or_share(
  request: &mut TikHubCollectionRequest,
  params: &Value,
  provider_id_key: &str,
  business_id_key: &str,
) -> AppResult<()> {
  if let Some(value) = params
    .get(business_id_key)
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
  {
    push_query(request, provider_id_key, value.to_string());
    return Ok(());
  }
  if let Some(value) = params
    .get("share_text")
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty())
  {
    push_query(request, "share_text", value.to_string());
    return Ok(());
  }
  Err(AppError::validation(
    format!("{business_id_key} 与 share_text 必须提供一项"),
    AppErrorStage::Collection,
  ))
}

fn relative_days(params: &Value) -> Option<String> {
  let value = params.get("time_range")?.as_str()?;
  let compact = value
    .chars()
    .filter(|character| !character.is_whitespace())
    .collect::<String>();
  match compact.as_str() {
    "近1天" | "1" => Some("1".to_string()),
    "近7天" | "7" => Some("7".to_string()),
    "近30天" | "30" => Some("30".to_string()),
    "近180天" | "180" => Some("180".to_string()),
    _ => None,
  }
}

fn xiaohongshu_time_filter(params: &Value) -> Option<String> {
  match relative_days(params).as_deref() {
    Some("1") => Some("一天内".to_string()),
    Some("7") => Some("一周内".to_string()),
    Some("180") => Some("半年内".to_string()),
    _ => None,
  }
}

fn douyin_publish_time(params: &Value) -> Option<String> {
  relative_days(params).filter(|value| matches!(value.as_str(), "1" | "7" | "180"))
}
