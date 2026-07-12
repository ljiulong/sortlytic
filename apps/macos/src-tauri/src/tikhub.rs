use std::io::Read;
use std::path::Path;
use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::secrets::read_secret_for_backend;

mod collection;

pub use collection::{
  build_collection_request, parse_collection_page, send_collection_request, CollectionPage,
  RequestMethod, TikHubCollectionRequest,
};

const DEFAULT_BASE_URL: &str = "https://api.tikhub.io";
const CHINA_BASE_URL: &str = "https://api.tikhub.dev";
const MAX_TIKHUB_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TikhubConnectionTestResult {
  pub success: bool,
  pub base_url: String,
  pub masked_email: Option<String>,
  pub balance: Option<f64>,
  pub free_credit: Option<f64>,
  pub email_verified: Option<bool>,
  pub api_key_status: Option<i64>,
  pub daily_usage_json: Value,
  pub message: String,
}

pub fn test_tikhub_connection(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
  base_url: Option<String>,
) -> AppResult<TikhubConnectionTestResult> {
  let base_url = normalize_tikhub_base_url(base_url)?;
  let token = read_secret_for_backend(root_path, secret_ref_id)?;
  let client = reqwest::blocking::Client::builder()
    .timeout(Duration::from_secs(20))
    .build()
    .map_err(reqwest_request_error)?;
  let user_info = get_tikhub_json(
    &client,
    &base_url,
    "/api/v1/tikhub/user/get_user_info",
    &token,
  )?;
  let daily_usage = get_tikhub_json(
    &client,
    &base_url,
    "/api/v1/tikhub/user/get_user_daily_usage",
    &token,
  )
  .unwrap_or_else(|error| {
    serde_json::json!({
      "warning": error.message
    })
  });
  let user_data = user_info.get("user_data").unwrap_or(&Value::Null);
  let api_key_data = user_info.get("api_key_data").unwrap_or(&Value::Null);
  let email_verified = user_data.get("email_verified").and_then(Value::as_bool);
  let balance = number_field(user_data, "balance");
  let free_credit = number_field(user_data, "free_credit");
  let api_key_status = api_key_data.get("api_key_status").and_then(Value::as_i64);
  let masked_email = user_data
    .get("email")
    .and_then(Value::as_str)
    .map(mask_email);
  let message = match (email_verified, free_credit) {
    (Some(false), _) => "TikHub Token 可用，但账号邮箱尚未验证".to_string(),
    (_, Some(value)) => format!("TikHub Token 可用，当前免费额度 {value}"),
    _ => "TikHub Token 可用".to_string(),
  };

  Ok(TikhubConnectionTestResult {
    success: true,
    base_url,
    masked_email,
    balance,
    free_credit,
    email_verified,
    api_key_status,
    daily_usage_json: daily_usage,
    message,
  })
}

fn get_tikhub_json(
  client: &reqwest::blocking::Client,
  base_url: &str,
  path: &str,
  token: &str,
) -> AppResult<Value> {
  let url = format!("{base_url}{path}");
  let response = client
    .get(url)
    .bearer_auth(token)
    .send()
    .map_err(reqwest_request_error)?;
  let status = response.status();
  let body = read_limited_response_body(response)?;

  if !status.is_success() {
    return Err(error_for_status(status, safe_body_summary(&body)));
  }

  serde_json::from_str(&body).map_err(|error| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      format!("TikHub 返回内容不是合法 JSON：{error}"),
      AppErrorStage::Collection,
      true,
    )
  })
}

fn normalize_tikhub_base_url(base_url: Option<String>) -> AppResult<String> {
  let base_url = base_url
    .map(|value| value.trim().trim_end_matches('/').to_string())
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

  match base_url.as_str() {
    DEFAULT_BASE_URL | CHINA_BASE_URL => Ok(base_url),
    _ => Err(AppError::validation(
      "TikHub Base URL 只允许 https://api.tikhub.io 或 https://api.tikhub.dev",
      AppErrorStage::Collection,
    )),
  }
}

fn error_for_status(status: StatusCode, message: String) -> AppError {
  let (code, retryable) = match status {
    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => (AppErrorCode::TikhubAuthError, false),
    StatusCode::PAYMENT_REQUIRED => (AppErrorCode::CostLimitError, false),
    StatusCode::TOO_MANY_REQUESTS => (AppErrorCode::TikhubRateLimit, true),
    StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_EARLY => (AppErrorCode::TikhubRequestError, true),
    _ => (AppErrorCode::TikhubRequestError, status.is_server_error()),
  };

  AppError::new(
    code,
    format!("TikHub 请求失败，HTTP {}：{}", status.as_u16(), message),
    AppErrorStage::Collection,
    retryable,
  )
}

fn tikhub_request_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::TikhubRequestError,
    error.to_string(),
    AppErrorStage::Collection,
    true,
  )
}

fn reqwest_request_error(error: reqwest::Error) -> AppError {
  if let Some(status) = error.status() {
    return error_for_status(status, "响应正文已隐藏".to_string());
  }
  let retryable = error.is_timeout() || error.is_connect() || error.is_body();
  let message = if error.is_timeout() {
    "TikHub 请求超时"
  } else if error.is_connect() {
    "TikHub 连接失败"
  } else if error.is_body() {
    "TikHub 响应读取失败"
  } else if error.is_builder() {
    "TikHub 请求构造失败"
  } else if error.is_redirect() {
    "TikHub 重定向被拒绝"
  } else {
    "TikHub 请求失败"
  };
  let sanitized_error = error.without_url().to_string();
  AppError::new(
    AppErrorCode::TikhubRequestError,
    message,
    AppErrorStage::Collection,
    retryable,
  )
  .with_safe_detail("transport_error", sanitized_error)
}

fn read_limited_response_body(reader: impl Read) -> AppResult<String> {
  let mut reader = reader.take(MAX_TIKHUB_RESPONSE_BYTES + 1);
  let mut body = Vec::new();
  reader
    .read_to_end(&mut body)
    .map_err(tikhub_request_error)?;
  if body.len() as u64 > MAX_TIKHUB_RESPONSE_BYTES {
    return Err(AppError::new(
      AppErrorCode::TikhubRequestError,
      format!(
        "TikHub 响应体超过 {} MiB 安全上限",
        MAX_TIKHUB_RESPONSE_BYTES / 1024 / 1024
      ),
      AppErrorStage::Collection,
      false,
    ));
  }
  String::from_utf8(body).map_err(|error| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      format!("TikHub 响应体不是合法 UTF-8：{error}"),
      AppErrorStage::Collection,
      false,
    )
  })
}

fn safe_body_summary(body: &str) -> String {
  format!("响应正文已隐藏（{} 字节）", body.len())
}

fn number_field(value: &Value, key: &str) -> Option<f64> {
  value.get(key).and_then(|value| {
    value
      .as_f64()
      .or_else(|| value.as_i64().map(|number| number as f64))
  })
}

fn mask_email(email: &str) -> String {
  let Some((name, domain)) = email.split_once('@') else {
    return "[REDACTED]".to_string();
  };
  let mut chars = name.chars();
  let first = chars.next().unwrap_or('*');
  let last = name.chars().last().unwrap_or(first);

  if name.chars().count() <= 2 {
    format!("{first}***@{domain}")
  } else {
    format!("{first}***{last}@{domain}")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn base_url_is_limited_to_official_tikhub_domains() {
    assert_eq!(
      normalize_tikhub_base_url(None).expect("default allowed"),
      DEFAULT_BASE_URL
    );
    assert!(normalize_tikhub_base_url(Some("https://api.tikhub.dev/".to_string())).is_ok());
    assert!(normalize_tikhub_base_url(Some("https://example.com".to_string())).is_err());
  }

  #[test]
  fn email_mask_keeps_domain_and_hides_name() {
    assert_eq!(mask_email("example@example.com"), "e***e@example.com");
    assert_eq!(mask_email("ab@example.com"), "a***@example.com");
  }

  #[test]
  fn response_body_reader_enforces_hard_size_limit() {
    let valid = read_limited_response_body(std::io::Cursor::new(b"{\"code\":200}"))
      .expect("small response should be accepted");
    assert_eq!(valid, "{\"code\":200}");

    let error = read_limited_response_body(std::io::repeat(b'x'))
      .expect_err("unbounded response must stop at the configured limit");
    assert_eq!(error.code, AppErrorCode::TikhubRequestError);
    assert!(error.message.contains("响应体超过"));
  }

  #[test]
  fn response_error_summary_never_echoes_untrusted_body() {
    let summary = safe_body_summary("token-without-a-label private keyword");

    assert!(!summary.contains("token-without-a-label"));
    assert!(!summary.contains("private keyword"));
    assert!(summary.contains("字节"));
  }

  #[test]
  fn transient_http_statuses_are_retryable() {
    assert!(error_for_status(StatusCode::REQUEST_TIMEOUT, "已隐藏".to_string()).retryable);
    assert!(error_for_status(StatusCode::TOO_EARLY, "已隐藏".to_string()).retryable);
  }
}

#[cfg(test)]
#[path = "tikhub/collection_tests.rs"]
mod collection_tests;
