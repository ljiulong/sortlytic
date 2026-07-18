use std::time::Duration;

use serde_json::{Map, Value};

use super::{
  error_for_status, normalize_tikhub_base_url, read_limited_response_body, reqwest_request_error,
  safe_body_summary,
};
use crate::collection::validate_collection_params;
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

mod cursor;

use cursor::{
  boolish, copy_cursor_field, copy_cursor_query, cursor_primary, extract_next_cursor,
  normalize_input_cursor, normalize_provider_cursor, value_to_text,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestMethod {
  Get,
  Post,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TikHubCollectionRequest {
  method: RequestMethod,
  paths: Vec<String>,
  query: Vec<(String, String)>,
  body: Option<Value>,
  platform: String,
  data_type: String,
  source_params: Value,
  input_cursor: Option<Value>,
  idempotency_key: Option<String>,
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

#[derive(Debug, Clone, PartialEq)]
pub struct CollectionPage {
  pub records: Vec<Value>,
  pub next_cursor: Option<Value>,
  pub has_more: bool,
  pub raw_response: Value,
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
      push_query(
        &mut request,
        "user_id",
        required_string(&params, "account_id")?,
      );
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
      push_query(
        &mut request,
        "note_id",
        required_string(&params, "item_id")?,
      );
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

pub fn parse_collection_page(
  request: &TikHubCollectionRequest,
  response: Value,
) -> AppResult<CollectionPage> {
  let code = response_code(&response).ok_or_else(|| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      "TikHub 响应缺少业务状态码 code",
      AppErrorStage::Collection,
      false,
    )
  })?;
  if code != 200 {
    return Err(business_response_error(code, &response));
  }

  let data = response.get("data").unwrap_or(&Value::Null);
  let records = extract_records(request, data)?;
  let has_more = match data
    .get("has_more")
    .or_else(|| data.get("data").and_then(|value| value.get("has_more")))
    .or_else(|| response.get("has_more"))
  {
    Some(value) => boolish(value)
      .ok_or_else(|| response_shape_error("invalid_has_more", "has_more 不是布尔值或 0/1"))?,
    None => xiaohongshu_search_has_more(request, data)?,
  };
  let next_cursor = if has_more {
    let raw_cursor = extract_next_cursor(request, data, &response).ok_or_else(|| {
      response_shape_error(
        "invalid_continuation",
        "has_more 为 true，但响应缺少可用的续页游标",
      )
    })?;
    let cursor_envelope =
      normalize_provider_cursor(&request.platform, &request.data_type, &raw_cursor).map_err(
        |_| {
          response_shape_error(
            "invalid_continuation",
            "续页游标类型或字段不符合 endpoint 契约",
          )
        },
      )?;
    let cursor = cursor_envelope
      .get("value")
      .ok_or_else(|| response_shape_error("invalid_continuation", "续页游标不能为空"))?;
    if request.input_cursor.as_ref() == Some(cursor) {
      return Err(response_shape_error(
        "stalled_continuation",
        "续页游标没有前进",
      ));
    }
    Some(cursor_envelope)
  } else {
    None
  };

  Ok(CollectionPage {
    records,
    next_cursor,
    has_more,
    raw_response: response,
  })
}

pub fn send_collection_request(
  base_url: Option<String>,
  token: &str,
  request: &TikHubCollectionRequest,
) -> AppResult<CollectionPage> {
  if token.trim().is_empty() {
    return Err(AppError::validation(
      "TikHub Token 不能为空",
      AppErrorStage::Collection,
    ));
  }
  if request.paths.is_empty() {
    return Err(AppError::validation(
      "TikHub 请求缺少 endpoint",
      AppErrorStage::Collection,
    ));
  }

  let base_url = normalize_tikhub_base_url(base_url)?;
  let client = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(30))
    .build()
    .map_err(reqwest_request_error)?;
  let mut last_error = None;

  for (index, path) in request.paths.iter().enumerate() {
    if !path.starts_with("/api/v1/") {
      return Err(AppError::validation(
        "TikHub endpoint 必须位于 /api/v1/ 下",
        AppErrorStage::Collection,
      ));
    }

    match send_single_request(&client, &base_url, token, path, request) {
      Ok(page) => return Ok(page),
      Err(error) if should_try_video_fallback(request, index, &error) => {
        last_error = Some(error);
      }
      Err(error) => return Err(error),
    }
  }

  Err(last_error.unwrap_or_else(|| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      "TikHub 请求未返回可用结果",
      AppErrorStage::Collection,
      true,
    )
  }))
}

fn send_single_request(
  client: &reqwest::blocking::Client,
  base_url: &str,
  token: &str,
  path: &str,
  request: &TikHubCollectionRequest,
) -> AppResult<CollectionPage> {
  let mut url = reqwest::Url::parse(&format!("{base_url}{path}"))
    .map_err(|_| AppError::validation("TikHub endpoint URL 无效", AppErrorStage::Collection))?;
  if url.path() != path || url.query().is_some() || url.fragment().is_some() {
    return Err(AppError::validation(
      "TikHub endpoint 未通过规范化路径校验",
      AppErrorStage::Collection,
    ));
  }
  let request_builder = match request.method {
    RequestMethod::Get => {
      {
        let mut query = url.query_pairs_mut();
        for (key, value) in &request.query {
          query.append_pair(key, value);
        }
      }
      client.get(url).bearer_auth(token)
    }
    RequestMethod::Post => {
      let builder = client.post(url).bearer_auth(token);
      match request.body.as_ref() {
        Some(body) => builder.json(body),
        None => builder,
      }
    }
  };
  let request_builder = match request.idempotency_key.as_deref() {
    Some(idempotency_key) => request_builder.header("Idempotency-Key", idempotency_key),
    None => request_builder,
  };
  let response = request_builder.send().map_err(reqwest_request_error)?;
  let status = response.status();
  let body = read_limited_response_body(response)?;

  if !status.is_success() {
    return Err(error_for_status(status, safe_body_summary(&body)));
  }

  let response = serde_json::from_str(&body).map_err(|error| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      format!("TikHub 返回内容不是合法 JSON：{error}"),
      AppErrorStage::Collection,
      false,
    )
  })?;
  parse_collection_page(request, response)
}

pub(super) fn should_try_video_fallback(
  request: &TikHubCollectionRequest,
  path_index: usize,
  error: &AppError,
) -> bool {
  request.platform == "xiaohongshu"
    && request.data_type == "item_detail"
    && path_index == 0
    && request.paths.len() == 2
    && error.safe_details.get("response_issue").map(String::as_str) == Some("empty_detail_data")
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

fn response_code(response: &Value) -> Option<i64> {
  response.get("code").and_then(|value| {
    value
      .as_i64()
      .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
  })
}

fn business_response_error(code: i64, _response: &Value) -> AppError {
  let (error_code, retryable) = match code {
    401 | 403 => (AppErrorCode::TikhubAuthError, false),
    402 => (AppErrorCode::CostLimitError, false),
    408 | 425 => (AppErrorCode::TikhubRequestError, true),
    429 => (AppErrorCode::TikhubRateLimit, true),
    500..=599 => (AppErrorCode::TikhubRequestError, true),
    _ => (AppErrorCode::TikhubRequestError, false),
  };

  AppError::new(
    error_code,
    format!("TikHub 业务请求失败，code {code}"),
    AppErrorStage::Collection,
    retryable,
  )
  .with_safe_detail("business_code", code.to_string())
}

fn extract_records(request: &TikHubCollectionRequest, data: &Value) -> AppResult<Vec<Value>> {
  match request.data_type.as_str() {
    "comments" => required_array_field(data, &["comments", "comment_list"]),
    "keyword_search" => {
      if request.platform == "douyin" {
        return Ok(
          required_array_field(data, &["business_data"])?
            .into_iter()
            .map(|record| {
              record
                .pointer("/data/aweme_info")
                .cloned()
                .unwrap_or(record)
            })
            .collect(),
        );
      }
      if request.platform == "xiaohongshu" {
        let search_data = data
          .get("data")
          .filter(|value| value.is_object())
          .unwrap_or(data);
        return Ok(
          required_array_field(search_data, &["items", "notes", "item_list"])?
            .into_iter()
            .map(|record| {
              record
                .get("note")
                .or_else(|| record.get("note_card"))
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or(record)
            })
            .collect(),
        );
      }
      required_array_field(data, &["aweme_list", "items", "notes", "item_list"])
    }
    "account_posts" => required_array_field(data, &["aweme_list", "items", "notes", "item_list"]),
    "account_profile" | "item_detail" if is_non_empty_record(data) => Ok(vec![data.clone()]),
    "account_profile" => Err(response_shape_error(
      "empty_record_data",
      "账号响应缺少非空 data",
    )),
    "item_detail" => Err(response_shape_error(
      "empty_detail_data",
      "详情响应缺少非空 data",
    )),
    _ => Err(response_shape_error(
      "unsupported_data_type",
      "请求数据类型没有对应的解析器",
    )),
  }
}

fn xiaohongshu_search_has_more(request: &TikHubCollectionRequest, data: &Value) -> AppResult<bool> {
  if request.platform != "xiaohongshu" || request.data_type != "keyword_search" {
    return Ok(false);
  }
  let Some(next_page) = data.get("next_page") else {
    return Ok(false);
  };
  let next_page = next_page
    .as_i64()
    .filter(|value| *value > 0)
    .ok_or_else(|| response_shape_error("invalid_continuation", "next_page 不是大于 0 的整数"))?;
  let current_page = data
    .get("page")
    .and_then(Value::as_i64)
    .or_else(|| {
      request
        .query
        .iter()
        .find(|(key, _)| key == "page")
        .and_then(|(_, value)| value.parse().ok())
    })
    .filter(|value| *value > 0)
    .ok_or_else(|| response_shape_error("invalid_continuation", "响应缺少有效的当前页码"))?;
  Ok(next_page > current_page)
}

fn required_array_field(data: &Value, keys: &[&str]) -> AppResult<Vec<Value>> {
  let records = data
    .as_array()
    .or_else(|| {
      keys
        .iter()
        .find_map(|key| data.get(*key).and_then(Value::as_array))
    })
    .ok_or_else(|| {
      response_shape_error("missing_record_array", "列表响应缺少预期的记录数组字段")
    })?;
  if records.iter().all(Value::is_object) {
    Ok(records.clone())
  } else {
    Err(response_shape_error(
      "invalid_record",
      "记录数组只能包含对象",
    ))
  }
}

fn is_non_empty_record(data: &Value) -> bool {
  match data {
    Value::Object(object) => !object.is_empty(),
    _ => false,
  }
}

fn response_shape_error(issue: &str, message: &str) -> AppError {
  AppError::new(
    AppErrorCode::TikhubRequestError,
    format!("TikHub 响应结构不符合 endpoint 契约：{message}"),
    AppErrorStage::Collection,
    false,
  )
  .with_safe_detail("response_issue", issue)
}
