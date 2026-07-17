use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::api_profiles::{
  initialize_api_profile_registry, load_existing_api_profile_registry, sync_api_profile_mirror,
  update_api_profile_registry, AiApiFormat, AiApiProfile, AiProviderType, ApiCredential,
  ApiProfileRegistry, ApiProfileStatus, CredentialProviderType, TikhubApiProfile,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

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

#[derive(Debug, Clone)]
enum ProfileLocation {
  Tikhub(String),
  Ai(String),
}

pub fn save_secret(
  root_path: impl AsRef<Path>,
  provider_type: &str,
  provider_id: &str,
  secret: &str,
  alias: Option<String>,
) -> AppResult<SecretRefView> {
  let provider_type = normalize_provider_type(provider_type)?;
  if provider_type == "webhook" {
    return Err(AppError::validation(
      "统一 API 配置库当前只接受 TikHub 与 AI 密钥",
      AppErrorStage::SecretStore,
    ));
  }
  let provider_id = normalize_provider_id(provider_id)?;
  let secret = normalize_secret(secret)?;
  let alias = normalize_alias(alias);
  let root_path = root_path.as_ref();
  validate_workspace_scope(root_path)?;
  ensure_registry_exists(root_path)?;

  let view = update_api_profile_registry(root_path, |registry| {
    let credential_ref_id = Uuid::new_v4().to_string();
    let timestamp = Utc::now().to_rfc3339();
    let location = if provider_type == "tikhub" {
      create_tikhub_profile(
        registry,
        &provider_id,
        alias.as_deref(),
        &credential_ref_id,
        &timestamp,
      )
    } else {
      create_ai_profile(
        registry,
        &provider_id,
        alias.as_deref(),
        &credential_ref_id,
        &timestamp,
      )
    };
    let (profile_id, credential_type) = location_identity(registry, &location)?;
    registry.credentials.insert(
      credential_ref_id.clone(),
      ApiCredential {
        id: credential_ref_id,
        provider_type: credential_type,
        profile_id,
        revision: 1,
        secret: secret.clone(),
      },
    );
    secret_ref_view(registry, &location)
  })?;
  sync_api_profile_mirror(root_path)?;
  write_secret_audit_log(
    root_path,
    "save_secret",
    Some(&view.id),
    serde_json::json!({
      "provider_type": view.provider_type,
      "profile_id": view.provider_id,
      "has_alias": view.alias.is_some(),
    }),
  )?;
  Ok(view)
}

pub fn update_secret(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
  secret: &str,
) -> AppResult<SecretRefView> {
  let secret = normalize_secret(secret)?;
  let root_path = root_path.as_ref();
  validate_workspace_scope(root_path)?;
  ensure_registry_exists(root_path)?;
  let now = Utc::now().to_rfc3339();
  let view = update_api_profile_registry(root_path, |registry| {
    let location = find_profile_by_credential(registry, secret_ref_id)
      .ok_or_else(|| secret_store_error("密钥引用不存在，请在 API 配置中重新输入"))?;
    let (profile_id, credential_type) = location_identity(registry, &location)?;
    let revision = registry
      .credentials
      .get(secret_ref_id)
      .map(|credential| credential.revision)
      .unwrap_or(0)
      .checked_add(1)
      .ok_or_else(|| secret_store_error("密钥修订号已达到上限"))?;
    registry.credentials.insert(
      secret_ref_id.to_string(),
      ApiCredential {
        id: secret_ref_id.to_string(),
        provider_type: credential_type,
        profile_id,
        revision,
        secret: secret.clone(),
      },
    );
    invalidate_profile_after_secret_change(registry, &location, &now);
    secret_ref_view(registry, &location)
  })?;
  sync_api_profile_mirror(root_path)?;
  write_secret_audit_log(
    root_path,
    "update_secret",
    Some(secret_ref_id),
    serde_json::json!({
      "provider_type": view.provider_type,
      "profile_id": view.provider_id,
    }),
  )?;
  Ok(view)
}

pub fn delete_secret(root_path: impl AsRef<Path>, secret_ref_id: &str) -> AppResult<bool> {
  let root_path = root_path.as_ref();
  validate_workspace_scope(root_path)?;
  ensure_registry_exists(root_path)?;
  let now = Utc::now().to_rfc3339();
  let deleted = update_api_profile_registry(root_path, |registry| {
    let Some(location) = find_profile_by_credential(registry, secret_ref_id) else {
      return Ok(false);
    };
    if registry.credentials.remove(secret_ref_id).is_none() {
      return Ok(false);
    }
    mark_profile_needs_rebind(registry, &location, &now);
    Ok(true)
  })?;
  if !deleted {
    return Ok(false);
  }
  sync_api_profile_mirror(root_path)?;
  write_secret_audit_log(
    root_path,
    "delete_secret",
    Some(secret_ref_id),
    serde_json::json!({ "credential_removed": true }),
  )?;
  Ok(true)
}

pub fn list_secret_refs(
  root_path: impl AsRef<Path>,
  provider_type: Option<String>,
) -> AppResult<Vec<SecretRefView>> {
  let root_path = root_path.as_ref();
  validate_workspace_scope(root_path)?;
  let registry = registry_for_access(root_path)?;
  let provider_type = provider_type
    .as_deref()
    .map(normalize_provider_type)
    .transpose()?;
  let mut views = registry
    .tikhub_profiles
    .keys()
    .map(|id| secret_ref_view(&registry, &ProfileLocation::Tikhub(id.clone())))
    .chain(registry.ai_profiles.keys().filter_map(|id| {
      registry.ai_profiles[id]
        .credential_ref_id
        .as_ref()
        .map(|_| secret_ref_view(&registry, &ProfileLocation::Ai(id.clone())))
    }))
    .collect::<AppResult<Vec<_>>>()?;
  views.retain(|view| {
    provider_type
      .as_deref()
      .is_none_or(|provider_type| view.provider_type == provider_type)
  });
  views.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
  Ok(views)
}

pub fn test_secret_connection(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
) -> AppResult<SecretConnectionTestResult> {
  let root_path = root_path.as_ref();
  let registry = registry_for_access(root_path)?;
  let tested_at = Utc::now().to_rfc3339();
  let success = registry
    .credentials
    .get(secret_ref_id)
    .is_some_and(|credential| !credential.secret.trim().is_empty());
  let message = if success {
    "密钥可从当前工作区私有 JSON 读取".to_string()
  } else {
    "密钥尚未写入当前工作区私有 JSON，请重新输入".to_string()
  };
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
  let connection = scoped_workspace_connection(root_path)?;
  validate_secret_ref_provider(&connection, secret_ref_id, &expected_provider_type)?;
  drop(connection);
  let registry = registry_for_access(root_path)?;
  let location = find_profile_by_credential(&registry, secret_ref_id)
    .ok_or_else(|| secret_store_error("旧密钥未迁移；请重新输入密钥完成绑定"))?;
  let actual_provider_type = match location {
    ProfileLocation::Tikhub(_) => "tikhub",
    ProfileLocation::Ai(_) => "model_provider",
  };
  if actual_provider_type != expected_provider_type {
    return Err(permission_error("密钥类型与当前调用目标不匹配"));
  }
  registry
    .credentials
    .get(secret_ref_id)
    .map(|credential| credential.secret.clone())
    .ok_or_else(|| secret_store_error("密钥需要重新输入，未从旧系统存储读取"))
}

pub(crate) fn read_secret_for_snapshot(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
  expected_provider_type: &str,
  expected_profile_id: &str,
  expected_revision: u64,
) -> AppResult<String> {
  let expected_provider_type = normalize_provider_type(expected_provider_type)?;
  let root_path = root_path.as_ref();
  let connection = scoped_workspace_connection(root_path)?;
  validate_secret_ref_provider(&connection, secret_ref_id, &expected_provider_type)?;
  drop(connection);

  let registry = registry_for_access(root_path)?;
  let location = find_profile_by_credential(&registry, secret_ref_id)
    .ok_or_else(|| secret_store_error("运行快照引用的密钥尚未迁移，请重新绑定后重试"))?;
  let (actual_provider_type, actual_profile_id) = match &location {
    ProfileLocation::Tikhub(profile_id) => ("tikhub", profile_id.as_str()),
    ProfileLocation::Ai(profile_id) => ("model_provider", profile_id.as_str()),
  };
  if actual_provider_type != expected_provider_type || actual_profile_id != expected_profile_id {
    return Err(permission_error("运行快照的 API 配置身份与当前凭据不匹配"));
  }

  let credential = registry
    .credentials
    .get(secret_ref_id)
    .ok_or_else(|| secret_store_error("运行快照引用的密钥需要重新输入"))?;
  if credential.profile_id != expected_profile_id || credential.revision != expected_revision {
    return Err(permission_error("运行快照的密钥修订号与当前凭据不匹配"));
  }
  Ok(credential.secret.clone())
}

pub(crate) fn validate_secret_ref_provider(
  connection: &Connection,
  secret_ref_id: &str,
  expected_provider_type: &str,
) -> AppResult<()> {
  let expected_provider_type = normalize_provider_type(expected_provider_type)?;
  let provider_type = connection
    .query_row(
      "SELECT provider_type FROM secret_ref WHERE id = ?1",
      params![secret_ref_id],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| secret_store_error("密钥引用不存在"))?;
  if provider_type != expected_provider_type {
    return Err(permission_error("密钥类型与当前调用目标不匹配"));
  }
  Ok(())
}

pub fn mask_secret(secret: &str) -> String {
  let chars = secret.chars().collect::<Vec<_>>();
  if chars.len() <= 8 {
    return "[REDACTED]".to_string();
  }
  let prefix = chars.iter().take(4).collect::<String>();
  let suffix = chars[chars.len() - 4..].iter().collect::<String>();
  format!("{prefix}...[REDACTED]...{suffix}")
}

fn create_tikhub_profile(
  registry: &mut ApiProfileRegistry,
  provider_id: &str,
  alias: Option<&str>,
  credential_ref_id: &str,
  timestamp: &str,
) -> ProfileLocation {
  if Uuid::parse_str(provider_id).is_ok() && registry.tikhub_profiles.contains_key(provider_id) {
    let profile = registry.tikhub_profiles.get_mut(provider_id).unwrap();
    profile.credential_ref_id = credential_ref_id.to_string();
    profile.status = ApiProfileStatus::Untested;
    profile.updated_at = timestamp.to_string();
    return ProfileLocation::Tikhub(provider_id.to_string());
  }
  let profile_id = Uuid::new_v4().to_string();
  let requested_name = alias.unwrap_or("TikHub API");
  let name = unique_profile_name(
    registry
      .tikhub_profiles
      .values()
      .map(|profile| profile.name.as_str()),
    requested_name,
  );
  let base_url = if requested_name.contains("中国") || requested_name.contains("dev") {
    "https://api.tikhub.dev"
  } else {
    "https://api.tikhub.io"
  };
  registry.tikhub_profiles.insert(
    profile_id.clone(),
    TikhubApiProfile {
      id: profile_id.clone(),
      name,
      base_url: base_url.to_string(),
      credential_ref_id: credential_ref_id.to_string(),
      revision: 1,
      status: ApiProfileStatus::Untested,
      last_tested_at: None,
      test_summary: None,
      created_at: timestamp.to_string(),
      updated_at: timestamp.to_string(),
    },
  );
  ProfileLocation::Tikhub(profile_id)
}

fn create_ai_profile(
  registry: &mut ApiProfileRegistry,
  provider_id: &str,
  alias: Option<&str>,
  credential_ref_id: &str,
  timestamp: &str,
) -> ProfileLocation {
  if Uuid::parse_str(provider_id).is_ok() && registry.ai_profiles.contains_key(provider_id) {
    let profile = registry.ai_profiles.get_mut(provider_id).unwrap();
    profile.credential_ref_id = Some(credential_ref_id.to_string());
    profile.status = if ai_profile_is_complete(profile) {
      ApiProfileStatus::Untested
    } else {
      ApiProfileStatus::NeedsRebind
    };
    profile.updated_at = timestamp.to_string();
    return ProfileLocation::Ai(provider_id.to_string());
  }
  let profile_id = Uuid::new_v4().to_string();
  let provider_type = infer_ai_provider_type(provider_id);
  let requested_name = alias.unwrap_or("AI API");
  let name = unique_profile_name(
    registry
      .ai_profiles
      .values()
      .map(|profile| profile.name.as_str()),
    requested_name,
  );
  registry.ai_profiles.insert(
    profile_id.clone(),
    AiApiProfile {
      id: profile_id.clone(),
      name,
      provider_type,
      api_format: api_format_for_provider(provider_type),
      base_url: default_ai_base_url(provider_type).to_string(),
      default_model_id: String::new(),
      credential_ref_id: Some(credential_ref_id.to_string()),
      revision: 1,
      status: ApiProfileStatus::NeedsRebind,
      last_tested_at: None,
      created_at: timestamp.to_string(),
      updated_at: timestamp.to_string(),
    },
  );
  ProfileLocation::Ai(profile_id)
}

fn location_identity(
  registry: &ApiProfileRegistry,
  location: &ProfileLocation,
) -> AppResult<(String, CredentialProviderType)> {
  match location {
    ProfileLocation::Tikhub(profile_id) => Ok((profile_id.clone(), CredentialProviderType::Tikhub)),
    ProfileLocation::Ai(profile_id) => {
      let profile = registry
        .ai_profiles
        .get(profile_id)
        .ok_or_else(|| secret_store_error("AI 配置不存在"))?;
      Ok((
        profile_id.clone(),
        credential_type_for_ai(profile.provider_type),
      ))
    }
  }
}

fn find_profile_by_credential(
  registry: &ApiProfileRegistry,
  credential_ref_id: &str,
) -> Option<ProfileLocation> {
  registry
    .tikhub_profiles
    .iter()
    .find(|(_, profile)| profile.credential_ref_id == credential_ref_id)
    .map(|(id, _)| ProfileLocation::Tikhub(id.clone()))
    .or_else(|| {
      registry
        .ai_profiles
        .iter()
        .find(|(_, profile)| profile.credential_ref_id.as_deref() == Some(credential_ref_id))
        .map(|(id, _)| ProfileLocation::Ai(id.clone()))
    })
}

fn invalidate_profile_after_secret_change(
  registry: &mut ApiProfileRegistry,
  location: &ProfileLocation,
  timestamp: &str,
) {
  match location {
    ProfileLocation::Tikhub(profile_id) => {
      let profile = registry.tikhub_profiles.get_mut(profile_id).unwrap();
      profile.status = ApiProfileStatus::Untested;
      profile.last_tested_at = None;
      profile.test_summary = None;
      profile.updated_at = timestamp.to_string();
      if registry.active_profile_ids.tikhub.as_deref() == Some(profile_id) {
        registry.active_profile_ids.tikhub = None;
      }
    }
    ProfileLocation::Ai(profile_id) => {
      let profile = registry.ai_profiles.get_mut(profile_id).unwrap();
      profile.status = if ai_profile_is_complete(profile) {
        ApiProfileStatus::Untested
      } else {
        ApiProfileStatus::NeedsRebind
      };
      profile.last_tested_at = None;
      profile.updated_at = timestamp.to_string();
      if registry.active_profile_ids.ai.as_deref() == Some(profile_id) {
        registry.active_profile_ids.ai = None;
      }
    }
  }
}

fn mark_profile_needs_rebind(
  registry: &mut ApiProfileRegistry,
  location: &ProfileLocation,
  timestamp: &str,
) {
  match location {
    ProfileLocation::Tikhub(profile_id) => {
      let profile = registry.tikhub_profiles.get_mut(profile_id).unwrap();
      profile.status = ApiProfileStatus::NeedsRebind;
      profile.last_tested_at = None;
      profile.test_summary = None;
      profile.updated_at = timestamp.to_string();
      if registry.active_profile_ids.tikhub.as_deref() == Some(profile_id) {
        registry.active_profile_ids.tikhub = None;
      }
    }
    ProfileLocation::Ai(profile_id) => {
      let profile = registry.ai_profiles.get_mut(profile_id).unwrap();
      profile.status = if profile.provider_type == AiProviderType::Ollama {
        ApiProfileStatus::Untested
      } else {
        ApiProfileStatus::NeedsRebind
      };
      profile.last_tested_at = None;
      profile.updated_at = timestamp.to_string();
      if registry.active_profile_ids.ai.as_deref() == Some(profile_id) {
        registry.active_profile_ids.ai = None;
      }
    }
  }
}

fn secret_ref_view(
  registry: &ApiProfileRegistry,
  location: &ProfileLocation,
) -> AppResult<SecretRefView> {
  let (id, provider_type, provider_id, alias, created_at, updated_at, last_tested_at, status) =
    match location {
      ProfileLocation::Tikhub(profile_id) => {
        let profile = registry
          .tikhub_profiles
          .get(profile_id)
          .ok_or_else(|| secret_store_error("TikHub 配置不存在"))?;
        (
          profile.credential_ref_id.clone(),
          "tikhub".to_string(),
          profile.id.clone(),
          Some(profile.name.clone()),
          profile.created_at.clone(),
          profile.updated_at.clone(),
          profile.last_tested_at.clone(),
          profile.status,
        )
      }
      ProfileLocation::Ai(profile_id) => {
        let profile = registry
          .ai_profiles
          .get(profile_id)
          .ok_or_else(|| secret_store_error("AI 配置不存在"))?;
        (
          profile
            .credential_ref_id
            .clone()
            .ok_or_else(|| secret_store_error("AI 配置没有密钥引用"))?,
          "model_provider".to_string(),
          profile.id.clone(),
          Some(profile.name.clone()),
          profile.created_at.clone(),
          profile.updated_at.clone(),
          profile.last_tested_at.clone(),
          profile.status,
        )
      }
    };
  let masked_hint = registry
    .credentials
    .get(&id)
    .map(|credential| mask_secret(&credential.secret))
    .unwrap_or_else(|| "[NEEDS_REBIND]".to_string());
  Ok(SecretRefView {
    id,
    provider_type,
    provider_id,
    alias,
    masked_hint,
    created_at,
    updated_at,
    last_tested_at,
    last_test_status: Some(profile_status_text(status).to_string()),
  })
}

fn ensure_registry_exists(root_path: &Path) -> AppResult<()> {
  if load_existing_api_profile_registry(root_path)?.is_none() {
    initialize_api_profile_registry(root_path)?;
  }
  Ok(())
}

fn registry_for_access(root_path: &Path) -> AppResult<ApiProfileRegistry> {
  match load_existing_api_profile_registry(root_path)? {
    Some(registry) => Ok(registry),
    None => initialize_api_profile_registry(root_path),
  }
}

fn scoped_workspace_connection(root_path: &Path) -> AppResult<Connection> {
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))?;
  validate_workspace_scope_with_connection(root_path, &connection)?;
  Ok(connection)
}

fn validate_workspace_scope(root_path: &Path) -> AppResult<()> {
  scoped_workspace_connection(root_path).map(|_| ())
}

fn validate_workspace_scope_with_connection(
  root_path: &Path,
  connection: &Connection,
) -> AppResult<()> {
  let (count, registered_root) = connection
    .query_row(
      "SELECT COUNT(*), MIN(root_path) FROM workspace",
      [],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .map_err(database_error)?;
  let Some(registered_root) = registered_root.filter(|_| count == 1) else {
    return Err(database_error("工作区元数据不完整，无法访问 API 配置"));
  };
  let canonical_root = std::fs::canonicalize(root_path).map_err(secret_store_error)?;
  let canonical_registered = std::fs::canonicalize(registered_root)
    .map_err(|_| permission_error("工作区登记路径无法验证，已拒绝访问 API 配置"))?;
  if canonical_root != canonical_registered {
    return Err(permission_error(
      "当前工作区路径与数据库登记路径不一致，已拒绝访问 API 配置",
    ));
  }
  Ok(())
}

fn write_secret_audit_log(
  root_path: &Path,
  action: &str,
  entity_id: Option<&str>,
  safe_details: serde_json::Value,
) -> AppResult<()> {
  let connection = scoped_workspace_connection(root_path)?;
  connection
    .execute(
      "INSERT INTO audit_log (id, entity_type, entity_id, action, safe_details_json, created_at)
       VALUES (?1, 'secret_ref', ?2, ?3, ?4, ?5)",
      params![
        Uuid::new_v4().to_string(),
        entity_id,
        action,
        safe_details.to_string(),
        Utc::now().to_rfc3339(),
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn normalize_provider_type(provider_type: &str) -> AppResult<String> {
  match provider_type.trim() {
    "tikhub" | "model_provider" | "webhook" => Ok(provider_type.trim().to_string()),
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
    (!alias.is_empty()).then_some(alias)
  })
}

fn unique_profile_name<'a>(names: impl Iterator<Item = &'a str>, requested: &str) -> String {
  let requested = requested.trim();
  let requested = if requested.is_empty() {
    "API 配置"
  } else {
    requested
  };
  let existing = names
    .map(|name| name.to_lowercase())
    .collect::<std::collections::BTreeSet<_>>();
  if !existing.contains(&requested.to_lowercase()) {
    return requested.to_string();
  }
  (2..)
    .map(|index| format!("{requested}（{index}）"))
    .find(|candidate| !existing.contains(&candidate.to_lowercase()))
    .unwrap()
}

fn infer_ai_provider_type(provider_id: &str) -> AiProviderType {
  match provider_id.trim().to_ascii_lowercase().as_str() {
    "openai" => AiProviderType::Openai,
    "anthropic" => AiProviderType::Anthropic,
    "gemini" => AiProviderType::Gemini,
    "ollama" => AiProviderType::Ollama,
    _ => AiProviderType::CustomOpenaiCompatible,
  }
}

fn api_format_for_provider(provider_type: AiProviderType) -> AiApiFormat {
  match provider_type {
    AiProviderType::Openai | AiProviderType::CustomOpenaiCompatible => {
      AiApiFormat::OpenaiCompatible
    }
    AiProviderType::Anthropic => AiApiFormat::AnthropicMessages,
    AiProviderType::Gemini => AiApiFormat::Gemini,
    AiProviderType::Ollama => AiApiFormat::Ollama,
  }
}

fn default_ai_base_url(provider_type: AiProviderType) -> &'static str {
  match provider_type {
    AiProviderType::Openai => "https://api.openai.com/v1",
    AiProviderType::Anthropic => "https://api.anthropic.com",
    AiProviderType::Gemini => "https://generativelanguage.googleapis.com",
    AiProviderType::Ollama => "http://localhost:11434",
    AiProviderType::CustomOpenaiCompatible => "",
  }
}

fn credential_type_for_ai(provider_type: AiProviderType) -> CredentialProviderType {
  match provider_type {
    AiProviderType::Openai => CredentialProviderType::Openai,
    AiProviderType::Anthropic => CredentialProviderType::Anthropic,
    AiProviderType::Gemini => CredentialProviderType::Gemini,
    AiProviderType::CustomOpenaiCompatible => CredentialProviderType::CustomOpenaiCompatible,
    AiProviderType::Ollama => CredentialProviderType::Ollama,
  }
}

fn ai_profile_is_complete(profile: &AiApiProfile) -> bool {
  !profile.base_url.trim().is_empty() && !profile.default_model_id.trim().is_empty()
}

fn profile_status_text(status: ApiProfileStatus) -> &'static str {
  match status {
    ApiProfileStatus::NeedsRebind => "needs_rebind",
    ApiProfileStatus::Untested => "untested",
    ApiProfileStatus::Success => "success",
    ApiProfileStatus::Failed => "failed",
  }
}

fn permission_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::PermissionError,
    message,
    AppErrorStage::SecretStore,
    false,
  )
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

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
