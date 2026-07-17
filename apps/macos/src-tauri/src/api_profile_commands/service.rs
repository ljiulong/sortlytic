use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Transaction, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use super::validation::{
  ai_url, completeness_status, credential_type, error, key_status, next_revision, optional_id,
  required, secret, tikhub_url, validate_ai_format, validate_ai_url,
};
use super::{ApiProfileKind, SaveApiProfileInput};
use crate::api_profiles::{
  load_existing_api_profile_registry, sync_api_profile_mirror, update_api_profile_registry,
  AiApiFormat, AiApiProfile, AiProviderType, ApiCredential, ApiProfileRegistry, ApiProfileStatus,
  CredentialProviderType, TikhubApiProfile, TikhubSafeTestSummary,
};
use crate::domain::{redact_sensitive_text, AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tikhub::{self, TikhubConnectionTestResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

pub(super) struct ServiceTestResult {
  pub success: bool,
  pub message: String,
  pub registry: ApiProfileRegistry,
}

pub(super) fn get_registry(root: &Path) -> AppResult<ApiProfileRegistry> {
  load_existing_api_profile_registry(root)?
    .ok_or_else(|| error("API 配置文件不存在，请重新打开工作区后再试"))
}

pub(super) fn save_profile(
  root: &Path,
  input: SaveApiProfileInput,
) -> AppResult<ApiProfileRegistry> {
  let (kind, editing_id) = match &input {
    SaveApiProfileInput::Tikhub { id, .. } => (ApiProfileKind::Tikhub, id.clone()),
    SaveApiProfileInput::Ai { id, .. } => (ApiProfileKind::Ai, id.clone()),
  };
  let (profile_id, created) = mutate_registry(root, |transaction, registry| {
    if let Some(id) = editing_id.as_deref() {
      ensure_mutable(transaction, registry, kind, id)?;
      #[cfg(test)]
      super::tests::run_after_mutability_check_hook();
    }
    match input {
      SaveApiProfileInput::Tikhub {
        id,
        name,
        base_url,
        api_key,
      } => save_tikhub(registry, id, name, base_url, api_key),
      SaveApiProfileInput::Ai {
        id,
        name,
        provider_type,
        api_format,
        base_url,
        default_model_id,
        api_key,
      } => save_ai(
        registry,
        id,
        name,
        provider_type,
        api_format,
        base_url,
        default_model_id,
        api_key,
      ),
    }
  })?;
  audit(
    root,
    "save_api_profile",
    &profile_id,
    serde_json::json!({"kind": kind_name(kind), "created": created}),
  )?;
  get_registry(root)
}

fn save_tikhub(
  registry: &mut ApiProfileRegistry,
  id: Option<String>,
  name: String,
  base_url: String,
  api_key: Option<String>,
) -> AppResult<(String, bool)> {
  let name = required(&name, "TikHub 配置名称")?;
  let base_url = tikhub_url(&base_url)?;
  let key = secret(api_key);
  let now = Utc::now().to_rfc3339();
  if let Some(id) = optional_id(id)? {
    let (credential_id, revision) = registry
      .tikhub_profiles
      .get(&id)
      .map(|profile| {
        Ok((
          profile.credential_ref_id.clone(),
          next_revision(profile.revision)?,
        ))
      })
      .transpose()?
      .ok_or_else(|| error("TikHub 配置不存在"))?;
    if let Some(key) = key {
      put_credential(
        registry,
        &credential_id,
        &id,
        CredentialProviderType::Tikhub,
        key,
      )?;
    }
    let has_key = registry.credentials.contains_key(&credential_id);
    let profile = registry.tikhub_profiles.get_mut(&id).unwrap();
    profile.name = name;
    profile.base_url = base_url;
    profile.revision = revision;
    profile.status = key_status(has_key);
    profile.last_tested_at = None;
    profile.test_summary = None;
    profile.updated_at = now;
    if registry.active_profile_ids.tikhub.as_deref() == Some(&id) {
      registry.active_profile_ids.tikhub = None;
    }
    return Ok((id, false));
  }
  let id = Uuid::new_v4().to_string();
  let credential_id = Uuid::new_v4().to_string();
  let has_key = key.is_some();
  if let Some(key) = key {
    put_credential(
      registry,
      &credential_id,
      &id,
      CredentialProviderType::Tikhub,
      key,
    )?;
  }
  registry.tikhub_profiles.insert(
    id.clone(),
    TikhubApiProfile {
      id: id.clone(),
      name,
      base_url,
      credential_ref_id: credential_id,
      revision: 1,
      status: key_status(has_key),
      last_tested_at: None,
      test_summary: None,
      created_at: now.clone(),
      updated_at: now,
    },
  );
  Ok((id, true))
}

#[allow(clippy::too_many_arguments)]
fn save_ai(
  registry: &mut ApiProfileRegistry,
  id: Option<String>,
  name: String,
  provider: AiProviderType,
  format: AiApiFormat,
  base_url: String,
  model: String,
  api_key: Option<String>,
) -> AppResult<(String, bool)> {
  let name = required(&name, "AI 配置名称")?;
  validate_ai_format(provider, format)?;
  let base_url = ai_url(provider, &base_url)?;
  let model = model.trim().to_string();
  let key = secret(api_key);
  let now = Utc::now().to_rfc3339();
  if let Some(id) = optional_id(id)? {
    let (mut credential_id, revision) = registry
      .ai_profiles
      .get(&id)
      .map(|profile| {
        Ok((
          profile.credential_ref_id.clone(),
          next_revision(profile.revision)?,
        ))
      })
      .transpose()?
      .ok_or_else(|| error("AI 配置不存在"))?;
    if (key.is_some() || provider != AiProviderType::Ollama) && credential_id.is_none() {
      credential_id = Some(Uuid::new_v4().to_string());
    }
    if let (Some(key), Some(credential_id)) = (key, credential_id.as_deref()) {
      put_credential(registry, credential_id, &id, credential_type(provider), key)?;
    } else if let Some(credential) = credential_id
      .as_ref()
      .and_then(|credential_id| registry.credentials.get_mut(credential_id))
    {
      credential.provider_type = credential_type(provider);
    }
    let has_key = credential_id
      .as_ref()
      .is_some_and(|value| registry.credentials.contains_key(value));
    let profile = registry.ai_profiles.get_mut(&id).unwrap();
    profile.name = name;
    profile.provider_type = provider;
    profile.api_format = format;
    profile.base_url = base_url;
    profile.default_model_id = model;
    profile.credential_ref_id = credential_id;
    profile.revision = revision;
    profile.status = completeness_status(
      provider,
      &profile.base_url,
      &profile.default_model_id,
      has_key,
    );
    profile.last_tested_at = None;
    profile.updated_at = now;
    if registry.active_profile_ids.ai.as_deref() == Some(&id) {
      registry.active_profile_ids.ai = None;
    }
    return Ok((id, false));
  }
  let id = Uuid::new_v4().to_string();
  let mut credential_id = key.as_ref().map(|_| Uuid::new_v4().to_string());
  if provider != AiProviderType::Ollama && credential_id.is_none() {
    credential_id = Some(Uuid::new_v4().to_string());
  }
  if let (Some(key), Some(credential_id)) = (key, credential_id.as_deref()) {
    put_credential(registry, credential_id, &id, credential_type(provider), key)?;
  }
  let has_key = credential_id
    .as_ref()
    .is_some_and(|value| registry.credentials.contains_key(value));
  registry.ai_profiles.insert(
    id.clone(),
    AiApiProfile {
      id: id.clone(),
      name,
      provider_type: provider,
      api_format: format,
      base_url: base_url.clone(),
      default_model_id: model.clone(),
      credential_ref_id: credential_id,
      revision: 1,
      status: completeness_status(provider, &base_url, &model, has_key),
      last_tested_at: None,
      created_at: now.clone(),
      updated_at: now,
    },
  );
  Ok((id, true))
}

pub(super) fn test_profile(
  root: &Path,
  kind: ApiProfileKind,
  id: &str,
) -> AppResult<ServiceTestResult> {
  test_profile_with(root, kind, id, |root, secret_id, base_url| {
    tikhub::test_tikhub_connection(root, secret_id, base_url)
  })
}

pub(super) fn test_profile_with<F>(
  root: &Path,
  kind: ApiProfileKind,
  id: &str,
  tester: F,
) -> AppResult<ServiceTestResult>
where
  F: FnOnce(&Path, &str, Option<String>) -> AppResult<TikhubConnectionTestResult>,
{
  match kind {
    ApiProfileKind::Tikhub => test_tikhub(root, id, tester),
    ApiProfileKind::Ai => test_ai(root, id),
  }
}

fn test_tikhub<F>(root: &Path, id: &str, tester: F) -> AppResult<ServiceTestResult>
where
  F: FnOnce(&Path, &str, Option<String>) -> AppResult<TikhubConnectionTestResult>,
{
  let registry = get_registry(root)?;
  let profile = registry
    .tikhub_profiles
    .get(id)
    .cloned()
    .ok_or_else(|| error("TikHub 配置不存在"))?;
  let credential = registry
    .credentials
    .get(&profile.credential_ref_id)
    .cloned();
  let Some(credential) = credential else {
    persist_tikhub(root, &profile, None, ApiProfileStatus::NeedsRebind, None)?;
    audit(
      root,
      "test_api_profile",
      id,
      serde_json::json!({"kind":"tikhub", "success":false, "error_code":"NEEDS_REBIND"}),
    )?;
    return result(root, false, "TikHub Token 需要重新输入后才能测试");
  };
  let outcome = tester(
    root,
    &profile.credential_ref_id,
    Some(profile.base_url.clone()),
  );
  let (success, message, status, summary, error_code) = match outcome {
    Ok(value) => (
      true,
      value.message,
      ApiProfileStatus::Success,
      Some(TikhubSafeTestSummary {
        masked_account: value.masked_email,
        balance: value.balance,
        free_credit: value.free_credit,
        available_credit: value.available_credit,
        today_usage: today_usage(&value.daily_usage_json),
      }),
      None,
    ),
    Err(failure) => (
      false,
      safe_message(&failure.message, &credential.secret),
      ApiProfileStatus::Failed,
      None,
      Some(format!("{:?}", failure.code)),
    ),
  };
  persist_tikhub(root, &profile, Some(credential.revision), status, summary)?;
  audit(
    root,
    "test_api_profile",
    id,
    serde_json::json!({"kind":"tikhub", "success":success, "error_code":error_code}),
  )?;
  result(root, success, &message)
}

fn persist_tikhub(
  root: &Path,
  expected: &TikhubApiProfile,
  credential_revision: Option<u64>,
  status: ApiProfileStatus,
  summary: Option<TikhubSafeTestSummary>,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  update_api_profile_registry(root, |registry| {
    let revision = registry
      .credentials
      .get(&expected.credential_ref_id)
      .map(|credential| credential.revision);
    let profile = registry
      .tikhub_profiles
      .get_mut(&expected.id)
      .ok_or_else(|| error("TikHub 配置已在测试期间删除"))?;
    if profile.revision != expected.revision
      || profile.credential_ref_id != expected.credential_ref_id
      || revision != credential_revision
    {
      return Err(error("TikHub 配置已在测试期间变更，请重新测试"));
    }
    let was_successful = profile.status == ApiProfileStatus::Success;
    profile.status = status;
    profile.last_tested_at = Some(now.clone());
    profile.test_summary = summary;
    profile.updated_at = now.clone();
    if status == ApiProfileStatus::Success
      && !was_successful
      && registry.active_profile_ids.tikhub.is_none()
      && registry.tikhub_profiles.len() == 1
    {
      registry.active_profile_ids.tikhub = Some(expected.id.clone());
    } else if status != ApiProfileStatus::Success
      && registry.active_profile_ids.tikhub.as_deref() == Some(expected.id.as_str())
    {
      registry.active_profile_ids.tikhub = None;
    }
    Ok(())
  })?;
  sync_api_profile_mirror(root)
}

fn test_ai(root: &Path, id: &str) -> AppResult<ServiceTestResult> {
  let registry = get_registry(root)?;
  let profile = registry
    .ai_profiles
    .get(id)
    .cloned()
    .ok_or_else(|| error("AI 配置不存在"))?;
  let expected_key_revision = credential_revision(&registry, &profile);
  let validation = validate_ai(&registry, &profile);
  let success = validation.is_ok();
  let message = validation.unwrap_or_else(|message| message);
  let status = if success {
    ApiProfileStatus::Success
  } else if may_store_failed(&registry, &profile) {
    ApiProfileStatus::Failed
  } else {
    ApiProfileStatus::NeedsRebind
  };
  let now = Utc::now().to_rfc3339();
  update_api_profile_registry(root, |registry| {
    let current_revision = credential_revision(registry, &profile);
    let current = registry
      .ai_profiles
      .get_mut(id)
      .ok_or_else(|| error("AI 配置已在校验期间删除"))?;
    if current.revision != profile.revision
      || current.credential_ref_id != profile.credential_ref_id
      || current_revision != expected_key_revision
    {
      return Err(error("AI 配置已在校验期间变更，请重新校验"));
    }
    let was_successful = current.status == ApiProfileStatus::Success;
    current.status = status;
    current.last_tested_at = Some(now.clone());
    current.updated_at = now.clone();
    if success
      && !was_successful
      && registry.active_profile_ids.ai.is_none()
      && registry.ai_profiles.len() == 1
    {
      registry.active_profile_ids.ai = Some(id.to_string());
    } else if !success && registry.active_profile_ids.ai.as_deref() == Some(id) {
      registry.active_profile_ids.ai = None;
    }
    Ok(())
  })?;
  sync_api_profile_mirror(root)?;
  audit(
    root,
    "test_api_profile",
    id,
    serde_json::json!({"kind":"ai", "success":success}),
  )?;
  result(root, success, &message)
}

pub(super) fn activate_profile(
  root: &Path,
  kind: ApiProfileKind,
  id: &str,
) -> AppResult<ApiProfileRegistry> {
  get_registry(root)?;
  update_api_profile_registry(root, |registry| {
    let status = match kind {
      ApiProfileKind::Tikhub => registry.tikhub_profiles.get(id).map(|value| value.status),
      ApiProfileKind::Ai => registry.ai_profiles.get(id).map(|value| value.status),
    }
    .ok_or_else(|| error("要激活的 API 配置不存在"))?;
    if status != ApiProfileStatus::Success {
      return Err(error("API 配置尚未通过测试或完整性校验，不能设为当前"));
    }
    match kind {
      ApiProfileKind::Tikhub => registry.active_profile_ids.tikhub = Some(id.to_string()),
      ApiProfileKind::Ai => registry.active_profile_ids.ai = Some(id.to_string()),
    }
    Ok(())
  })?;
  sync_api_profile_mirror(root)?;
  audit(
    root,
    "activate_api_profile",
    id,
    serde_json::json!({"kind":kind_name(kind)}),
  )?;
  get_registry(root)
}

pub(super) fn delete_profile(
  root: &Path,
  kind: ApiProfileKind,
  id: &str,
) -> AppResult<ApiProfileRegistry> {
  mutate_registry(root, |transaction, registry| {
    ensure_mutable(transaction, registry, kind, id)?;
    #[cfg(test)]
    super::tests::run_after_mutability_check_hook();
    let credential_id = match kind {
      ApiProfileKind::Tikhub => registry
        .tikhub_profiles
        .remove(id)
        .map(|value| Some(value.credential_ref_id)),
      ApiProfileKind::Ai => registry
        .ai_profiles
        .remove(id)
        .map(|value| value.credential_ref_id),
    }
    .ok_or_else(|| error("要删除的 API 配置不存在"))?;
    if let Some(credential_id) = credential_id {
      registry.credentials.remove(&credential_id);
    }
    if registry.active_profile_ids.tikhub.as_deref() == Some(id) {
      registry.active_profile_ids.tikhub = None;
    }
    if registry.active_profile_ids.ai.as_deref() == Some(id) {
      registry.active_profile_ids.ai = None;
    }
    Ok(())
  })?;
  audit(
    root,
    "delete_api_profile",
    id,
    serde_json::json!({"kind":kind_name(kind)}),
  )?;
  get_registry(root)
}

fn mutate_registry<T>(
  root: &Path,
  update: impl FnOnce(&Transaction<'_>, &mut ApiProfileRegistry) -> AppResult<T>,
) -> AppResult<T> {
  let mut connection = open_workspace_database(root.join(DATABASE_FILE_NAME))?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  get_registry(root)?;
  let result = update_api_profile_registry(root, |registry| update(&transaction, registry))?;
  transaction.commit().map_err(database_error)?;
  sync_api_profile_mirror(root)?;
  Ok(result)
}

fn ensure_mutable(
  transaction: &Transaction<'_>,
  registry: &ApiProfileRegistry,
  kind: ApiProfileKind,
  id: &str,
) -> AppResult<()> {
  if kind == ApiProfileKind::Ai {
    return Ok(());
  }
  let credential_ref_id = registry
    .tikhub_profiles
    .get(id)
    .map(|profile| profile.credential_ref_id.clone())
    .unwrap_or_default();
  let referenced: i64 = transaction
    .query_row(
      "SELECT EXISTS (
         SELECT 1 FROM collection_runtime_snapshot AS snapshot
         JOIN task_run AS run ON run.id = snapshot.task_run_id
         WHERE (snapshot.secret_ref_id = ?1 OR snapshot.secret_provider_id = ?2)
           AND run.status IN ('queued','running')
       )",
      params![credential_ref_id, id],
      |row| row.get(0),
    )
    .map_err(database_error)?;
  if referenced != 0 {
    return Err(error(
      "该 TikHub 配置正被运行中或恢复中的任务快照引用，不能编辑或删除",
    ));
  }
  Ok(())
}
fn result(root: &Path, success: bool, message: &str) -> AppResult<ServiceTestResult> {
  Ok(ServiceTestResult {
    success,
    message: redact_sensitive_text(message),
    registry: get_registry(root)?,
  })
}
fn validate_ai(registry: &ApiProfileRegistry, profile: &AiApiProfile) -> Result<String, String> {
  validate_ai_format(profile.provider_type, profile.api_format).map_err(|value| value.message)?;
  if profile.default_model_id.trim().is_empty() {
    return Err("AI 默认模型 ID 不能为空".to_string());
  }
  validate_ai_url(&profile.base_url).map_err(|value| value.message)?;
  let has_key = profile
    .credential_ref_id
    .as_ref()
    .is_some_and(|value| registry.credentials.contains_key(value));
  if profile.provider_type != AiProviderType::Ollama && !has_key {
    return Err("AI API Key 需要重新输入后才能校验".to_string());
  }
  Ok("AI API 配置完整性校验通过；本版本不会发起真实模型请求".to_string())
}
fn may_store_failed(registry: &ApiProfileRegistry, profile: &AiApiProfile) -> bool {
  let has_key = profile
    .credential_ref_id
    .as_ref()
    .is_some_and(|value| registry.credentials.contains_key(value));
  !profile.base_url.is_empty()
    && !profile.default_model_id.is_empty()
    && (profile.provider_type == AiProviderType::Ollama || has_key)
}
fn credential_revision(registry: &ApiProfileRegistry, profile: &AiApiProfile) -> Option<u64> {
  profile
    .credential_ref_id
    .as_ref()
    .and_then(|value| registry.credentials.get(value))
    .map(|value| value.revision)
}
fn put_credential(
  registry: &mut ApiProfileRegistry,
  id: &str,
  profile_id: &str,
  provider_type: CredentialProviderType,
  secret: String,
) -> AppResult<()> {
  let revision = registry
    .credentials
    .get(id)
    .map(|value| next_revision(value.revision))
    .transpose()?
    .unwrap_or(1);
  registry.credentials.insert(
    id.to_string(),
    ApiCredential {
      id: id.to_string(),
      provider_type,
      profile_id: profile_id.to_string(),
      revision,
      secret,
    },
  );
  Ok(())
}
fn today_usage(value: &Value) -> Option<f64> {
  [
    "/total_requests",
    "/today_usage",
    "/data/total_requests",
    "/data/today_usage",
  ]
  .iter()
  .find_map(|pointer| {
    value.pointer(pointer).and_then(|value| {
      value
        .as_f64()
        .or_else(|| value.as_i64().map(|number| number as f64))
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    })
  })
  .filter(|value| value.is_finite() && *value >= 0.0)
}

fn safe_message(message: &str, secret: &str) -> String {
  redact_sensitive_text(&message.replace(secret, "[REDACTED]"))
}

fn audit(root: &Path, action: &str, id: &str, details: Value) -> AppResult<()> {
  open_workspace_database(root.join(DATABASE_FILE_NAME))?
    .execute(
      "INSERT INTO audit_log (id,entity_type,entity_id,action,safe_details_json,created_at)
       VALUES (?1,'api_profile',?2,?3,?4,?5)",
      params![
        Uuid::new_v4().to_string(),
        id,
        action,
        details.to_string(),
        Utc::now().to_rfc3339()
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn kind_name(kind: ApiProfileKind) -> &'static str {
  match kind {
    ApiProfileKind::Tikhub => "tikhub",
    ApiProfileKind::Ai => "ai",
  }
}

fn database_error(value: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    value.to_string(),
    AppErrorStage::Database,
    false,
  )
}
