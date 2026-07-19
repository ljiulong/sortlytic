use serde_json::Value;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

use super::cursor::{boolish, extract_next_cursor, normalize_provider_cursor};
use super::request::TikHubCollectionRequest;

#[derive(Debug, Clone, PartialEq)]
pub struct CollectionPage {
  pub records: Vec<Value>,
  pub next_cursor: Option<Value>,
  pub has_more: bool,
  pub raw_response: Value,
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
    return Err(business_response_error(code));
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

fn response_code(response: &Value) -> Option<i64> {
  response.get("code").and_then(|value| {
    value
      .as_i64()
      .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
  })
}

fn business_response_error(code: i64) -> AppError {
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
      if request.platform == "tiktok" {
        return Ok(
          required_array_field(
            data,
            &[
              "search_item_list",
              "aweme_list",
              "items",
              "notes",
              "item_list",
            ],
          )?
          .into_iter()
          .map(|record| {
            record
              .get("aweme_info")
              .filter(|value| value.is_object())
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
        .filter_map(|key| data.get(*key).and_then(Value::as_array))
        .find(|records| !records.is_empty())
        .or_else(|| {
          keys
            .iter()
            .find_map(|key| data.get(*key).and_then(Value::as_array))
        })
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
  matches!(data, Value::Object(object) if !object.is_empty())
}

pub(super) fn response_shape_error(issue: &str, message: &str) -> AppError {
  AppError::new(
    AppErrorCode::TikhubRequestError,
    format!("TikHub 响应结构不符合 endpoint 契约：{message}"),
    AppErrorStage::Collection,
    false,
  )
  .with_safe_detail("response_issue", issue)
}
