use std::path::Path;

use chrono::Utc;
use keyring::Entry;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

const KEYCHAIN_SERVICE: &str = "com.steven.sortlytic";

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
  let root_path = root_path.as_ref();
  let connection = open_workspace_connection(root_path)?;
  let secret_ref_id = Uuid::new_v4().to_string();
  let workspace_scope = workspace_secret_scope(root_path, &connection)?;
  let secret_store_key = build_secret_store_key(
    &workspace_scope,
    &provider_type,
    &provider_id,
    &secret_ref_id,
  );
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
  let root_path = root_path.as_ref();
  let connection = open_workspace_connection(root_path)?;
  let workspace_scope = workspace_secret_scope(root_path, &connection)?;
  let metadata = load_secret_metadata(&connection, secret_ref_id)?;
  let expected_store_key = build_secret_store_key(
    &workspace_scope,
    &metadata.provider_type,
    &metadata.provider_id,
    &metadata.id,
  );
  let security_rebound = metadata.secret_store_key != expected_store_key;
  let now = Utc::now().to_rfc3339();

  keychain_entry(&expected_store_key)?
    .set_password(&secret)
    .map_err(secret_store_error)?;

  connection
    .execute(
      "UPDATE secret_ref
       SET secret_store_key = ?1, masked_hint = ?2, updated_at = ?3,
           last_tested_at = NULL, last_test_status = NULL
       WHERE id = ?4",
      params![expected_store_key, mask_secret(&secret), now, secret_ref_id],
    )
    .map_err(database_error)?;

  write_secret_audit_log(
    &connection,
    "update_secret",
    Some(secret_ref_id),
    serde_json::json!({
      "provider_type": metadata.provider_type,
      "security_rebound": security_rebound,
    }),
  )?;

  get_secret_ref(&connection, secret_ref_id)
}

pub fn delete_secret(root_path: impl AsRef<Path>, secret_ref_id: &str) -> AppResult<bool> {
  let root_path = root_path.as_ref();
  let connection = open_workspace_connection(root_path)?;
  let workspace_scope = workspace_secret_scope(root_path, &connection)?;
  let metadata = get_secret_metadata(&connection, secret_ref_id, &workspace_scope)?;

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
  let root_path = root_path.as_ref();
  let connection = open_workspace_connection(root_path)?;
  let workspace_scope = workspace_secret_scope(root_path, &connection)?;
  let metadata = get_secret_metadata(&connection, secret_ref_id, &workspace_scope)?;
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
  let root_path = root_path.as_ref();
  let connection = open_workspace_connection(root_path)?;
  let workspace_scope = workspace_secret_scope(root_path, &connection)?;
  let metadata = load_secret_metadata(&connection, secret_ref_id)?;
  ensure_provider_type(&metadata, &expected_provider_type)?;
  ensure_secret_store_key(&metadata, &workspace_scope)?;
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
  let metadata = load_secret_metadata(connection, secret_ref_id)?;
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

fn build_secret_store_key(
  workspace_scope: &str,
  provider_type: &str,
  provider_id: &str,
  secret_ref_id: &str,
) -> String {
  let digest = hash_components(
    "sortlytic-secret-store-v2",
    &[workspace_scope, provider_type, provider_id, secret_ref_id],
  );
  format!("sortlytic:v2:{digest}")
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

fn get_secret_metadata(
  connection: &Connection,
  secret_ref_id: &str,
  workspace_scope: &str,
) -> AppResult<SecretMetadata> {
  let metadata = load_secret_metadata(connection, secret_ref_id)?;
  ensure_secret_store_key(&metadata, workspace_scope)?;
  Ok(metadata)
}

fn load_secret_metadata(connection: &Connection, secret_ref_id: &str) -> AppResult<SecretMetadata> {
  connection
    .query_row(
      "SELECT id, provider_type, provider_id, secret_store_key FROM secret_ref WHERE id = ?1",
      params![secret_ref_id],
      |row| {
        Ok(SecretMetadata {
          id: row.get(0)?,
          provider_type: row.get(1)?,
          provider_id: row.get(2)?,
          secret_store_key: row.get(3)?,
        })
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| secret_store_error("密钥引用不存在"))
}

fn ensure_secret_store_key(metadata: &SecretMetadata, workspace_scope: &str) -> AppResult<()> {
  let expected = build_secret_store_key(
    workspace_scope,
    &metadata.provider_type,
    &metadata.provider_id,
    &metadata.id,
  );
  if metadata.secret_store_key == expected {
    return Ok(());
  }

  if metadata.secret_store_key == build_legacy_secret_store_key(metadata) {
    return Err(AppError::new(
      AppErrorCode::PermissionError,
      "检测到旧版密钥引用；为防止跨工作区读取，请重新输入密钥完成安全重绑",
      AppErrorStage::SecretStore,
      false,
    ));
  }

  Err(AppError::new(
    AppErrorCode::PermissionError,
    "密钥引用与当前工作区不匹配，已拒绝访问系统安全存储",
    AppErrorStage::SecretStore,
    false,
  ))
}

fn build_legacy_secret_store_key(metadata: &SecretMetadata) -> String {
  format!(
    "sortlytic:{}:{}:{}",
    metadata.provider_type, metadata.provider_id, metadata.id
  )
}

fn workspace_secret_scope(root_path: &Path, connection: &Connection) -> AppResult<String> {
  let (count, workspace_id, registered_root) = connection
    .query_row(
      "SELECT COUNT(*), MIN(id), MIN(root_path) FROM workspace",
      [],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .map_err(database_error)?;

  match (count, workspace_id, registered_root) {
    (1, Some(workspace_id), Some(registered_root)) => {
      let canonical_root = std::fs::canonicalize(root_path).map_err(secret_store_error)?;
      let canonical_registered = std::fs::canonicalize(&registered_root).map_err(|_| {
        AppError::new(
          AppErrorCode::PermissionError,
          "工作区登记路径无法验证，已拒绝派生系统密钥作用域",
          AppErrorStage::SecretStore,
          false,
        )
      })?;
      if canonical_root != canonical_registered {
        return Err(AppError::new(
          AppErrorCode::PermissionError,
          "当前工作区路径与数据库登记路径不一致，已拒绝访问系统安全存储",
          AppErrorStage::SecretStore,
          false,
        ));
      }
      let canonical_root = canonical_root.to_string_lossy();
      Ok(hash_components(
        "sortlytic-workspace-scope-v1",
        &[&workspace_id, &canonical_root],
      ))
    }
    _ => Err(database_error(
      "工作区元数据必须恰好包含一条完整记录，无法派生密钥作用域",
    )),
  }
}

fn hash_components(domain: &str, components: &[&str]) -> String {
  let mut digest = Sha256::new();
  digest.update(domain.as_bytes());
  for component in components {
    digest.update((component.len() as u64).to_be_bytes());
    digest.update(component.as_bytes());
  }
  format!("{:x}", digest.finalize())
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
  id: String,
  provider_type: String,
  provider_id: String,
  secret_store_key: String,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::workspace::create_workspace;

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
    let key = build_secret_store_key(
      "workspace-scope",
      "model_provider",
      "openai",
      "secret-ref-id",
    );

    assert!(key.starts_with("sortlytic:v2:"));
    assert_eq!(key.len(), "sortlytic:v2:".len() + 64);
    assert!(!key.contains("sk-"));
  }

  #[test]
  fn secret_store_keys_are_isolated_by_workspace_scope() {
    let first = build_secret_store_key("workspace-a", "tikhub", "default", "secret-ref-id");
    let second = build_secret_store_key("workspace-b", "tikhub", "default", "secret-ref-id");

    assert_ne!(first, second);
  }

  #[test]
  fn rejects_a_tampered_secret_store_key_before_keychain_access() {
    let root_path = std::env::temp_dir().join(format!(
      "sortlytic-secret-tamper-{}",
      Uuid::new_v4()
    ));
    create_workspace("密钥隔离测试", &root_path).expect("workspace should be created");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    connection
      .execute(
        "INSERT INTO secret_ref (
          id, provider_type, provider_id, secret_store_key, masked_hint, created_at, updated_at
        ) VALUES ('tampered-secret', 'tikhub', 'default',
                  'sortlytic:tikhub:default:foreign-secret',
                  '[REDACTED]', '2026-07-13T00:00:00Z', '2026-07-13T00:00:00Z')",
        [],
      )
      .expect("tampered metadata should be inserted as an adversarial fixture");

    let workspace_scope =
      workspace_secret_scope(&root_path, &connection).expect("workspace scope should derive");
    let error = get_secret_metadata(&connection, "tampered-secret", &workspace_scope)
      .expect_err("editable SQLite metadata must not select an arbitrary Keychain account");

    assert_eq!(error.code, AppErrorCode::PermissionError);
    std::fs::remove_dir_all(root_path).ok();
  }

  #[test]
  fn published_legacy_secret_reference_requires_an_explicit_rebind() {
    let root_path = std::env::temp_dir().join(format!(
      "sortlytic-secret-legacy-{}",
      Uuid::new_v4()
    ));
    create_workspace("旧密钥升级测试", &root_path).expect("workspace should be created");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    connection
      .execute(
        "INSERT INTO secret_ref (
          id, provider_type, provider_id, secret_store_key, masked_hint, created_at, updated_at
        ) VALUES ('legacy-secret', 'tikhub', 'default',
                  'sortlytic:tikhub:default:legacy-secret',
                  '[REDACTED]', '2026-07-07T00:00:00Z', '2026-07-07T00:00:00Z')",
        [],
      )
      .expect("published legacy metadata should insert");
    let workspace_scope =
      workspace_secret_scope(&root_path, &connection).expect("workspace scope should derive");

    let error = get_secret_metadata(&connection, "legacy-secret", &workspace_scope)
      .expect_err("legacy account must not be read automatically from editable SQLite metadata");

    assert_eq!(error.code, AppErrorCode::PermissionError);
    assert!(error.message.contains("重新输入"));
    std::fs::remove_dir_all(root_path).ok();
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn updating_a_legacy_reference_rebinds_it_without_reading_the_old_account() {
    let root_path = std::env::temp_dir().join(format!(
      "sortlytic-secret-rebind-{}",
      Uuid::new_v4()
    ));
    create_workspace("旧密钥重绑测试", &root_path).expect("workspace should be created");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    let secret_ref_id = format!("legacy-secret-{}", Uuid::new_v4());
    let legacy_key = format!("sortlytic:tikhub:default:{secret_ref_id}");
    connection
      .execute(
        "INSERT INTO secret_ref (
          id, provider_type, provider_id, secret_store_key, masked_hint, created_at, updated_at
        ) VALUES (?1, 'tikhub', 'default', ?2, '[REDACTED]', ?3, ?3)",
        params![secret_ref_id, legacy_key, "2026-07-07T00:00:00Z"],
      )
      .expect("published legacy metadata should insert");
    drop(connection);

    update_secret(&root_path, &secret_ref_id, "replacement-secret-value")
      .expect("user-supplied replacement should securely rebind the reference");
    let stored = read_secret_for_backend(&root_path, &secret_ref_id, "tikhub")
      .expect("rebound secret should be readable through the scoped account");

    assert_eq!(stored, "replacement-secret-value");
    delete_secret(&root_path, &secret_ref_id).expect("rebound secret should clean up");
    std::fs::remove_dir_all(root_path).ok();
  }

  #[test]
  fn workspace_scope_rejects_a_database_registered_for_another_root() {
    let root_path = std::env::temp_dir().join(format!(
      "sortlytic-secret-root-{}",
      Uuid::new_v4()
    ));
    let other_root = std::env::temp_dir().join(format!(
      "sortlytic-secret-other-root-{}",
      Uuid::new_v4()
    ));
    create_workspace("密钥路径测试", &root_path).expect("workspace should be created");
    std::fs::create_dir_all(&other_root).expect("other root should exist");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    connection
      .execute(
        "UPDATE workspace SET root_path = ?1",
        params![other_root.to_string_lossy()],
      )
      .expect("adversarial fixture should alter the registered root");

    let error = workspace_secret_scope(&root_path, &connection)
      .expect_err("the live root must participate in workspace isolation");

    assert_eq!(error.code, AppErrorCode::PermissionError);
    std::fs::remove_dir_all(root_path).ok();
    std::fs::remove_dir_all(other_root).ok();
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn system_keychain_round_trip_works_for_app_credentials() {
    let secret_store_key = build_secret_store_key(
      "system-keychain-test-workspace",
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
