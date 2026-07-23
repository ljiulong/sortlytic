use std::io::Read;

use reqwest::StatusCode;
use serde_json::Value;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

const MAX_TIKHUB_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;

pub(super) fn error_for_status(status: StatusCode, message: String) -> AppError {
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
  .with_safe_detail("response_state", "received")
  .with_safe_detail("http_status", status.as_u16().to_string())
}

pub(super) fn reqwest_request_error(error: reqwest::Error) -> AppError {
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
  .with_safe_detail("response_state", "uncertain")
  .with_safe_detail("transport_error", sanitized_error)
}

pub(super) fn read_limited_response_body(reader: impl Read) -> AppResult<String> {
  let mut reader = reader.take(MAX_TIKHUB_RESPONSE_BYTES + 1);
  let mut body = Vec::new();
  reader.read_to_end(&mut body).map_err(|_| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      "TikHub 响应读取失败",
      AppErrorStage::Collection,
      true,
    )
    .with_safe_detail("response_state", "uncertain")
  })?;
  if body.len() as u64 > MAX_TIKHUB_RESPONSE_BYTES {
    return Err(
      AppError::new(
        AppErrorCode::TikhubRequestError,
        format!(
          "TikHub 响应体超过 {} MiB 安全上限",
          MAX_TIKHUB_RESPONSE_BYTES / 1024 / 1024
        ),
        AppErrorStage::Collection,
        false,
      )
      .with_safe_detail("response_state", "received"),
    );
  }
  String::from_utf8(body).map_err(|error| {
    AppError::new(
      AppErrorCode::TikhubRequestError,
      format!("TikHub 响应体不是合法 UTF-8：{error}"),
      AppErrorStage::Collection,
      false,
    )
    .with_safe_detail("response_state", "received")
  })
}

pub(super) fn safe_body_summary(body: &str) -> String {
  format!("响应正文已隐藏（{} 字节）", body.len())
}

pub(super) fn number_field(value: &Value, key: &str) -> Option<f64> {
  value.get(key).and_then(|value| {
    value
      .as_f64()
      .or_else(|| value.as_i64().map(|number| number as f64))
      .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
  })
}

pub(super) fn mask_email(email: &str) -> String {
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
