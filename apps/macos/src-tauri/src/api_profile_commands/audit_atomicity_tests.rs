use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::json;
use uuid::Uuid;

use super::{service, AiApiFormat, AiProviderType, ApiProfileKind, SaveApiProfileInput};
use crate::tikhub::TikhubConnectionTestResult;
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

const NEW_SECRET: &str = "sk-audit-atomicity-new-secret-123456789";
const EXISTING_SECRET: &str = "sk-audit-atomicity-existing-secret-987654321";
const TIKHUB_SECRET: &str = "tk-audit-atomicity-existing-secret-456789123";

#[derive(Debug, PartialEq, Eq)]
struct MirrorSnapshot {
  tikhub_connectors: Vec<String>,
  model_providers: Vec<String>,
  model_profiles: Vec<String>,
  secret_refs: Vec<String>,
}

fn workspace(label: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!("api-audit-{label}-{}", Uuid::new_v4()));
  create_workspace("API 审计原子性测试", &root).unwrap();
  root
}

fn ai_input(name: &str, key: &str) -> SaveApiProfileInput {
  SaveApiProfileInput::Ai {
    id: None,
    name: name.to_string(),
    provider_type: AiProviderType::Openai,
    api_format: AiApiFormat::OpenaiCompatible,
    base_url: "https://api.openai.com/v1".to_string(),
    default_model_id: "gpt-audit-test".to_string(),
    api_key: Some(key.to_string()),
  }
}

fn tikhub_input() -> SaveApiProfileInput {
  SaveApiProfileInput::Tikhub {
    id: None,
    name: "TikHub 审计测试".to_string(),
    base_url: "https://api.tikhub.io".to_string(),
    api_key: Some(TIKHUB_SECRET.to_string()),
  }
}

fn successful_tikhub_test() -> TikhubConnectionTestResult {
  TikhubConnectionTestResult {
    success: true,
    base_url: "https://api.tikhub.io".to_string(),
    masked_email: Some("a***t@example.test".to_string()),
    balance: Some(4.0),
    free_credit: Some(1.0),
    available_credit: Some(5.0),
    email_verified: Some(true),
    api_key_status: Some(1),
    daily_usage_json: json!({"data":{"total_requests":12}}),
    message: "TikHub Token 可用".to_string(),
  }
}

fn install_failing_audit_trigger(root: &Path) {
  open_workspace_database(root.join(DATABASE_FILE_NAME))
    .unwrap()
    .execute_batch(
      "CREATE TRIGGER fail_api_profile_audit_insert
       BEFORE INSERT ON audit_log
       WHEN NEW.entity_type = 'api_profile'
       BEGIN
         SELECT RAISE(FAIL, 'API profile audit insert blocked');
       END;",
    )
    .unwrap();
}

fn registry_bytes(root: &Path) -> Vec<u8> {
  fs::read(root.join("secrets/api-config.json")).unwrap()
}

fn rows(connection: &Connection, sql: &str) -> Vec<String> {
  connection
    .prepare(sql)
    .unwrap()
    .query_map([], |row| row.get(0))
    .unwrap()
    .collect::<Result<Vec<String>, _>>()
    .unwrap()
}

fn mirror_snapshot(root: &Path) -> MirrorSnapshot {
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  MirrorSnapshot {
    tikhub_connectors: rows(
      &connection,
      "SELECT id || '|' || secret_ref_id || '|' || base_url || '|' || enabled || '|' ||
              config_version || '|' || last_test_status
       FROM tikhub_connector ORDER BY id",
    ),
    model_providers: rows(
      &connection,
      "SELECT id || '|' || provider_id || '|' || display_name || '|' || enabled || '|' ||
              COALESCE(secret_ref_id, '') || '|' || COALESCE(base_url, '') || '|' ||
              COALESCE(default_model_id, '') || '|' || health_check_json
       FROM model_provider ORDER BY id",
    ),
    model_profiles: rows(
      &connection,
      "SELECT id || '|' || provider_id || '|' || model_id || '|' || enabled
       FROM model_profile ORDER BY id",
    ),
    secret_refs: rows(
      &connection,
      "SELECT id || '|' || provider_type || '|' || provider_id || '|' || alias || '|' ||
              masked_hint || '|' || last_test_status || '|' || credential_revision
       FROM secret_ref
       WHERE provider_type IN ('tikhub', 'model_provider')
       ORDER BY id",
    ),
  }
}

fn assert_failed_operation_is_atomic(
  root: &Path,
  operation: impl FnOnce() -> crate::domain::AppResult<()>,
) {
  let before_registry = registry_bytes(root);
  let before_mirror = mirror_snapshot(root);

  let error = operation().unwrap_err();

  assert_eq!(registry_bytes(root), before_registry);
  assert_eq!(mirror_snapshot(root), before_mirror);
  for secret in [NEW_SECRET, EXISTING_SECRET, TIKHUB_SECRET] {
    assert!(!error.message.contains(secret));
    let database = fs::read(root.join(DATABASE_FILE_NAME)).unwrap();
    assert!(!database
      .windows(secret.len())
      .any(|window| window == secret.as_bytes()));
  }
}

#[test]
fn save_keeps_json_and_mirror_unchanged_when_api_profile_audit_insert_fails() {
  let root = workspace("save");
  install_failing_audit_trigger(&root);

  assert_failed_operation_is_atomic(&root, || {
    service::save_profile(&root, ai_input("OpenAI 新配置", NEW_SECRET)).map(|_| ())
  });

  fs::remove_dir_all(root).ok();
}

#[test]
fn activate_keeps_json_and_mirror_unchanged_when_api_profile_audit_insert_fails() {
  let root = workspace("activate");
  let first = service::save_profile(&root, ai_input("OpenAI A", EXISTING_SECRET)).unwrap();
  let first_id = first.ai_profiles.values().next().unwrap().id.clone();
  service::test_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
  let second =
    service::save_profile(&root, ai_input("OpenAI B", "sk-audit-second-123456789")).unwrap();
  let second_id = second
    .ai_profiles
    .values()
    .find(|profile| profile.name == "OpenAI B")
    .unwrap()
    .id
    .clone();
  service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  install_failing_audit_trigger(&root);

  assert_failed_operation_is_atomic(&root, || {
    service::activate_profile(&root, ApiProfileKind::Ai, &second_id).map(|_| ())
  });

  fs::remove_dir_all(root).ok();
}

#[test]
fn delete_keeps_json_and_mirror_unchanged_when_api_profile_audit_insert_fails() {
  let root = workspace("delete");
  let registry = service::save_profile(&root, ai_input("OpenAI 待删除", EXISTING_SECRET)).unwrap();
  let profile_id = registry.ai_profiles.values().next().unwrap().id.clone();
  install_failing_audit_trigger(&root);

  assert_failed_operation_is_atomic(&root, || {
    service::delete_profile(&root, ApiProfileKind::Ai, &profile_id).map(|_| ())
  });

  fs::remove_dir_all(root).ok();
}

#[test]
fn ai_test_keeps_json_and_mirror_unchanged_when_api_profile_audit_insert_fails() {
  let root = workspace("ai-test");
  let registry = service::save_profile(&root, ai_input("OpenAI 待校验", EXISTING_SECRET)).unwrap();
  let profile_id = registry.ai_profiles.values().next().unwrap().id.clone();
  install_failing_audit_trigger(&root);

  assert_failed_operation_is_atomic(&root, || {
    service::test_profile(&root, ApiProfileKind::Ai, &profile_id).map(|_| ())
  });

  fs::remove_dir_all(root).ok();
}

#[test]
fn tikhub_test_keeps_json_and_mirror_unchanged_when_api_profile_audit_insert_fails() {
  let root = workspace("tikhub-test");
  let registry = service::save_profile(&root, tikhub_input()).unwrap();
  let profile_id = registry.tikhub_profiles.values().next().unwrap().id.clone();
  install_failing_audit_trigger(&root);

  assert_failed_operation_is_atomic(&root, || {
    service::test_profile_with(&root, ApiProfileKind::Tikhub, &profile_id, |_, _, _| {
      Ok(successful_tikhub_test())
    })
    .map(|_| ())
  });

  fs::remove_dir_all(root).ok();
}
