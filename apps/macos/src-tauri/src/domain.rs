use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AppErrorCode {
  WorkspaceError,
  DatabaseError,
  SecretStoreError,
  PermissionError,
  ValidationError,
  CostLimitError,
  TikhubAuthError,
  TikhubRateLimit,
  TikhubRequestError,
  ModelConfigError,
  ModelAuthError,
  ModelRateLimit,
  ModelRequestError,
  ModelProtocolError,
  ModelSchemaError,
  ExportIntegrityError,
  PdfFontError,
  WebhookError,
  Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppErrorStage {
  Startup,
  Workspace,
  Database,
  SecretStore,
  Provider,
  Validation,
  Collection,
  Ai,
  Export,
  Webhook,
  Backup,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppError {
  pub code: AppErrorCode,
  pub message: String,
  pub stage: AppErrorStage,
  pub retryable: bool,
  pub safe_details: BTreeMap<String, String>,
}

impl AppError {
  pub fn new(
    code: AppErrorCode,
    message: impl Into<String>,
    stage: AppErrorStage,
    retryable: bool,
  ) -> Self {
    Self {
      code,
      message: redact_sensitive_text(&message.into()),
      stage,
      retryable,
      safe_details: BTreeMap::new(),
    }
  }

  pub fn validation(message: impl Into<String>, stage: AppErrorStage) -> Self {
    Self::new(AppErrorCode::ValidationError, message, stage, false)
  }

  pub fn with_safe_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
    self
      .safe_details
      .insert(key.into(), redact_sensitive_text(&value.into()));
    self
  }
}

pub fn redact_sensitive_text(input: &str) -> String {
  let mut output = redact_sensitive_lines(input);
  output = redact_bearer_tokens(&output);
  redact_query_secret_values(&output)
}

fn redact_sensitive_lines(input: &str) -> String {
  input
    .lines()
    .map(redact_sensitive_line)
    .collect::<Vec<_>>()
    .join("\n")
}

fn redact_sensitive_line(line: &str) -> String {
  for separator in [':', '='] {
    if let Some(index) = line.find(separator) {
      let key = line[..index].trim().to_ascii_lowercase();
      if is_sensitive_key(&key) {
        return format!("{}{} [REDACTED]", &line[..index], separator);
      }
    }
  }

  line.to_string()
}

fn redact_bearer_tokens(input: &str) -> String {
  let mut output = String::with_capacity(input.len());
  let mut cursor = 0;
  let lower = input.to_ascii_lowercase();

  while let Some(relative_index) = lower[cursor..].find("bearer ") {
    let start = cursor + relative_index;
    let token_start = start + "bearer ".len();
    output.push_str(&input[cursor..token_start]);

    let token_end = input[token_start..]
      .find(|character: char| character.is_whitespace() || character == ',' || character == '"')
      .map_or(input.len(), |end| token_start + end);

    output.push_str("[REDACTED]");
    cursor = token_end;
  }

  output.push_str(&input[cursor..]);
  output
}

fn redact_query_secret_values(input: &str) -> String {
  let mut output = input.to_string();

  for marker in ["api_key=", "apikey=", "token=", "secret=", "key="] {
    output = redact_after_marker(&output, marker);
  }

  output
}

fn redact_after_marker(input: &str, marker: &str) -> String {
  let lower = input.to_ascii_lowercase();
  let mut output = String::with_capacity(input.len());
  let mut cursor = 0;

  while let Some(relative_index) = lower[cursor..].find(marker) {
    let start = cursor + relative_index;
    let value_start = start + marker.len();
    output.push_str(&input[cursor..value_start]);

    let value_end = input[value_start..]
      .find(|character| ['&', ' ', '\n', '\r', '\t', '"'].contains(&character))
      .map_or(input.len(), |end| value_start + end);

    output.push_str("[REDACTED]");
    cursor = value_end;
  }

  output.push_str(&input[cursor..]);
  output
}

fn is_sensitive_key(key: &str) -> bool {
  matches!(
    key,
    "authorization"
      | "cookie"
      | "set-cookie"
      | "api-key"
      | "api_key"
      | "apikey"
      | "x-api-key"
      | "token"
      | "access_token"
      | "refresh_token"
      | "secret"
      | "client_secret"
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn app_error_serializes_with_expected_shape() {
    let error = AppError::validation("缺少工作区路径", AppErrorStage::Workspace)
      .with_safe_detail("field", "root_path");

    let serialized = serde_json::to_value(error).expect("error should serialize");

    assert_eq!(serialized["code"], "VALIDATION_ERROR");
    assert_eq!(serialized["message"], "缺少工作区路径");
    assert_eq!(serialized["stage"], "workspace");
    assert_eq!(serialized["retryable"], false);
    assert_eq!(serialized["safe_details"]["field"], "root_path");
  }

  #[test]
  fn redacts_sensitive_headers_and_query_values() {
    let input = [
      "Authorization: Bearer sk-live-secret",
      "Cookie=session=secret-cookie",
      "https://example.test/items?api_key=abc123&next=1",
    ]
    .join("\n");

    let redacted = redact_sensitive_text(&input);

    assert!(!redacted.contains("sk-live-secret"));
    assert!(!redacted.contains("secret-cookie"));
    assert!(!redacted.contains("abc123"));
    assert!(redacted.contains("Authorization: [REDACTED]"));
    assert!(redacted.contains("Cookie= [REDACTED]"));
    assert!(redacted.contains("api_key=[REDACTED]"));
  }
}
