use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, RETRY_AFTER};

use super::{
  error_for_status, normalize_tikhub_base_url, read_limited_response_body, reqwest_request_error,
  safe_body_summary,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

mod cursor;
mod request;
mod response;

pub use request::{build_collection_request, RequestMethod, TikHubCollectionRequest};
pub use response::{parse_collection_page, CollectionPage};

const COLLECTION_MAX_ATTEMPTS: usize = 3;
const COLLECTION_RETRY_BASE_DELAY_MS: u64 = 1_000;
const COLLECTION_RETRY_MAX_DELAY_MS: u64 = 5_000;

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
  if request.method == RequestMethod::Get {
    let mut query = url.query_pairs_mut();
    for (key, value) in &request.query {
      query.append_pair(key, value);
    }
  }

  for attempt in 0..COLLECTION_MAX_ATTEMPTS {
    let request_builder = match request.method {
      RequestMethod::Get => client.get(url.clone()).bearer_auth(token),
      RequestMethod::Post => {
        let builder = client.post(url.clone()).bearer_auth(token);
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
    let retry_after = retry_after_delay(response.headers());
    let body = read_limited_response_body(response)?;
    let result = if !status.is_success() {
      Err(error_for_status(status, safe_body_summary(&body)))
    } else {
      serde_json::from_str(&body)
        .map_err(|error| {
          AppError::new(
            AppErrorCode::TikhubRequestError,
            format!("TikHub 返回内容不是合法 JSON：{error}"),
            AppErrorStage::Collection,
            false,
          )
        })
        .and_then(|response| parse_collection_page(request, response))
    };

    match result {
      Ok(page) => return Ok(page),
      Err(error)
        if error.code == AppErrorCode::TikhubRateLimit && attempt + 1 < COLLECTION_MAX_ATTEMPTS =>
      {
        std::thread::sleep(collection_retry_delay(attempt, retry_after));
      }
      Err(error) if error.code == AppErrorCode::TikhubRateLimit => {
        return Err(error.with_safe_detail("retry_attempts", (attempt + 1).to_string()));
      }
      Err(error) => return Err(error),
    }
  }

  unreachable!("collection retry loop returns on its final attempt")
}

fn collection_retry_delay(attempt: usize, retry_after: Option<Duration>) -> Duration {
  retry_after.unwrap_or_else(|| {
    let multiplier = 1_u64 << attempt.min(8);
    Duration::from_millis(
      COLLECTION_RETRY_BASE_DELAY_MS
        .saturating_mul(multiplier)
        .min(COLLECTION_RETRY_MAX_DELAY_MS),
    )
  })
}

fn retry_after_delay(headers: &HeaderMap) -> Option<Duration> {
  let value = headers.get(RETRY_AFTER)?.to_str().ok()?.trim();
  if let Ok(seconds) = value.parse::<u64>() {
    return Some(Duration::from_millis(
      seconds
        .saturating_mul(1_000)
        .min(COLLECTION_RETRY_MAX_DELAY_MS),
    ));
  }
  let retry_at = DateTime::parse_from_rfc2822(value)
    .ok()?
    .with_timezone(&Utc);
  let milliseconds = retry_at
    .signed_duration_since(Utc::now())
    .num_milliseconds()
    .max(0) as u64;
  Some(Duration::from_millis(
    milliseconds.min(COLLECTION_RETRY_MAX_DELAY_MS),
  ))
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
