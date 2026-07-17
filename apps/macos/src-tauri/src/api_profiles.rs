use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::DateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

mod mirror;
mod storage;

#[cfg(test)]
mod concurrency_tests;

#[cfg(test)]
mod storage_tests;

#[cfg(test)]
mod url_validation_tests;

pub const API_PROFILE_SCHEMA_VERSION: u32 = 1;

static REGISTRY_LOCK: Mutex<()> = Mutex::new(());
static MIRROR_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ApiProfileRegistry {
  pub schema_version: u32,
  pub active_profile_ids: ActiveApiProfileIds,
  pub tikhub_profiles: BTreeMap<String, TikhubApiProfile>,
  pub ai_profiles: BTreeMap<String, AiApiProfile>,
  pub credentials: BTreeMap<String, ApiCredential>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ActiveApiProfileIds {
  pub tikhub: Option<String>,
  pub ai: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TikhubApiProfile {
  pub id: String,
  pub name: String,
  pub base_url: String,
  pub credential_ref_id: String,
  pub revision: u64,
  pub status: ApiProfileStatus,
  pub last_tested_at: Option<String>,
  pub test_summary: Option<TikhubSafeTestSummary>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TikhubSafeTestSummary {
  pub masked_account: Option<String>,
  pub balance: Option<f64>,
  pub free_credit: Option<f64>,
  pub available_credit: Option<f64>,
  pub today_usage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AiApiProfile {
  pub id: String,
  pub name: String,
  pub provider_type: AiProviderType,
  pub api_format: AiApiFormat,
  pub base_url: String,
  pub default_model_id: String,
  pub credential_ref_id: Option<String>,
  pub revision: u64,
  pub status: ApiProfileStatus,
  pub last_tested_at: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiProviderType {
  Openai,
  Anthropic,
  Gemini,
  CustomOpenaiCompatible,
  Ollama,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiApiFormat {
  OpenaiCompatible,
  AnthropicMessages,
  Gemini,
  Ollama,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiProfileStatus {
  NeedsRebind,
  Untested,
  Success,
  Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ApiCredential {
  pub id: String,
  pub provider_type: CredentialProviderType,
  pub profile_id: String,
  pub revision: u64,
  pub secret: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialProviderType {
  Tikhub,
  Openai,
  Anthropic,
  Gemini,
  CustomOpenaiCompatible,
  Ollama,
}

impl Default for ApiProfileRegistry {
  fn default() -> Self {
    Self {
      schema_version: API_PROFILE_SCHEMA_VERSION,
      active_profile_ids: ActiveApiProfileIds::default(),
      tikhub_profiles: BTreeMap::new(),
      ai_profiles: BTreeMap::new(),
      credentials: BTreeMap::new(),
    }
  }
}

pub fn api_profile_registry_path(root_path: impl AsRef<Path>) -> PathBuf {
  storage::registry_path(root_path.as_ref())
}

pub fn api_profile_registry_exists(root_path: impl AsRef<Path>) -> AppResult<bool> {
  with_registry_lock(|| storage::registry_exists(root_path.as_ref()))
}

pub fn load_api_profile_registry(root_path: impl AsRef<Path>) -> AppResult<ApiProfileRegistry> {
  with_registry_lock(|| {
    storage::load_optional_registry(root_path.as_ref()).map(|registry| registry.unwrap_or_default())
  })
}

pub fn load_existing_api_profile_registry(
  root_path: impl AsRef<Path>,
) -> AppResult<Option<ApiProfileRegistry>> {
  with_registry_lock(|| storage::load_optional_registry(root_path.as_ref()))
}

pub fn save_api_profile_registry(
  root_path: impl AsRef<Path>,
  registry: &ApiProfileRegistry,
) -> AppResult<()> {
  with_registry_lock(|| {
    validate_registry(registry)?;
    storage::write_registry(root_path.as_ref(), registry)
  })
}

pub fn update_api_profile_registry<T, F>(root_path: impl AsRef<Path>, update: F) -> AppResult<T>
where
  F: FnOnce(&mut ApiProfileRegistry) -> AppResult<T>,
{
  with_registry_lock(|| {
    let mut registry = storage::load_optional_registry(root_path.as_ref())?.unwrap_or_default();
    let result = update(&mut registry)?;
    validate_registry(&registry)?;
    storage::write_registry(root_path.as_ref(), &registry)?;
    Ok(result)
  })
}

pub fn initialize_api_profile_registry(
  root_path: impl AsRef<Path>,
) -> AppResult<ApiProfileRegistry> {
  let root_path = root_path.as_ref();
  with_mirror_lock(|| {
    let registry = match load_existing_api_profile_registry(root_path)? {
      Some(registry) => registry,
      None => {
        let imported = mirror::import_legacy_registry(root_path)?;
        with_registry_lock(|| match storage::load_optional_registry(root_path)? {
          Some(registry) => Ok(registry),
          None => {
            validate_registry(&imported)?;
            storage::write_registry(root_path, &imported)?;
            Ok(imported)
          }
        })?
      }
    };
    mirror::mirror_registry(root_path, &registry)?;
    Ok(registry)
  })
}

pub fn sync_api_profile_mirror(root_path: impl AsRef<Path>) -> AppResult<()> {
  let root_path = root_path.as_ref();
  with_mirror_lock(|| {
    let registry = load_existing_api_profile_registry(root_path)?
      .ok_or_else(|| registry_error("API 配置文件不存在，已拒绝重建 SQLite 镜像"))?;
    #[cfg(test)]
    concurrency_tests::run_before_mirror_hook(root_path);
    mirror::mirror_registry(root_path, &registry)
  })
}

fn with_registry_lock<T>(operation: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
  let _guard = REGISTRY_LOCK
    .lock()
    .map_err(|_| registry_error("API 配置注册表锁已损坏，请重启应用后重试"))?;
  #[cfg(test)]
  let _state_guard = concurrency_tests::mark_registry_lock_held();
  operation()
}

fn with_mirror_lock<T>(operation: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
  let _guard = MIRROR_LOCK
    .lock()
    .map_err(|_| registry_error("API 配置镜像锁已损坏，请重启应用后重试"))?;
  operation()
}

pub(crate) fn validate_registry(registry: &ApiProfileRegistry) -> AppResult<()> {
  if registry.schema_version != API_PROFILE_SCHEMA_VERSION {
    return Err(registry_error(format!(
      "不支持的 API 配置版本：{}",
      registry.schema_version
    )));
  }

  validate_unique_names(
    registry
      .tikhub_profiles
      .values()
      .map(|profile| profile.name.as_str()),
    "TikHub",
  )?;
  validate_unique_names(
    registry
      .ai_profiles
      .values()
      .map(|profile| profile.name.as_str()),
    "AI",
  )?;

  for (id, profile) in &registry.tikhub_profiles {
    validate_map_id(id, &profile.id, "TikHub 配置")?;
    validate_common_profile(
      &profile.id,
      &profile.name,
      profile.revision,
      &profile.created_at,
      &profile.updated_at,
      profile.last_tested_at.as_deref(),
    )?;
    if !matches!(
      profile.base_url.as_str(),
      "https://api.tikhub.io" | "https://api.tikhub.dev"
    ) {
      return Err(registry_error("TikHub Base URL 不在允许列表中"));
    }
    validate_uuid(&profile.credential_ref_id, "TikHub 密钥引用")?;
    validate_profile_credential(
      registry,
      &profile.id,
      Some(&profile.credential_ref_id),
      Some(CredentialProviderType::Tikhub),
      &profile.status,
      false,
    )?;
  }

  for (id, profile) in &registry.ai_profiles {
    validate_map_id(id, &profile.id, "AI 配置")?;
    validate_common_profile(
      &profile.id,
      &profile.name,
      profile.revision,
      &profile.created_at,
      &profile.updated_at,
      profile.last_tested_at.as_deref(),
    )?;
    validate_ai_profile(profile)?;
    validate_profile_credential(
      registry,
      &profile.id,
      profile.credential_ref_id.as_ref(),
      credential_type_for_ai(&profile.provider_type),
      &profile.status,
      profile.provider_type == AiProviderType::Ollama,
    )?;
  }

  validate_credentials(registry)?;
  validate_active_profile(
    registry.active_profile_ids.tikhub.as_deref(),
    &registry.tikhub_profiles,
    "TikHub",
  )?;
  validate_active_profile(
    registry.active_profile_ids.ai.as_deref(),
    &registry.ai_profiles,
    "AI",
  )
}

fn validate_unique_names<'a>(names: impl Iterator<Item = &'a str>, kind: &str) -> AppResult<()> {
  let mut seen = BTreeSet::new();
  for name in names {
    validate_name(name, kind)?;
    if !seen.insert(name.to_lowercase()) {
      return Err(registry_error(format!("{kind} 配置名称不能重复")));
    }
  }
  Ok(())
}

fn validate_name(name: &str, kind: &str) -> AppResult<()> {
  if name.trim().is_empty() || name.trim() != name {
    return Err(registry_error(format!(
      "{kind} 配置名称不能为空或包含首尾空格"
    )));
  }
  Ok(())
}

fn validate_map_id(map_id: &str, profile_id: &str, label: &str) -> AppResult<()> {
  validate_uuid(profile_id, label)?;
  if map_id != profile_id {
    return Err(registry_error(format!("{label} ID 与对象键不一致")));
  }
  Ok(())
}

fn validate_common_profile(
  id: &str,
  name: &str,
  revision: u64,
  created_at: &str,
  updated_at: &str,
  last_tested_at: Option<&str>,
) -> AppResult<()> {
  validate_uuid(id, "API 配置")?;
  validate_name(name, "API")?;
  if revision == 0 {
    return Err(registry_error("API 配置修订号必须大于零"));
  }
  validate_timestamp(created_at, "创建时间")?;
  validate_timestamp(updated_at, "更新时间")?;
  if let Some(last_tested_at) = last_tested_at {
    validate_timestamp(last_tested_at, "最近测试时间")?;
  }
  Ok(())
}

fn validate_ai_profile(profile: &AiApiProfile) -> AppResult<()> {
  let expected_format = match profile.provider_type {
    AiProviderType::Openai | AiProviderType::CustomOpenaiCompatible => {
      AiApiFormat::OpenaiCompatible
    }
    AiProviderType::Anthropic => AiApiFormat::AnthropicMessages,
    AiProviderType::Gemini => AiApiFormat::Gemini,
    AiProviderType::Ollama => AiApiFormat::Ollama,
  };
  if profile.api_format != expected_format {
    return Err(registry_error("AI 供应商类型与 API 格式不匹配"));
  }
  if (profile.default_model_id.trim().is_empty() && profile.status != ApiProfileStatus::NeedsRebind)
    || profile.default_model_id.trim() != profile.default_model_id
  {
    return Err(registry_error("AI 默认模型 ID 不能为空或包含首尾空格"));
  }
  if profile.base_url.is_empty() {
    if profile.status != ApiProfileStatus::NeedsRebind {
      return Err(registry_error("AI Base URL 必须是有效的 HTTP(S) 地址"));
    }
  } else {
    let url = reqwest::Url::parse(&profile.base_url)
      .map_err(|_| registry_error("AI Base URL 必须是有效的 HTTP(S) 地址"))?;
    if !matches!(url.scheme(), "http" | "https")
      || url.host_str().is_none()
      || !url.username().is_empty()
      || url.password().is_some()
      || url.query().is_some()
      || url.fragment().is_some()
      || profile.base_url.chars().any(char::is_whitespace)
    {
      return Err(registry_error("AI Base URL 不能包含凭据、查询串或片段"));
    }
  }
  if let Some(credential_ref_id) = &profile.credential_ref_id {
    validate_uuid(credential_ref_id, "AI 密钥引用")?;
  }
  Ok(())
}

fn validate_profile_credential(
  registry: &ApiProfileRegistry,
  profile_id: &str,
  credential_ref_id: Option<&String>,
  expected_type: Option<CredentialProviderType>,
  status: &ApiProfileStatus,
  key_optional: bool,
) -> AppResult<()> {
  let credential = credential_ref_id.and_then(|id| registry.credentials.get(id));
  if credential.is_none() && !key_optional && *status != ApiProfileStatus::NeedsRebind {
    return Err(registry_error("API 配置缺少对应的凭据"));
  }
  if let Some(credential) = credential {
    if credential.profile_id != profile_id || Some(credential.provider_type) != expected_type {
      return Err(registry_error("API 凭据与所属配置或供应商不匹配"));
    }
  }
  Ok(())
}

fn validate_credentials(registry: &ApiProfileRegistry) -> AppResult<()> {
  for (id, credential) in &registry.credentials {
    validate_map_id(id, &credential.id, "API 凭据")?;
    validate_uuid(&credential.profile_id, "API 凭据所属配置")?;
    if credential.revision == 0 || credential.secret.trim().is_empty() {
      return Err(registry_error("API 凭据修订号和密钥内容无效"));
    }
    let referenced = registry
      .tikhub_profiles
      .get(&credential.profile_id)
      .is_some_and(|profile| profile.credential_ref_id == credential.id)
      || registry
        .ai_profiles
        .get(&credential.profile_id)
        .is_some_and(|profile| {
          profile.credential_ref_id.as_deref() == Some(credential.id.as_str())
        });
    if !referenced {
      return Err(registry_error("API 凭据没有对应的配置引用"));
    }
  }
  Ok(())
}

fn validate_active_profile<T: ProfileStatus>(
  active_id: Option<&str>,
  profiles: &BTreeMap<String, T>,
  kind: &str,
) -> AppResult<()> {
  let Some(active_id) = active_id else {
    return Ok(());
  };
  let profile = profiles
    .get(active_id)
    .ok_or_else(|| registry_error(format!("当前 {kind} 配置不存在")))?;
  if profile.status() != &ApiProfileStatus::Success {
    return Err(registry_error(format!("当前 {kind} 配置尚未通过验证")));
  }
  Ok(())
}

trait ProfileStatus {
  fn status(&self) -> &ApiProfileStatus;
}

impl ProfileStatus for TikhubApiProfile {
  fn status(&self) -> &ApiProfileStatus {
    &self.status
  }
}

impl ProfileStatus for AiApiProfile {
  fn status(&self) -> &ApiProfileStatus {
    &self.status
  }
}

fn credential_type_for_ai(provider_type: &AiProviderType) -> Option<CredentialProviderType> {
  match provider_type {
    AiProviderType::Openai => Some(CredentialProviderType::Openai),
    AiProviderType::Anthropic => Some(CredentialProviderType::Anthropic),
    AiProviderType::Gemini => Some(CredentialProviderType::Gemini),
    AiProviderType::CustomOpenaiCompatible => Some(CredentialProviderType::CustomOpenaiCompatible),
    AiProviderType::Ollama => Some(CredentialProviderType::Ollama),
  }
}

fn validate_uuid(value: &str, label: &str) -> AppResult<()> {
  Uuid::parse_str(value)
    .map(|_| ())
    .map_err(|_| registry_error(format!("{label}必须是 UUID")))
}

fn validate_timestamp(value: &str, label: &str) -> AppResult<()> {
  DateTime::parse_from_rfc3339(value)
    .map(|_| ())
    .map_err(|_| registry_error(format!("{label}必须是 RFC 3339 时间")))
}

pub(crate) fn registry_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::SecretStoreError,
    message,
    AppErrorStage::SecretStore,
    false,
  )
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::os::unix::fs::{symlink, PermissionsExt};
  use std::sync::{Arc, Barrier};
  use std::thread;

  use chrono::Utc;

  use super::*;

  fn private_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("api-registry-{}", Uuid::new_v4()));
    fs::create_dir(&root).expect("test root should be created");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
      .expect("test root should be private");
    root
  }

  fn now() -> String {
    Utc::now().to_rfc3339()
  }

  fn add_tikhub(registry: &mut ApiProfileRegistry, name: &str, secret: &str) -> String {
    let profile_id = Uuid::new_v4().to_string();
    let credential_id = Uuid::new_v4().to_string();
    let timestamp = now();
    registry.tikhub_profiles.insert(
      profile_id.clone(),
      TikhubApiProfile {
        id: profile_id.clone(),
        name: name.to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        credential_ref_id: credential_id.clone(),
        revision: 1,
        status: ApiProfileStatus::Success,
        last_tested_at: Some(timestamp.clone()),
        test_summary: Some(TikhubSafeTestSummary {
          masked_account: Some("s***@example.test".to_string()),
          balance: Some(8.75),
          free_credit: Some(0.25),
          available_credit: Some(9.0),
          today_usage: Some(0.41),
        }),
        created_at: timestamp.clone(),
        updated_at: timestamp,
      },
    );
    registry.credentials.insert(
      credential_id.clone(),
      ApiCredential {
        id: credential_id,
        provider_type: CredentialProviderType::Tikhub,
        profile_id: profile_id.clone(),
        revision: 1,
        secret: secret.to_string(),
      },
    );
    profile_id
  }

  fn add_ai(
    registry: &mut ApiProfileRegistry,
    name: &str,
    provider_type: AiProviderType,
    secret: &str,
  ) -> String {
    let profile_id = Uuid::new_v4().to_string();
    let credential_id = Uuid::new_v4().to_string();
    let timestamp = now();
    let (api_format, base_url, credential_type) = match provider_type {
      AiProviderType::Openai => (
        AiApiFormat::OpenaiCompatible,
        "https://api.openai.com/v1",
        CredentialProviderType::Openai,
      ),
      AiProviderType::CustomOpenaiCompatible => (
        AiApiFormat::OpenaiCompatible,
        "https://ai.example.test/v1",
        CredentialProviderType::CustomOpenaiCompatible,
      ),
      _ => panic!("test helper only uses OpenAI-compatible providers"),
    };
    registry.ai_profiles.insert(
      profile_id.clone(),
      AiApiProfile {
        id: profile_id.clone(),
        name: name.to_string(),
        provider_type,
        api_format,
        base_url: base_url.to_string(),
        default_model_id: "model-test".to_string(),
        credential_ref_id: Some(credential_id.clone()),
        revision: 1,
        status: ApiProfileStatus::Success,
        last_tested_at: Some(timestamp.clone()),
        created_at: timestamp.clone(),
        updated_at: timestamp,
      },
    );
    registry.credentials.insert(
      credential_id.clone(),
      ApiCredential {
        id: credential_id,
        provider_type: credential_type,
        profile_id: profile_id.clone(),
        revision: 1,
        secret: secret.to_string(),
      },
    );
    profile_id
  }

  #[test]
  fn one_private_json_round_trips_multiple_tikhub_and_ai_profiles() {
    let root = private_root();
    let mut registry = ApiProfileRegistry::default();
    let tikhub_one = add_tikhub(&mut registry, "TikHub 国际", "tk-one-sentinel");
    add_tikhub(&mut registry, "TikHub 国内", "tk-two-sentinel");
    let ai_one = add_ai(
      &mut registry,
      "OpenAI 主账号",
      AiProviderType::Openai,
      "ai-one-sentinel",
    );
    add_ai(
      &mut registry,
      "兼容端点",
      AiProviderType::CustomOpenaiCompatible,
      "ai-two-sentinel",
    );
    registry.active_profile_ids.tikhub = Some(tikhub_one);
    registry.active_profile_ids.ai = Some(ai_one);

    save_api_profile_registry(&root, &registry).expect("registry should save");
    let loaded = load_api_profile_registry(&root).expect("registry should reload");

    assert_eq!(loaded, registry);
    assert_eq!(loaded.tikhub_profiles.len(), 2);
    assert_eq!(loaded.ai_profiles.len(), 2);
    assert_eq!(loaded.credentials.len(), 4);
    let directory_mode = fs::symlink_metadata(root.join("secrets"))
      .unwrap()
      .permissions()
      .mode()
      & 0o7777;
    let file_mode = fs::symlink_metadata(api_profile_registry_path(&root))
      .unwrap()
      .permissions()
      .mode()
      & 0o7777;
    assert_eq!(directory_mode, 0o700);
    assert_eq!(file_mode, 0o600);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn damaged_or_unsupported_registry_is_not_overwritten() {
    let root = private_root();
    let mut registry = ApiProfileRegistry::default();
    add_tikhub(&mut registry, "初始配置", "original-sentinel");
    save_api_profile_registry(&root, &registry).expect("initial registry should save");
    let path = api_profile_registry_path(&root);
    fs::write(&path, b"{ damaged-json").expect("test should corrupt registry");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let damaged = fs::read(&path).unwrap();

    assert!(load_api_profile_registry(&root).is_err());
    assert!(save_api_profile_registry(&root, &ApiProfileRegistry::default()).is_err());
    assert_eq!(fs::read(&path).unwrap(), damaged);

    fs::write(
      &path,
      serde_json::to_vec(&serde_json::json!({
        "schema_version": 99,
        "active_profile_ids": { "tikhub": null, "ai": null },
        "tikhub_profiles": {},
        "ai_profiles": {},
        "credentials": {}
      }))
      .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(load_api_profile_registry(&root).is_err());
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn registry_rejects_symlinks_and_broad_permissions() {
    let root = private_root();
    fs::create_dir(root.join("secrets")).unwrap();
    fs::set_permissions(root.join("secrets"), fs::Permissions::from_mode(0o700)).unwrap();
    let outside = root.with_extension("outside.json");
    fs::write(&outside, b"outside").unwrap();
    symlink(&outside, api_profile_registry_path(&root)).unwrap();

    assert!(load_api_profile_registry(&root).is_err());
    fs::remove_file(api_profile_registry_path(&root)).unwrap();
    save_api_profile_registry(&root, &ApiProfileRegistry::default()).unwrap();
    fs::set_permissions(
      api_profile_registry_path(&root),
      fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(load_api_profile_registry(&root).is_err());
    fs::remove_dir_all(root).ok();
    fs::remove_file(outside).ok();
  }

  #[test]
  fn concurrent_updates_do_not_drop_profiles() {
    let root = Arc::new(private_root());
    save_api_profile_registry(root.as_ref(), &ApiProfileRegistry::default()).unwrap();
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
      .map(|index| {
        let root = Arc::clone(&root);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
          barrier.wait();
          update_api_profile_registry(root.as_ref(), |registry| {
            add_ai(
              registry,
              &format!("AI 配置 {index}"),
              AiProviderType::CustomOpenaiCompatible,
              &format!("concurrent-sentinel-{index}"),
            );
            Ok(())
          })
        })
      })
      .collect::<Vec<_>>();
    for handle in handles {
      handle.join().unwrap().unwrap();
    }

    let loaded = load_api_profile_registry(root.as_ref()).unwrap();
    assert_eq!(loaded.ai_profiles.len(), 8);
    assert_eq!(loaded.credentials.len(), 8);
    fs::remove_dir_all(root.as_ref()).ok();
  }
}
