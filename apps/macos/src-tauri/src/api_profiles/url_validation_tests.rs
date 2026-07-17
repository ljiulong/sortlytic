use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

const URL_SENTINEL: &str = "url-sensitive-sentinel-7193";

fn workspace() -> PathBuf {
  let root = std::env::temp_dir().join(format!("api-url-validation-{}", Uuid::new_v4()));
  fs::create_dir(&root).unwrap();
  fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
  create_workspace("AI URL 校验", &root).unwrap();
  root
}

fn registry_with_ai(base_url: &str, status: ApiProfileStatus) -> ApiProfileRegistry {
  let profile_id = Uuid::new_v4().to_string();
  let credential_id = Uuid::new_v4().to_string();
  let timestamp = Utc::now().to_rfc3339();
  let mut registry = ApiProfileRegistry::default();
  registry.ai_profiles.insert(
    profile_id.clone(),
    AiApiProfile {
      id: profile_id.clone(),
      name: "自定义 AI".to_string(),
      provider_type: AiProviderType::CustomOpenaiCompatible,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url: base_url.to_string(),
      default_model_id: "model-test".to_string(),
      credential_ref_id: Some(credential_id.clone()),
      revision: 1,
      status,
      last_tested_at: None,
      created_at: timestamp.clone(),
      updated_at: timestamp,
    },
  );
  registry.credentials.insert(
    credential_id.clone(),
    ApiCredential {
      id: credential_id,
      provider_type: CredentialProviderType::CustomOpenaiCompatible,
      profile_id,
      revision: 1,
      secret: "test-key".to_string(),
    },
  );
  registry
}

#[test]
fn save_rejects_sensitive_ai_url_components_without_persisting_them() {
  let root = workspace();
  let json_path = api_profile_registry_path(&root);
  let database_path = root.join(DATABASE_FILE_NAME);
  let original_json = fs::read(&json_path).unwrap();
  let original_database = fs::read(&database_path).unwrap();

  for url in [
    format!("https://{URL_SENTINEL}@example.test/v1"),
    format!("https://user:{URL_SENTINEL}@example.test/v1"),
    format!("https://example.test/v1?token={URL_SENTINEL}"),
    format!("https://example.test/v1#{URL_SENTINEL}"),
  ] {
    let error =
      save_api_profile_registry(&root, &registry_with_ai(&url, ApiProfileStatus::Success))
        .unwrap_err();
    assert!(!error.message.contains(URL_SENTINEL));
  }

  let json = fs::read(&json_path).unwrap();
  let database = fs::read(&database_path).unwrap();
  assert_eq!(json, original_json);
  assert_eq!(database, original_database);
  assert!(!json
    .windows(URL_SENTINEL.len())
    .any(|part| part == URL_SENTINEL.as_bytes()));
  assert!(!database
    .windows(URL_SENTINEL.len())
    .any(|part| part == URL_SENTINEL.as_bytes()));
  fs::remove_dir_all(root).ok();
}

#[test]
fn load_rejects_an_ai_url_with_forbidden_components() {
  let root = workspace();
  let registry = registry_with_ai(
    "https://example.test/v1?unsafe=true",
    ApiProfileStatus::Success,
  );
  fs::write(
    api_profile_registry_path(&root),
    serde_json::to_vec_pretty(&registry).unwrap(),
  )
  .unwrap();

  let error = load_api_profile_registry(&root).unwrap_err();
  assert!(!error.message.contains("unsafe=true"));
  fs::remove_dir_all(root).ok();
}

#[test]
fn legacy_import_rejects_an_ai_url_with_forbidden_components() {
  let root = workspace();
  fs::remove_file(api_profile_registry_path(&root)).unwrap();
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  let timestamp = Utc::now().to_rfc3339();
  connection
    .execute(
      "INSERT INTO model_provider (
         id, provider_id, display_name, enabled, auth_type, base_url,
         api_format, default_model_id, created_at, updated_at
       ) VALUES (?1, 'custom-openai', '旧端点', 0, 'api_key',
                 'https://example.test/v1#unsafe', 'openai_compatible', 'legacy-model', ?2, ?2)",
      params![Uuid::new_v4().to_string(), timestamp],
    )
    .unwrap();
  drop(connection);

  let error = initialize_api_profile_registry(&root).unwrap_err();
  assert!(!error.message.contains("#unsafe"));
  assert!(!api_profile_registry_path(&root).exists());
  fs::remove_dir_all(root).ok();
}

#[test]
fn needs_rebind_custom_ai_profile_allows_an_empty_url() {
  let root = workspace();
  let mut registry = registry_with_ai("", ApiProfileStatus::NeedsRebind);
  registry.credentials.clear();

  save_api_profile_registry(&root, &registry).unwrap();
  assert_eq!(load_api_profile_registry(&root).unwrap(), registry);
  fs::remove_dir_all(root).ok();
}
