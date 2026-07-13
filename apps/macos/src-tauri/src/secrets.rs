use std::path::Path;

use chrono::Utc;
use keyring::Entry;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

const KEYCHAIN_SERVICE: &str = "com.steven.smart-data-workbench";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretRefView {
  pub id: String,
  pub provider_type: String,
  pub provider_id: String,
  pub alias: Option<String>,
  pub masked_hint: String,
  pub created_at: String,
  pub updated_at: String,
  pub last_tested_at: Option<String>,
  pub last_test_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretConnectionTestResult {
  pub secret_ref_id: String,
  pub success: bool,
  pub message: String,
  pub tested_at: String,
}

pub fn save_secret(
  root_path: impl AsRef<Path>,
  provider_type: &str,
  provider_id: &str,
  secret: &str,
  alias: Option<String>,
) -> AppResult<SecretRefView> {
  let provider_type = normalize_provider_type(provider_type)?;
  let provider_id = normalize_provider_id(provider_id)?;
  let secret = normalize_secret(secret)?;
  let alias = normalize_alias(alias);
  let connection = open_workspace_connection(root_path)?;
  let secret_ref_id = Uuid::new_v4().to_string();
  let secret_store_key = build_secret_store_key(&provider_type, &provider_id, &secret_ref_id);
  let now = Utc::now().to_rfc3339();

  keychain_entry(&secret_store_key)?
    .set_password(&secret)
    .map_err(secret_store_error)?;

  let insert_result = connection.execute(
    "INSERT INTO secret_ref (
      id, provider_type, provider_id, alias, secret_store_key, masked_hint,
      created_at, updated_at, last_tested_at, last_test_status
    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, NULL)",
    params![
      secret_ref_id,
      provider_type,
      provider_id,
      alias,
      secret_store_key,
      mask_secret(&secret),
      now,
      now
    ],
  );

  if let Err(error) = insert_result {
    let _ = keychain_entry(&secret_store_key).and_then(|entry| {
      entry.delete_credential().map_err(secret_store_error)?;
      Ok(())
    });
    return Err(database_error(error));
  }

  write_secret_audit_log(
    &connection,
    "save_secret",
    Some(&secret_ref_id),
    serde_json::json!({
      "provider_type": provider_type,
      "provider_id": provider_id,
      "alias": alias,
    }),
  )?;

  get_secret_ref(&connection, &secret_ref_id)
}

pub fn update_secret(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
  secret: &str,
) -> AppResult<SecretRefView> {
  let secret = normalize_secret(secret)?;
  let connection = open_workspace_connection(root_path)?;
  let metadata = get_secret_metadata(&connection, secret_ref_id)?;
  let now = Utc::now().to_rfc3339();

  keychain_entry(&metadata.secret_store_key)?
    .set_password(&secret)
    .map_err(secret_store_error)?;

  connection
    .execute(
      "UPDATE secret_ref
       SET masked_hint = ?1, updated_at = ?2, last_tested_at = NULL, last_test_status = NULL
       WHERE id = ?3",
      params![mask_secret(&secret), now, secret_ref_id],
    )
    .map_err(database_error)?;

  write_secret_audit_log(
    &connection,
    "update_secret",
    Some(secret_ref_id),
    serde_json::json!({ "provider_type": metadata.provider_type }),
  )?;

  get_secret_ref(&connection, secret_ref_id)
}

pub fn delete_secret(root_path: impl AsRef<Path>, secret_ref_id: &str) -> AppResult<bool> {
  let connection = open_workspace_connection(root_path)?;
  let metadata = get_secret_metadata(&connection, secret_ref_id)?;

  keychain_entry(&metadata.secret_store_key)?
    .delete_credential()
    .map_err(secret_store_error)?;

  connection
    .execute(
      "DELETE FROM secret_ref WHERE id = ?1",
      params![secret_ref_id],
    )
    .map_err(database_error)?;

  write_secret_audit_log(
    &connection,
    "delete_secret",
    Some(secret_ref_id),
    serde_json::json!({ "provider_type": metadata.provider_type }),
  )?;

  Ok(true)
}

pub fn list_secret_refs(
  root_path: impl AsRef<Path>,
  provider_type: Option<String>,
) -> AppResult<Vec<SecretRefView>> {
  let connection = open_workspace_connection(root_path)?;

  if let Some(provider_type) = provider_type {
    let provider_type = normalize_provider_type(&provider_type)?;
    let mut statement = connection
      .prepare(
        "SELECT id, provider_type, provider_id, alias, masked_hint,
                created_at, updated_at, last_tested_at, last_test_status
         FROM secret_ref
         WHERE provider_type = ?1
         ORDER BY updated_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![provider_type], map_secret_ref_view)
      .map_err(database_error)?;
    collect_secret_rows(rows)
  } else {
    let mut statement = connection
      .prepare(
        "SELECT id, provider_type, provider_id, alias, masked_hint,
                created_at, updated_at, last_tested_at, last_test_status
         FROM secret_ref
         ORDER BY updated_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map([], map_secret_ref_view)
      .map_err(database_error)?;
    collect_secret_rows(rows)
  }
}

pub fn test_secret_connection(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
) -> AppResult<SecretConnectionTestResult> {
  let connection = open_workspace_connection(root_path)?;
  let metadata = get_secret_metadata(&connection, secret_ref_id)?;
  let tested_at = Utc::now().to_rfc3339();
  let read_result = keychain_entry(&metadata.secret_store_key)?
    .get_password()
    .map(|secret| !secret.is_empty())
    .map_err(secret_store_error);

  let (success, status, message) = match read_result {
    Ok(true) => (true, "success", "密钥可从系统安全存储读取".to_string()),
    Ok(false) => (false, "failed", "系统安全存储中的密钥为空".to_string()),
    Err(error) => (false, "failed", error.message),
  };

  connection
    .execute(
      "UPDATE secret_ref
       SET last_tested_at = ?1, last_test_status = ?2, updated_at = ?1
       WHERE id = ?3",
      params![tested_at, status, secret_ref_id],
    )
    .map_err(database_error)?;

  Ok(SecretConnectionTestResult {
    secret_ref_id: secret_ref_id.to_string(),
    success,
    message,
    tested_at,
  })
}

pub fn read_secret_for_backend(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
  expected_provider_type: &str,
) -> AppResult<String> {
  let expected_provider_type = normalize_provider_type(expected_provider_type)?;
  let connection = open_workspace_connection(root_path)?;
  let metadata = get_secret_metadata(&connection, secret_ref_id)?;
  ensure_provider_type(&metadata, &expected_provider_type)?;
  keychain_entry(&metadata.secret_store_key)?
    .get_password()
    .map_err(secret_store_error)
}

pub(crate) fn validate_secret_ref_provider(
  connection: &Connection,
  secret_ref_id: &str,
  expected_provider_type: &str,
) -> AppResult<()> {
  let expected_provider_type = normalize_provider_type(expected_provider_type)?;
  let metadata = get_secret_metadata(connection, secret_ref_id)?;
  ensure_provider_type(&metadata, &expected_provider_type)
}

pub fn mask_secret(secret: &str) -> String {
  let chars = secret.chars().collect::<Vec<_>>();

  if chars.len() <= 8 {
    return "[REDACTED]".to_string();
  }

  let prefix = chars.iter().take(4).collect::<String>();
  let suffix = chars
    .iter()
    .rev()
    .take(4)
    .collect::<Vec<_>>()
    .into_iter()
    .rev()
    .collect::<String>();

  format!("{prefix}...[REDACTED]...{suffix}")
}

fn open_workspace_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
}

fn normalize_provider_type(provider_type: &str) -> AppResult<String> {
  let provider_type = provider_type.trim();

  match provider_type {
    "tikhub" | "model_provider" | "webhook" => Ok(provider_type.to_string()),
    _ => Err(AppError::validation(
      "密钥类型只支持 tikhub、model_provider 或 webhook",
      AppErrorStage::SecretStore,
    )),
  }
}

fn normalize_provider_id(provider_id: &str) -> AppResult<String> {
  let provider_id = provider_id.trim();

  if provider_id.is_empty() {
    return Err(AppError::validation(
      "密钥 provider_id 不能为空",
      AppErrorStage::SecretStore,
    ));
  }

  Ok(provider_id.to_string())
}

fn normalize_secret(secret: &str) -> AppResult<String> {
  let secret = secret.trim();

  if secret.is_empty() {
    return Err(AppError::validation(
      "密钥不能为空",
      AppErrorStage::SecretStore,
    ));
  }

  Ok(secret.to_string())
}

fn normalize_alias(alias: Option<String>) -> Option<String> {
  alias.and_then(|alias| {
    let alias = alias.trim().to_string();
    if alias.is_empty() {
      None
    } else {
      Some(alias)
    }
  })
}

fn build_secret_store_key(provider_type: &str, provider_id: &str, secret_ref_id: &str) -> String {
  format!("smart-data-workbench:{provider_type}:{provider_id}:{secret_ref_id}")
}

fn keychain_entry(secret_store_key: &str) -> AppResult<Entry> {
  ensure_keychain_store()?;
  Entry::new(KEYCHAIN_SERVICE, secret_store_key).map_err(secret_store_error)
}

#[cfg(target_os = "macos")]
fn ensure_keychain_store() -> AppResult<()> {
  let store = apple_native_keyring_store::keychain::Store::new().map_err(secret_store_error)?;
  keyring_core::set_default_store(store);
  Ok(())
}

#[cfg(not(target_os = "macos"))]
fn ensure_keychain_store() -> AppResult<()> {
  Ok(())
}

fn get_secret_ref(connection: &Connection, secret_ref_id: &str) -> AppResult<SecretRefView> {
  connection
    .query_row(
      "SELECT id, provider_type, provider_id, alias, masked_hint,
              created_at, updated_at, last_tested_at, last_test_status
       FROM secret_ref
       WHERE id = ?1",
      params![secret_ref_id],
      map_secret_ref_view,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| secret_store_error("密钥引用不存在"))
}

fn get_secret_metadata(connection: &Connection, secret_ref_id: &str) -> AppResult<SecretMetadata> {
  connection
    .query_row(
      "SELECT id, provider_type, secret_store_key FROM secret_ref WHERE id = ?1",
      params![secret_ref_id],
      |row| {
        Ok(SecretMetadata {
          provider_type: row.get(1)?,
          secret_store_key: row.get(2)?,
        })
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| secret_store_error("密钥引用不存在"))
}

fn ensure_provider_type(metadata: &SecretMetadata, expected_provider_type: &str) -> AppResult<()> {
  if metadata.provider_type == expected_provider_type {
    return Ok(());
  }
  Err(AppError::new(
    AppErrorCode::PermissionError,
    "密钥类型与当前调用目标不匹配",
    AppErrorStage::SecretStore,
    false,
  ))
}

fn map_secret_ref_view(row: &Row<'_>) -> rusqlite::Result<SecretRefView> {
  Ok(SecretRefView {
    id: row.get(0)?,
    provider_type: row.get(1)?,
    provider_id: row.get(2)?,
    alias: row.get(3)?,
    masked_hint: row.get(4)?,
    created_at: row.get(5)?,
    updated_at: row.get(6)?,
    last_tested_at: row.get(7)?,
    last_test_status: row.get(8)?,
  })
}

fn collect_secret_rows(
  rows: impl Iterator<Item = rusqlite::Result<SecretRefView>>,
) -> AppResult<Vec<SecretRefView>> {
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn write_secret_audit_log(
  connection: &Connection,
  action: &str,
  entity_id: Option<&str>,
  safe_details: serde_json::Value,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO audit_log (id, entity_type, entity_id, action, safe_details_json, created_at)
       VALUES (?1, 'secret_ref', ?2, ?3, ?4, ?5)",
      params![
        Uuid::new_v4().to_string(),
        entity_id,
        action,
        safe_details.to_string(),
        Utc::now().to_rfc3339()
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn secret_store_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::SecretStoreError,
    error.to_string(),
    AppErrorStage::SecretStore,
    false,
  )
}

fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
}

#[derive(Debug)]
struct SecretMetadata {
  provider_type: String,
  secret_store_key: String,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn masks_secret_without_returning_full_value() {
    let secret = "sk-1234567890abcdef";
    let masked = mask_secret(secret);

    assert_ne!(masked, secret);
    assert!(masked.starts_with("sk-1"));
    assert!(masked.ends_with("cdef"));
    assert!(!masked.contains("567890ab"));
  }

  #[test]
  fn short_secret_is_fully_redacted() {
    assert_eq!(mask_secret("short"), "[REDACTED]");
  }

  #[test]
  fn secret_store_key_does_not_include_secret_value() {
    let key = build_secret_store_key("model_provider", "openai", "secret-ref-id");

    assert_eq!(
      key,
      "smart-data-workbench:model_provider:openai:secret-ref-id"
    );
    assert!(!key.contains("sk-"));
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn system_keychain_round_trip_works_for_app_credentials() {
    let secret_store_key = build_secret_store_key(
      "tikhub",
      "connectivity-test",
      &format!("test-{}", Uuid::new_v4()),
    );
    let entry = keychain_entry(&secret_store_key).expect("keychain entry should initialize");

    entry
      .set_password("test-secret")
      .expect("test secret should be saved");
    assert_eq!(
      entry
        .get_password()
        .expect("test secret should be readable"),
      "test-secret"
    );
    entry
      .delete_credential()
      .expect("test secret should be deleted");
  }
}
