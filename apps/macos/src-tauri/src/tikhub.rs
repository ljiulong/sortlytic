use std::io::Read;
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use reqwest::StatusCode;
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::secrets::{read_secret_for_backend, validate_secret_ref_provider};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TikhubConnectorInput {
  pub secret_ref_id: Option<String>,
  pub base_url: String,
  pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TikhubConnectorView {
  pub id: String,
  pub workspace_id: String,
  pub secret_ref_id: Option<String>,
  pub base_url: String,
  pub enabled: bool,
  pub config_version: i64,
  pub last_tested_at: Option<String>,
  pub last_test_status: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

pub fn get_tikhub_connector(root_path: impl AsRef<Path>) -> AppResult<Option<TikhubConnectorView>> {
  let connection = open_connector_connection(root_path)?;
  read_connector(&connection)
}

pub fn save_tikhub_connector(
  root_path: impl AsRef<Path>,
  input: TikhubConnectorInput,
) -> AppResult<TikhubConnectorView> {
  let base_url = normalize_tikhub_base_url(Some(input.base_url))?;
  let secret_ref_id = input.secret_ref_id.and_then(|value| {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
  });
  if input.enabled && secret_ref_id.is_none() {
    return Err(AppError::validation(
      "启用 TikHub 连接器前必须选择 TikHub 密钥",
      AppErrorStage::Collection,
    ));
  }

  let mut connection = open_connector_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if let Some(secret_ref_id) = secret_ref_id.as_deref() {
    validate_secret_ref_provider(&transaction, secret_ref_id, "tikhub")?;
  }
  let workspace_id = transaction
    .query_row("SELECT id FROM workspace", [], |row| {
      row.get::<_, String>(0)
    })
    .map_err(database_error)?;
  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "INSERT INTO tikhub_connector (
        id, workspace_id, secret_ref_id, base_url, enabled, config_version,
        last_tested_at, last_test_status, created_at, updated_at
      ) VALUES ('default', ?1, ?2, ?3, ?4, 1, NULL, NULL, ?5, ?5)
      ON CONFLICT(id) DO UPDATE SET
        workspace_id = excluded.workspace_id,
        secret_ref_id = excluded.secret_ref_id,
        base_url = excluded.base_url,
        enabled = excluded.enabled,
        config_version = tikhub_connector.config_version + 1,
        last_tested_at = NULL,
        last_test_status = NULL,
        updated_at = excluded.updated_at",
      params![
        workspace_id,
        secret_ref_id,
        base_url,
        bool_to_i64(input.enabled),
        now
      ],
    )
    .map_err(database_error)?;
  let connector = read_connector(&transaction)?
    .ok_or_else(|| database_error("TikHub 连接器写入后无法读取，请检查工作区数据库完整性"))?;
  write_connector_audit(
    &transaction,
    "save_tikhub_connector",
    serde_json::json!({
      "base_url": connector.base_url,
      "enabled": connector.enabled,
      "config_version": connector.config_version,
      "has_secret_ref": connector.secret_ref_id.is_some(),
    }),
    &now,
  )?;
  transaction.commit().map_err(database_error)?;
  Ok(connector)
}

pub fn test_tikhub_connector(root_path: impl AsRef<Path>) -> AppResult<TikhubConnectionTestResult> {
  let root_path = root_path.as_ref();
  let connector = get_tikhub_connector(root_path)?
    .ok_or_else(|| AppError::validation("尚未配置 TikHub 连接器", AppErrorStage::Collection))?;
  let result = if !connector.enabled {
    Err(AppError::validation(
      "TikHub 连接器尚未启用",
      AppErrorStage::Collection,
    ))
  } else if let Some(secret_ref_id) = connector.secret_ref_id.as_deref() {
    test_tikhub_connection(root_path, secret_ref_id, Some(connector.base_url.clone()))
  } else {
    Err(AppError::validation(
      "TikHub 连接器缺少密钥引用",
      AppErrorStage::Collection,
    ))
  };
  let status = if result.is_ok() { "success" } else { "failed" };
  let error_code = result
    .as_ref()
    .err()
    .map(|error| format!("{:?}", error.code));
  persist_connector_test_status(root_path, connector.config_version, status, error_code)?;
  result
}

pub fn test_tikhub_connection(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
  base_url: Option<String>,
) -> AppResult<TikhubConnectionTestResult> {
  let base_url = normalize_tikhub_base_url(base_url)?;
  let token = read_secret_for_backend(root_path, secret_ref_id, "tikhub")?;
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

fn open_connector_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
}

fn read_connector(connection: &Connection) -> AppResult<Option<TikhubConnectorView>> {
  let connector = connection
    .query_row(
      "SELECT id, workspace_id, secret_ref_id, base_url, enabled, config_version,
              last_tested_at, last_test_status, created_at, updated_at
       FROM tikhub_connector WHERE id = 'default'",
      [],
      map_connector,
    )
    .optional()
    .map_err(database_error)?;
  connector
    .map(|mut connector| {
      connector.base_url = normalize_tikhub_base_url(Some(connector.base_url))?;
      Ok(connector)
    })
    .transpose()
}

fn map_connector(row: &Row<'_>) -> rusqlite::Result<TikhubConnectorView> {
  Ok(TikhubConnectorView {
    id: row.get(0)?,
    workspace_id: row.get(1)?,
    secret_ref_id: row.get(2)?,
    base_url: row.get(3)?,
    enabled: row.get::<_, i64>(4)? != 0,
    config_version: row.get(5)?,
    last_tested_at: row.get(6)?,
    last_test_status: row.get(7)?,
    created_at: row.get(8)?,
    updated_at: row.get(9)?,
  })
}

fn persist_connector_test_status(
  root_path: impl AsRef<Path>,
  expected_config_version: i64,
  status: &str,
  error_code: Option<String>,
) -> AppResult<()> {
  let mut connection = open_connector_connection(root_path)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let tested_at = Utc::now().to_rfc3339();
  let changed = transaction
    .execute(
      "UPDATE tikhub_connector
       SET last_tested_at = ?1, last_test_status = ?2, updated_at = ?1
       WHERE id = 'default' AND config_version = ?3",
      params![tested_at, status, expected_config_version],
    )
    .map_err(database_error)?;
  if changed != 1 {
    return Err(AppError::validation(
      "TikHub 连接器配置已在测试期间变更，请重新测试",
      AppErrorStage::Collection,
    ));
  }
  write_connector_audit(
    &transaction,
    "test_tikhub_connector",
    serde_json::json!({
      "status": status,
      "config_version": expected_config_version,
      "error_code": error_code,
    }),
    &tested_at,
  )?;
  transaction.commit().map_err(database_error)
}

fn write_connector_audit(
  connection: &Connection,
  action: &str,
  safe_details: Value,
  created_at: &str,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO audit_log (
        id, entity_type, entity_id, action, safe_details_json, created_at
       ) VALUES (?1, 'tikhub_connector', 'default', ?2, ?3, ?4)",
      params![
        Uuid::new_v4().to_string(),
        action,
        safe_details.to_string(),
        created_at
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn bool_to_i64(value: bool) -> i64 {
  i64::from(value)
}

fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
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

  fn create_test_workspace(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("tikhub-connector-{}", uuid::Uuid::new_v4()));
    crate::workspace::create_workspace(name, &root).expect("workspace should be created");
    root
  }

  fn insert_secret_metadata(root: &Path, provider_type: &str, secret_store_key: &str) -> String {
    let connection =
      crate::workspace::open_workspace_database(root.join(crate::workspace::DATABASE_FILE_NAME))
        .expect("database should open");
    let secret_id = uuid::Uuid::new_v4().to_string();
    connection
      .execute(
        "INSERT INTO secret_ref (
          id, provider_type, provider_id, secret_store_key, masked_hint, created_at, updated_at
        ) VALUES (?1, ?2, 'test-provider', ?3, '[REDACTED]', ?4, ?4)",
        rusqlite::params![
          secret_id,
          provider_type,
          secret_store_key,
          "2026-07-13T00:00:00+00:00"
        ],
      )
      .expect("secret metadata should insert");
    secret_id
  }

  fn connector_input(secret_ref_id: Option<String>) -> TikhubConnectorInput {
    TikhubConnectorInput {
      secret_ref_id,
      base_url: "https://api.tikhub.dev/".to_string(),
      enabled: true,
    }
  }

  #[test]
  fn connector_is_absent_until_configured() {
    let root = create_test_workspace("TikHub 空配置测试");

    assert_eq!(
      get_tikhub_connector(&root).expect("connector lookup should work"),
      None
    );
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn enabled_connector_requires_a_tikhub_secret() {
    let root = create_test_workspace("TikHub 必填密钥测试");

    let error = save_tikhub_connector(&root, connector_input(None))
      .expect_err("enabled connector must require a secret");

    assert_eq!(error.code, AppErrorCode::ValidationError);
    assert!(get_tikhub_connector(&root)
      .expect("connector lookup should work")
      .is_none());
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn connector_rejects_a_secret_from_another_provider_type() {
    let root = create_test_workspace("TikHub 错误密钥类型测试");
    let secret_id = insert_secret_metadata(&root, "model_provider", "missing-model-key");

    let error = save_tikhub_connector(&root, connector_input(Some(secret_id)))
      .expect_err("a model provider secret must be rejected");

    assert_eq!(error.code, AppErrorCode::PermissionError);
    assert!(get_tikhub_connector(&root)
      .expect("connector lookup should work")
      .is_none());
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn connector_upsert_normalizes_url_and_increments_version() {
    let root = create_test_workspace("TikHub 单例更新测试");
    let secret_id = insert_secret_metadata(&root, "tikhub", "missing-tikhub-key");

    let created = save_tikhub_connector(&root, connector_input(Some(secret_id.clone())))
      .expect("connector should save");
    let updated = save_tikhub_connector(
      &root,
      TikhubConnectorInput {
        secret_ref_id: None,
        base_url: "https://api.tikhub.io///".to_string(),
        enabled: false,
      },
    )
    .expect("disabled connector should allow no secret");

    assert_eq!(created.id, "default");
    assert_eq!(created.base_url, CHINA_BASE_URL);
    assert_eq!(created.config_version, 1);
    assert_eq!(updated.id, "default");
    assert_eq!(updated.base_url, DEFAULT_BASE_URL);
    assert_eq!(updated.config_version, 2);
    assert_eq!(updated.created_at, created.created_at);
    assert_eq!(updated.secret_ref_id, None);
    assert!(!updated.enabled);

    let connection =
      crate::workspace::open_workspace_database(root.join(crate::workspace::DATABASE_FILE_NAME))
        .expect("database should open");
    let count: i64 = connection
      .query_row("SELECT COUNT(*) FROM tikhub_connector", [], |row| {
        row.get(0)
      })
      .expect("connector count should query");
    assert_eq!(count, 1);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn connector_change_clears_previous_test_status() {
    let root = create_test_workspace("TikHub 测试状态清理测试");
    let secret_id = insert_secret_metadata(&root, "tikhub", "missing-tikhub-key");
    save_tikhub_connector(&root, connector_input(Some(secret_id.clone())))
      .expect("connector should save");
    let connection =
      crate::workspace::open_workspace_database(root.join(crate::workspace::DATABASE_FILE_NAME))
        .expect("database should open");
    connection
      .execute(
        "UPDATE tikhub_connector
         SET last_tested_at = '2026-07-13T01:00:00+00:00', last_test_status = 'success'",
        [],
      )
      .expect("test status should update");
    drop(connection);

    let updated = save_tikhub_connector(&root, connector_input(Some(secret_id)))
      .expect("connector should update");

    assert_eq!(updated.last_tested_at, None);
    assert_eq!(updated.last_test_status, None);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn failed_connector_test_is_persisted_before_returning_the_original_error() {
    let root = create_test_workspace("TikHub 失败状态测试");
    let secret_id = insert_secret_metadata(
      &root,
      "model_provider",
      "token-body-that-must-never-reach-audit",
    );
    let connection =
      crate::workspace::open_workspace_database(root.join(crate::workspace::DATABASE_FILE_NAME))
        .expect("database should open");
    let workspace_id: String = connection
      .query_row("SELECT id FROM workspace", [], |row| row.get(0))
      .expect("workspace id should query");
    connection
      .execute(
        "INSERT INTO tikhub_connector (
          id, workspace_id, secret_ref_id, base_url, enabled, config_version,
          last_tested_at, last_test_status, created_at, updated_at
        ) VALUES ('default', ?1, ?2, ?3, 1, 1, NULL, NULL, ?4, ?4)",
        rusqlite::params![
          workspace_id,
          secret_id,
          DEFAULT_BASE_URL,
          "2026-07-13T00:00:00+00:00"
        ],
      )
      .expect("invalid connector fixture should insert");
    drop(connection);

    let error = test_tikhub_connector(&root)
      .expect_err("provider mismatch should fail before any network access");
    let connector = get_tikhub_connector(&root)
      .expect("connector lookup should work")
      .expect("connector should remain present");

    assert_eq!(error.code, AppErrorCode::PermissionError);
    assert_eq!(connector.last_test_status.as_deref(), Some("failed"));
    assert!(connector.last_tested_at.is_some());

    let connection =
      crate::workspace::open_workspace_database(root.join(crate::workspace::DATABASE_FILE_NAME))
        .expect("database should open");
    let audit_details: String = connection
      .query_row(
        "SELECT safe_details_json FROM audit_log
         WHERE entity_type = 'tikhub_connector' AND action = 'test_tikhub_connector'
         ORDER BY created_at DESC LIMIT 1",
        [],
        |row| row.get(0),
      )
      .expect("failed test audit should exist");
    assert!(!audit_details.contains("token-body-that-must-never-reach-audit"));
    assert!(!audit_details.contains("missing-model-key"));
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn connector_save_audit_never_contains_secret_storage_content() {
    let root = create_test_workspace("TikHub 安全审计测试");
    let forbidden = "raw-token-content-that-must-stay-out-of-audit";
    let secret_id = insert_secret_metadata(&root, "tikhub", forbidden);

    save_tikhub_connector(&root, connector_input(Some(secret_id))).expect("connector should save");

    let connection =
      crate::workspace::open_workspace_database(root.join(crate::workspace::DATABASE_FILE_NAME))
        .expect("database should open");
    let audit_details: String = connection
      .query_row(
        "SELECT safe_details_json FROM audit_log
         WHERE entity_type = 'tikhub_connector' AND action = 'save_tikhub_connector'
         ORDER BY created_at DESC LIMIT 1",
        [],
        |row| row.get(0),
      )
      .expect("save audit should exist");
    assert!(!audit_details.contains(forbidden));
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn rejects_non_tikhub_secret_reference_before_reading_the_keychain() {
    let root = std::env::temp_dir().join(format!("tikhub-secret-type-{}", uuid::Uuid::new_v4()));
    crate::workspace::create_workspace("TikHub 密钥类型测试", &root)
      .expect("workspace should be created");
    let connection =
      crate::workspace::open_workspace_database(root.join(crate::workspace::DATABASE_FILE_NAME))
        .expect("database should open");
    let secret_id = uuid::Uuid::new_v4().to_string();
    connection
      .execute(
        "INSERT INTO secret_ref (
          id, provider_type, provider_id, secret_store_key, masked_hint, created_at, updated_at
        ) VALUES (?1, 'model_provider', 'openai', 'missing-test-key', '[REDACTED]', ?2, ?2)",
        rusqlite::params![secret_id, "2026-07-13T00:00:00+00:00"],
      )
      .expect("wrong-type secret metadata should insert");

    let error = test_tikhub_connection(&root, &secret_id, None)
      .expect_err("a model provider secret must never be sent to TikHub");

    assert_eq!(error.code, AppErrorCode::PermissionError);
    assert!(error.message.contains("密钥类型"));
    std::fs::remove_dir_all(root).ok();
  }

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
