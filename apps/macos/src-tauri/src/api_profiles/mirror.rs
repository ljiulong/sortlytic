use std::collections::BTreeSet;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
  AiApiFormat, AiApiProfile, AiProviderType, ApiCredential, ApiProfileRegistry, ApiProfileStatus,
  TikhubApiProfile,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

pub(super) fn import_legacy_registry(root_path: &Path) -> AppResult<ApiProfileRegistry> {
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))?;
  let workspace_id: String = connection
    .query_row("SELECT id FROM workspace LIMIT 1", [], |row| row.get(0))
    .map_err(|error| database_error("无法读取工作区 ID", error))?;
  let mut registry = ApiProfileRegistry::default();

  if let Some(legacy) = read_legacy_tikhub(&connection)? {
    let profile_id = stable_uuid(&workspace_id, "tikhub-profile", "default");
    let credential_ref_id = legacy
      .secret_ref_id
      .as_deref()
      .map(|id| stable_uuid_or_existing(&workspace_id, "tikhub-credential", id))
      .unwrap_or_else(|| stable_uuid(&workspace_id, "tikhub-credential", "default"));
    registry.tikhub_profiles.insert(
      profile_id.clone(),
      TikhubApiProfile {
        id: profile_id,
        name: normalized_name(legacy.alias.as_deref(), "TikHub API"),
        base_url: normalized_tikhub_base_url(&legacy.base_url),
        credential_ref_id,
        revision: positive_revision(legacy.config_version),
        status: ApiProfileStatus::NeedsRebind,
        last_tested_at: valid_optional_timestamp(legacy.last_tested_at),
        test_summary: None,
        created_at: valid_timestamp_or_now(legacy.created_at),
        updated_at: valid_timestamp_or_now(legacy.updated_at),
      },
    );
  }

  let mut used_names = BTreeSet::new();
  for legacy in read_legacy_ai_profiles(&connection)? {
    let profile_id = stable_uuid_or_existing(
      &workspace_id,
      "ai-profile",
      if legacy.id.trim().is_empty() {
        &legacy.provider_id
      } else {
        &legacy.id
      },
    );
    let provider_type = infer_ai_provider_type(&legacy.provider_id, &legacy.api_format);
    let api_format = format_for_provider(provider_type);
    let credential_ref_id = if provider_type == AiProviderType::Ollama {
      legacy
        .secret_ref_id
        .as_deref()
        .map(|id| stable_uuid_or_existing(&workspace_id, "ai-credential", id))
    } else {
      Some(
        legacy
          .secret_ref_id
          .as_deref()
          .map(|id| stable_uuid_or_existing(&workspace_id, "ai-credential", id))
          .unwrap_or_else(|| stable_uuid(&workspace_id, "ai-credential", &legacy.provider_id)),
      )
    };
    let base_name = normalized_name(Some(&legacy.display_name), "AI API");
    let name = unique_name(&mut used_names, &base_name);
    registry.ai_profiles.insert(
      profile_id.clone(),
      AiApiProfile {
        id: profile_id,
        name,
        provider_type,
        api_format,
        base_url: normalized_ai_base_url(provider_type, legacy.base_url.as_deref()),
        default_model_id: legacy.default_model_id.trim().to_string(),
        credential_ref_id,
        revision: 1,
        status: ApiProfileStatus::NeedsRebind,
        last_tested_at: None,
        created_at: valid_timestamp_or_now(legacy.created_at),
        updated_at: valid_timestamp_or_now(legacy.updated_at),
      },
    );
  }

  Ok(registry)
}

pub(super) fn mirror_registry(root_path: &Path, registry: &ApiProfileRegistry) -> AppResult<()> {
  let mut connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(|error| database_error("无法开始 API 配置镜像事务", error))?;
  rebuild_mirror(&transaction, registry)?;
  transaction
    .commit()
    .map_err(|error| database_error("无法提交 API 配置镜像", error))
}

pub(super) fn rebuild_mirror(
  transaction: &Transaction<'_>,
  registry: &ApiProfileRegistry,
) -> AppResult<()> {
  transaction
    .execute("DELETE FROM tikhub_connector", [])
    .map_err(|error| database_error("无法清理 TikHub 派生镜像", error))?;
  transaction
    .execute("DELETE FROM model_provider", [])
    .map_err(|error| database_error("无法清理 AI 派生镜像", error))?;
  transaction
    .execute(
      "DELETE FROM secret_ref WHERE provider_type IN ('tikhub', 'model_provider')",
      [],
    )
    .map_err(|error| database_error("无法清理 API 密钥引用镜像", error))?;

  for profile in registry.tikhub_profiles.values() {
    insert_secret_mirror(
      transaction,
      registry.credentials.get(&profile.credential_ref_id),
      &profile.credential_ref_id,
      "tikhub",
      &profile.id,
      &profile.name,
      profile.revision,
      profile.last_tested_at.as_deref(),
      profile.status,
      &profile.created_at,
      &profile.updated_at,
    )?;
  }
  for profile in registry.ai_profiles.values() {
    if let Some(credential_ref_id) = profile.credential_ref_id.as_deref() {
      insert_secret_mirror(
        transaction,
        registry.credentials.get(credential_ref_id),
        credential_ref_id,
        "model_provider",
        &profile.id,
        &profile.name,
        profile.revision,
        profile.last_tested_at.as_deref(),
        profile.status,
        &profile.created_at,
        &profile.updated_at,
      )?;
    }
  }

  mirror_tikhub_connector(transaction, registry)?;
  mirror_ai_providers(transaction, registry)?;
  let workspace_id: String = transaction
    .query_row("SELECT id FROM workspace LIMIT 1", [], |row| row.get(0))
    .map_err(|error| database_error("无法读取镜像审计工作区 ID", error))?;
  transaction
    .execute(
      "INSERT INTO audit_log (
         id, entity_type, entity_id, action, safe_details_json, created_at
       ) VALUES (?1, 'api_profile_registry', ?2, 'mirror_api_profiles', ?3, ?4)",
      params![
        Uuid::new_v4().to_string(),
        workspace_id,
        serde_json::json!({
          "schema_version": registry.schema_version,
          "tikhub_profile_count": registry.tikhub_profiles.len(),
          "ai_profile_count": registry.ai_profiles.len(),
          "has_active_tikhub": registry.active_profile_ids.tikhub.is_some(),
          "has_active_ai": registry.active_profile_ids.ai.is_some(),
        })
        .to_string(),
        Utc::now().to_rfc3339(),
      ],
    )
    .map_err(|error| database_error("无法写入 API 配置镜像审计", error))?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_secret_mirror(
  transaction: &Transaction<'_>,
  credential: Option<&ApiCredential>,
  credential_ref_id: &str,
  provider_type: &str,
  provider_id: &str,
  alias: &str,
  profile_revision: u64,
  last_tested_at: Option<&str>,
  status: ApiProfileStatus,
  created_at: &str,
  updated_at: &str,
) -> AppResult<()> {
  let credential_revision = credential
    .map(|credential| credential.revision)
    .unwrap_or(profile_revision);
  let masked_hint = credential
    .map(|credential| mask_secret(&credential.secret))
    .unwrap_or_else(|| "[NEEDS_REBIND]".to_string());
  transaction
    .execute(
      "INSERT INTO secret_ref (
         id, provider_type, provider_id, alias, secret_store_key, masked_hint,
         created_at, updated_at, last_tested_at, last_test_status, credential_revision
       ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
      params![
        credential_ref_id,
        provider_type,
        provider_id,
        alias,
        format!("api-config.json#credentials/{credential_ref_id}"),
        masked_hint,
        created_at,
        updated_at,
        last_tested_at,
        status_text(status),
        checked_i64(credential_revision, "密钥修订号")?,
      ],
    )
    .map_err(|error| database_error("无法写入 API 密钥引用镜像", error))?;
  Ok(())
}

fn mirror_tikhub_connector(
  transaction: &Transaction<'_>,
  registry: &ApiProfileRegistry,
) -> AppResult<()> {
  let selected = registry
    .active_profile_ids
    .tikhub
    .as_ref()
    .and_then(|id| registry.tikhub_profiles.get(id))
    .or_else(|| registry.tikhub_profiles.values().next());
  let Some(profile) = selected else {
    return Ok(());
  };
  let workspace_id: String = transaction
    .query_row("SELECT id FROM workspace LIMIT 1", [], |row| row.get(0))
    .map_err(|error| database_error("无法读取 TikHub 镜像工作区 ID", error))?;
  let is_active = registry.active_profile_ids.tikhub.as_deref() == Some(profile.id.as_str());
  transaction
    .execute(
      "INSERT INTO tikhub_connector (
         id, workspace_id, secret_ref_id, base_url, enabled, config_version,
         last_tested_at, last_test_status, created_at, updated_at
       ) VALUES ('default', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
      params![
        workspace_id,
        profile.credential_ref_id,
        profile.base_url,
        i64::from(is_active && profile.status == ApiProfileStatus::Success),
        checked_i64(profile.revision, "TikHub 配置修订号")?,
        profile.last_tested_at,
        status_text(profile.status),
        profile.created_at,
        profile.updated_at,
      ],
    )
    .map_err(|error| database_error("无法写入 TikHub 连接器镜像", error))?;
  Ok(())
}

fn mirror_ai_providers(
  transaction: &Transaction<'_>,
  registry: &ApiProfileRegistry,
) -> AppResult<()> {
  for profile in registry.ai_profiles.values() {
    let enabled = registry.active_profile_ids.ai.as_deref() == Some(profile.id.as_str())
      && profile.status == ApiProfileStatus::Success;
    let default_model_id =
      (!profile.default_model_id.is_empty()).then_some(profile.default_model_id.as_str());
    transaction
      .execute(
        "INSERT INTO model_provider (
           id, provider_id, display_name, enabled, auth_type, secret_ref_id, base_url,
           api_format, region, default_model_id, cost_policy_json, rate_limit_policy_json,
           health_check_json, created_at, updated_at
         ) VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, '{}', '{}', ?9, ?10, ?11)",
        params![
          profile.id,
          profile.name,
          i64::from(enabled),
          if profile.provider_type == AiProviderType::Ollama {
            "none"
          } else {
            "api_key"
          },
          profile.credential_ref_id,
          (!profile.base_url.is_empty()).then_some(profile.base_url.as_str()),
          api_format_text(profile.api_format),
          default_model_id,
          serde_json::json!({
            "profile_status": status_text(profile.status),
            "last_tested_at": profile.last_tested_at,
            "revision": profile.revision,
          })
          .to_string(),
          profile.created_at,
          profile.updated_at,
        ],
      )
      .map_err(|error| database_error("无法写入 AI 供应商镜像", error))?;

    if let Some(model_id) = default_model_id {
      let model_profile_id = stable_uuid(&profile.id, "model-profile", model_id);
      transaction
        .execute(
          "INSERT INTO model_profile (
             id, provider_id, model_id, display_name, capabilities_json, context_window,
             supports_structured_output, supports_streaming, supports_tools, supports_vision,
             enabled, created_at, updated_at
           ) VALUES (?1, ?2, ?3, ?3, '{}', NULL, 0, 0, 0, 0, 1, ?4, ?5)",
          params![
            model_profile_id,
            profile.id,
            model_id,
            profile.created_at,
            profile.updated_at,
          ],
        )
        .map_err(|error| database_error("无法写入 AI 模型镜像", error))?;
    }
  }
  Ok(())
}

fn read_legacy_tikhub(connection: &Connection) -> AppResult<Option<LegacyTikhub>> {
  connection
    .query_row(
      "SELECT connector.base_url, connector.secret_ref_id, connector.config_version,
              connector.last_tested_at, connector.created_at, connector.updated_at, secret.alias
       FROM tikhub_connector AS connector
       LEFT JOIN secret_ref AS secret ON secret.id = connector.secret_ref_id
       WHERE connector.id = 'default'",
      [],
      |row| {
        Ok(LegacyTikhub {
          base_url: row.get(0)?,
          secret_ref_id: row.get(1)?,
          config_version: row.get(2)?,
          last_tested_at: row.get(3)?,
          created_at: row.get(4)?,
          updated_at: row.get(5)?,
          alias: row.get(6)?,
        })
      },
    )
    .optional()
    .map_err(|error| database_error("无法导入旧 TikHub 配置", error))
}

fn read_legacy_ai_profiles(connection: &Connection) -> AppResult<Vec<LegacyAiProfile>> {
  let mut statement = connection
    .prepare(
      "SELECT provider.id, provider.provider_id, provider.display_name,
              provider.secret_ref_id, provider.base_url, provider.api_format,
              COALESCE(provider.default_model_id, (
                SELECT profile.model_id FROM model_profile AS profile
                WHERE profile.provider_id = provider.provider_id
                ORDER BY profile.enabled DESC, profile.display_name, profile.model_id LIMIT 1
              ), ''), provider.created_at, provider.updated_at
       FROM model_provider AS provider
       ORDER BY provider.display_name, provider.provider_id",
    )
    .map_err(|error| database_error("无法准备旧 AI 配置导入", error))?;
  let rows = statement
    .query_map([], |row| {
      Ok(LegacyAiProfile {
        id: row.get(0)?,
        provider_id: row.get(1)?,
        display_name: row.get(2)?,
        secret_ref_id: row.get(3)?,
        base_url: row.get(4)?,
        api_format: row.get(5)?,
        default_model_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
      })
    })
    .map_err(|error| database_error("无法读取旧 AI 配置", error))?;
  rows
    .collect::<Result<Vec<_>, _>>()
    .map_err(|error| database_error("无法解析旧 AI 配置", error))
}

fn infer_ai_provider_type(provider_id: &str, api_format: &str) -> AiProviderType {
  let provider_id = provider_id.trim().to_ascii_lowercase();
  match api_format.trim() {
    "anthropic_messages" => AiProviderType::Anthropic,
    "gemini" => AiProviderType::Gemini,
    "ollama" => AiProviderType::Ollama,
    "openai_compatible" if provider_id == "openai" => AiProviderType::Openai,
    _ => AiProviderType::CustomOpenaiCompatible,
  }
}

fn format_for_provider(provider_type: AiProviderType) -> AiApiFormat {
  match provider_type {
    AiProviderType::Openai | AiProviderType::CustomOpenaiCompatible => {
      AiApiFormat::OpenaiCompatible
    }
    AiProviderType::Anthropic => AiApiFormat::AnthropicMessages,
    AiProviderType::Gemini => AiApiFormat::Gemini,
    AiProviderType::Ollama => AiApiFormat::Ollama,
  }
}

fn normalized_ai_base_url(provider_type: AiProviderType, base_url: Option<&str>) -> String {
  let base_url = base_url.unwrap_or_default().trim();
  if !base_url.is_empty() {
    return base_url.to_string();
  }
  match provider_type {
    AiProviderType::Openai => "https://api.openai.com/v1".to_string(),
    AiProviderType::Anthropic => "https://api.anthropic.com".to_string(),
    AiProviderType::Gemini => "https://generativelanguage.googleapis.com".to_string(),
    AiProviderType::Ollama => "http://localhost:11434".to_string(),
    AiProviderType::CustomOpenaiCompatible => String::new(),
  }
}

fn normalized_tikhub_base_url(base_url: &str) -> String {
  match base_url.trim_end_matches('/') {
    "https://api.tikhub.dev" => "https://api.tikhub.dev".to_string(),
    _ => "https://api.tikhub.io".to_string(),
  }
}

fn normalized_name(value: Option<&str>, fallback: &str) -> String {
  let value = value.unwrap_or_default().trim();
  if value.is_empty() {
    fallback.to_string()
  } else {
    value.to_string()
  }
}

fn unique_name(used: &mut BTreeSet<String>, base_name: &str) -> String {
  if used.insert(base_name.to_lowercase()) {
    return base_name.to_string();
  }
  for index in 2.. {
    let candidate = format!("{base_name}（{index}）");
    if used.insert(candidate.to_lowercase()) {
      return candidate;
    }
  }
  unreachable!()
}

fn stable_uuid_or_existing(namespace: &str, kind: &str, value: &str) -> String {
  Uuid::parse_str(value)
    .map(|uuid| uuid.to_string())
    .unwrap_or_else(|_| stable_uuid(namespace, kind, value))
}

fn stable_uuid(namespace: &str, kind: &str, value: &str) -> String {
  let mut hasher = Sha256::new();
  for component in ["sortlytic-api-profile-v1", namespace, kind, value] {
    hasher.update((component.len() as u64).to_be_bytes());
    hasher.update(component.as_bytes());
  }
  let digest = hasher.finalize();
  let mut bytes = [0_u8; 16];
  bytes.copy_from_slice(&digest[..16]);
  bytes[6] = (bytes[6] & 0x0f) | 0x50;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  Uuid::from_bytes(bytes).to_string()
}

fn mask_secret(secret: &str) -> String {
  let chars = secret.chars().collect::<Vec<_>>();
  if chars.len() <= 8 {
    return "[REDACTED]".to_string();
  }
  let prefix = chars.iter().take(4).collect::<String>();
  let suffix = chars[chars.len() - 4..].iter().collect::<String>();
  format!("{prefix}...[REDACTED]...{suffix}")
}

fn status_text(status: ApiProfileStatus) -> &'static str {
  match status {
    ApiProfileStatus::NeedsRebind => "needs_rebind",
    ApiProfileStatus::Untested => "untested",
    ApiProfileStatus::Success => "success",
    ApiProfileStatus::Failed => "failed",
  }
}

fn api_format_text(api_format: AiApiFormat) -> &'static str {
  match api_format {
    AiApiFormat::OpenaiCompatible => "openai_compatible",
    AiApiFormat::AnthropicMessages => "anthropic_messages",
    AiApiFormat::Gemini => "gemini",
    AiApiFormat::Ollama => "ollama",
  }
}

fn positive_revision(revision: i64) -> u64 {
  u64::try_from(revision)
    .ok()
    .filter(|value| *value > 0)
    .unwrap_or(1)
}

fn checked_i64(value: u64, label: &str) -> AppResult<i64> {
  i64::try_from(value).map_err(|_| {
    AppError::validation(
      format!("{label}超过 SQLite 可表示范围"),
      AppErrorStage::SecretStore,
    )
  })
}

fn valid_timestamp_or_now(value: String) -> String {
  if DateTime::parse_from_rfc3339(&value).is_ok() {
    value
  } else {
    Utc::now().to_rfc3339()
  }
}

fn valid_optional_timestamp(value: Option<String>) -> Option<String> {
  value.filter(|value| DateTime::parse_from_rfc3339(value).is_ok())
}

fn database_error(context: &str, error: rusqlite::Error) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    format!("{context}：{error}"),
    AppErrorStage::Database,
    false,
  )
}

struct LegacyTikhub {
  base_url: String,
  secret_ref_id: Option<String>,
  config_version: i64,
  last_tested_at: Option<String>,
  created_at: String,
  updated_at: String,
  alias: Option<String>,
}

struct LegacyAiProfile {
  id: String,
  provider_id: String,
  display_name: String,
  secret_ref_id: Option<String>,
  base_url: Option<String>,
  api_format: String,
  default_model_id: String,
  created_at: String,
  updated_at: String,
}

#[cfg(test)]
mod tests {
  use std::fs;

  use rusqlite::params;

  use super::*;
  use crate::api_profiles::{
    api_profile_registry_path, initialize_api_profile_registry, load_api_profile_registry,
    load_existing_api_profile_registry, save_api_profile_registry, sync_api_profile_mirror,
    CredentialProviderType,
  };
  use crate::workspace::{create_workspace, open_workspace, CURRENT_SCHEMA_VERSION};

  fn legacy_workspace() -> (std::path::PathBuf, String, String) {
    let root = std::env::temp_dir().join(format!("api-import-{}", Uuid::new_v4()));
    create_workspace("API 导入测试", &root).unwrap();
    fs::remove_file(api_profile_registry_path(&root)).unwrap();
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute("DELETE FROM schema_migrations WHERE version = 8", [])
      .unwrap();
    connection
      .execute("UPDATE workspace SET schema_version = 7", [])
      .unwrap();
    let workspace_id: String = connection
      .query_row("SELECT id FROM workspace", [], |row| row.get(0))
      .unwrap();
    let tikhub_secret_id = Uuid::new_v4().to_string();
    let ai_secret_id = Uuid::new_v4().to_string();
    let ai_profile_id = Uuid::new_v4().to_string();
    let timestamp = "2026-07-16T09:30:00+00:00";
    for (id, provider_type, provider_id, alias) in [
      (&tikhub_secret_id, "tikhub", "default", "TikHub 旧账号"),
      (
        &ai_secret_id,
        "model_provider",
        "custom-openai",
        "DeepSeek 旧账号",
      ),
    ] {
      connection
        .execute(
          "INSERT INTO secret_ref (
             id, provider_type, provider_id, alias, secret_store_key, masked_hint,
             created_at, updated_at, credential_revision
           ) VALUES (?1, ?2, ?3, ?4, ?5, 'old...[REDACTED]...hint', ?6, ?6, 3)",
          params![
            id,
            provider_type,
            provider_id,
            alias,
            format!("legacy-keychain-{id}"),
            timestamp
          ],
        )
        .unwrap();
    }
    connection
      .execute(
        "INSERT INTO tikhub_connector (
           id, workspace_id, secret_ref_id, base_url, enabled, config_version,
           last_tested_at, last_test_status, created_at, updated_at
         ) VALUES ('default', ?1, ?2, 'https://api.tikhub.io', 1, 4, ?3, 'success', ?3, ?3)",
        params![workspace_id, tikhub_secret_id, timestamp],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO model_provider (
           id, provider_id, display_name, enabled, auth_type, secret_ref_id, base_url,
           api_format, default_model_id, created_at, updated_at
         ) VALUES (?1, 'custom-openai', 'DeepSeek 生产端点', 1, 'api_key', ?2,
                   'https://api.deepseek.com', 'openai_compatible', 'deepseek-v4-flash', ?3, ?3)",
        params![ai_profile_id, ai_secret_id, timestamp],
      )
      .unwrap();
    drop(connection);
    (root, tikhub_secret_id, ai_secret_id)
  }

  #[test]
  fn opening_legacy_workspace_imports_metadata_for_rebind_without_keychain_access() {
    let (root, tikhub_secret_id, ai_secret_id) = legacy_workspace();

    let summary = open_workspace(&root).expect("legacy workspace should open");
    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(api_profile_registry_path(&root).is_file());
    let registry = load_api_profile_registry(&root).unwrap();

    assert_eq!(registry.tikhub_profiles.len(), 1);
    assert_eq!(registry.ai_profiles.len(), 1);
    assert!(registry.credentials.is_empty());
    assert_eq!(registry.active_profile_ids.tikhub, None);
    assert_eq!(registry.active_profile_ids.ai, None);
    let tikhub = registry.tikhub_profiles.values().next().unwrap();
    assert_eq!(tikhub.credential_ref_id, tikhub_secret_id);
    assert_eq!(tikhub.status, ApiProfileStatus::NeedsRebind);
    let ai = registry.ai_profiles.values().next().unwrap();
    assert_eq!(ai.credential_ref_id.as_deref(), Some(ai_secret_id.as_str()));
    assert_eq!(ai.default_model_id, "deepseek-v4-flash");
    assert_eq!(ai.base_url, "https://api.deepseek.com");
    assert_eq!(ai.status, ApiProfileStatus::NeedsRebind);
    assert!(api_profile_registry_path(&root).is_file());
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn mirror_is_deterministic_and_never_writes_plaintext_credentials_to_sqlite() {
    let (root, _, _) = legacy_workspace();
    let mut registry = initialize_api_profile_registry(&root).unwrap();
    let tikhub = registry.tikhub_profiles.values().next().unwrap().clone();
    let ai = registry.ai_profiles.values().next().unwrap().clone();
    registry.credentials.insert(
      tikhub.credential_ref_id.clone(),
      ApiCredential {
        id: tikhub.credential_ref_id.clone(),
        provider_type: CredentialProviderType::Tikhub,
        profile_id: tikhub.id,
        revision: 4,
        secret: "tikhub-plaintext-sentinel-7193".to_string(),
      },
    );
    registry.credentials.insert(
      ai.credential_ref_id.clone().unwrap(),
      ApiCredential {
        id: ai.credential_ref_id.clone().unwrap(),
        provider_type: CredentialProviderType::CustomOpenaiCompatible,
        profile_id: ai.id,
        revision: 2,
        secret: "ai-plaintext-sentinel-4821".to_string(),
      },
    );
    save_api_profile_registry(&root, &registry).unwrap();
    sync_api_profile_mirror(&root).unwrap();
    let first = mirror_dump(&root);
    sync_api_profile_mirror(&root).unwrap();
    let second = mirror_dump(&root);

    assert_eq!(first, second);
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let audit_text: String = connection
      .query_row(
        "SELECT COALESCE(group_concat(safe_details_json, '\n'), '') FROM audit_log",
        [],
        |row| row.get(0),
      )
      .unwrap();
    let database_text = format!("{}\n{audit_text}", first.join("\n"));
    assert!(!database_text.contains("tikhub-plaintext-sentinel-7193"));
    assert!(!database_text.contains("ai-plaintext-sentinel-4821"));
    let json = fs::read_to_string(api_profile_registry_path(&root)).unwrap();
    assert!(json.contains("tikhub-plaintext-sentinel-7193"));
    assert!(json.contains("ai-plaintext-sentinel-4821"));
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn damaged_registry_does_not_block_local_workspace_browsing() {
    let root = std::env::temp_dir().join(format!("api-corrupt-open-{}", Uuid::new_v4()));
    create_workspace("损坏注册表测试", &root).unwrap();
    let damaged = b"{ damaged registry sentinel";
    fs::write(api_profile_registry_path(&root), damaged).unwrap();

    assert!(open_workspace(&root).is_ok());
    assert_eq!(fs::read(api_profile_registry_path(&root)).unwrap(), damaged);
    assert!(load_api_profile_registry(&root).is_err());
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn rejected_legacy_api_metadata_does_not_block_local_workspace_browsing() {
    const SENTINEL: &str = "legacy-url-secret-sentinel-4071";
    let (root, _, _) = legacy_workspace();
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute(
        "UPDATE model_provider SET base_url = ?1",
        [format!(
          "https://{SENTINEL}@example.test/v1?api_key={SENTINEL}"
        )],
      )
      .unwrap();
    drop(connection);

    let summary = open_workspace(&root).expect("local workspace should remain browseable");
    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let migration_count: i64 = connection
      .query_row(
        "SELECT COUNT(*) FROM schema_migrations WHERE version = 8",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(migration_count, 1);
    drop(connection);

    assert!(!api_profile_registry_path(&root).exists());
    assert_eq!(load_existing_api_profile_registry(&root).unwrap(), None);
    let error = initialize_api_profile_registry(&root).unwrap_err();
    assert!(!error.message.contains(SENTINEL));
    assert!(!api_profile_registry_path(&root).exists());
    fs::remove_dir_all(root).ok();
  }

  fn mirror_dump(root: &Path) -> Vec<String> {
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let mut values = Vec::new();
    for sql in [
      "SELECT id || '|' || provider_type || '|' || provider_id || '|' || secret_store_key || '|' || masked_hint FROM secret_ref WHERE provider_type IN ('tikhub', 'model_provider') ORDER BY id",
      "SELECT id || '|' || provider_id || '|' || display_name || '|' || enabled || '|' || COALESCE(base_url, '') || '|' || api_format || '|' || COALESCE(default_model_id, '') FROM model_provider ORDER BY id",
      "SELECT id || '|' || base_url || '|' || enabled || '|' || config_version || '|' || COALESCE(last_test_status, '') FROM tikhub_connector ORDER BY id",
    ] {
      let mut statement = connection.prepare(sql).unwrap();
      values.extend(
        statement
          .query_map([], |row| row.get::<_, String>(0))
          .unwrap()
          .collect::<Result<Vec<_>, _>>()
          .unwrap(),
      );
    }
    values
  }
}
