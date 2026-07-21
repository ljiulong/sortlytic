use reqwest::StatusCode;

use crate::domain::{AppError, AppErrorCode, AppErrorStage};

pub(super) fn status_error(status: StatusCode, retry_after: Option<&str>) -> AppError {
  let (code, message, retryable) = match status.as_u16() {
    401 | 403 => (
      AppErrorCode::ModelAuthError,
      "AI 服务鉴权失败，请检查 API Key 和访问权限",
      false,
    ),
    429 => (
      AppErrorCode::ModelRateLimit,
      "AI 服务请求过于频繁或额度不足，请稍后重试",
      true,
    ),
    500..=599 => (
      AppErrorCode::ModelRequestError,
      "AI 服务暂时不可用，请稍后重试",
      true,
    ),
    _ => (
      AppErrorCode::ModelProtocolError,
      "AI 服务拒绝了请求，请检查 Base URL、模型 ID 和协议",
      false,
    ),
  };
  let mut error = model_error(code, message, retryable)
    .with_safe_detail("http_status", status.as_u16().to_string());
  if status == StatusCode::TOO_MANY_REQUESTS {
    if let Some(retry_after) = retry_after.and_then(safe_retry_after) {
      error = error.with_safe_detail("retry_after", retry_after);
    }
  }
  error
}

pub(super) fn safe_retry_after(value: &str) -> Option<&str> {
  let value = value.trim();
  (!value.is_empty()
    && value.len() <= 64
    && (value.chars().all(|character| character.is_ascii_digit())
      || chrono::DateTime::parse_from_rfc2822(value).is_ok()))
  .then_some(value)
}

pub(super) fn transport_error(error: reqwest::Error) -> AppError {
  let (message, retryable, kind) = if error.is_timeout() {
    ("AI 服务请求超时", true, "timeout")
  } else if error.is_connect() {
    ("无法连接 AI 服务，请检查 Base URL 和网络", true, "connect")
  } else if error.is_redirect() {
    ("AI 服务返回重定向，已按安全策略拒绝", false, "redirect")
  } else if error.is_body() {
    ("读取 AI 服务响应失败", true, "body")
  } else {
    ("AI 服务请求失败", true, "request")
  };
  let code = if kind == "redirect" {
    AppErrorCode::ModelProtocolError
  } else {
    AppErrorCode::ModelRequestError
  };
  model_error(code, message, retryable).with_safe_detail("transport_kind", kind)
}

pub(super) fn model_error(code: AppErrorCode, message: &str, retryable: bool) -> AppError {
  AppError::new(code, message, AppErrorStage::Ai, retryable)
}
