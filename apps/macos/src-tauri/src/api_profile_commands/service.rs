use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Transaction, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use super::validation::{
  ai_url, completeness_status, credential_type, error, key_status, next_revision, optional_id,
  required, same_url_authority, secret, tikhub_url, validate_ai_format, validate_ai_url,
};
pub(super) use super::ServiceTestResult;
use super::{ApiProfileKind, ApiProfileRegistryView, SaveApiProfileInput};
use crate::ai::provider_client::{call_model, connection_test_request, ProviderConfig};
use crate::api_profiles::{
  load_api_profile_safe_snapshot, load_existing_api_profile_registry, rebuild_api_profile_mirror,
  save_api_profile_registry, update_api_profile_registry, with_api_profile_mirror_lock,
  AiApiFormat, AiApiProfile, AiProviderType, ApiCredential, ApiProfileRegistry, ApiProfileStatus,
  CredentialProviderType, TikhubApiProfile, TikhubSafeTestSummary,
};
use crate::domain::{redact_sensitive_text, AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tikhub::{self, TikhubConnectionTestResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

type ProfileTestResult = AppResult<ServiceTestResult>;

pub(super) fn get_registry(root: &Path) -> AppResult<ApiProfileRegistry> {
  load_existing_api_profile_registry(root)?
    .ok_or_else(|| error("API 配置文件不存在，请重新打开工作区后再试"))
}

pub(super) fn get_registry_view(root: &Path) -> AppResult<ApiProfileRegistryView> {
  serde_json::from_value(load_api_profile_safe_snapshot(root)?)
    .map_err(|_| safe_snapshot_error("API 配置安全状态镜像无法解析"))
}

pub(super) fn save_profile(
  root: &Path,
  input: SaveApiProfileInput,
) -> AppResult<ApiProfileRegistry> {
  let (kind, editing_id) = match &input {
    SaveApiProfileInput::Tikhub { id, .. } => (ApiProfileKind::Tikhub, id.clone()),
    SaveApiProfileInput::Ai { id, .. } => (ApiProfileKind::Ai, id.clone()),
  };
  mutate_registry(root, |transaction, registry| {
    if let Some(id) = editing_id.as_deref() {
      ensure_mutable(transaction, registry, kind, id)?;
      #[cfg(test)]
      super::tests::run_after_mutability_check_hook();
    }
    let (profile_id, created) = match input {
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
    }?;
    audit(
      transaction,
      "save_api_profile",
      &profile_id,
      serde_json::json!({"kind": kind_name(kind), "created": created}),
    )?;
    Ok(())
  })?;
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
    let (mut credential_id, previous_provider, previous_base_url, revision) = registry
      .ai_profiles
      .get(&id)
      .map(|profile| {
        Ok((
          profile.credential_ref_id.clone(),
          profile.provider_type,
          profile.base_url.clone(),
          next_revision(profile.revision)?,
        ))
      })
      .transpose()?
      .ok_or_else(|| error("AI 配置不存在"))?;
    let provider_changed = previous_provider != provider;
    let endpoint_changed = !same_url_authority(&previous_base_url, &base_url);
    let credential_scope_changed = provider_changed || endpoint_changed;
    if credential_scope_changed && key.is_none() && provider != AiProviderType::Ollama {
      return Err(error("切换 AI 供应商或端点时必须重新输入 API Key"));
    }
    if credential_scope_changed && provider == AiProviderType::Ollama && key.is_none() {
      if let Some(previous_credential_id) = credential_id.take() {
        registry.credentials.remove(&previous_credential_id);
      }
    }
    if (key.is_some() || provider != AiProviderType::Ollama) && credential_id.is_none() {
      credential_id = Some(Uuid::new_v4().to_string());
    }
    if let (Some(key), Some(credential_id)) = (key, credential_id.as_deref()) {
      put_credential(registry, credential_id, &id, credential_type(provider), key)?;
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

pub(super) fn test_profile(root: &Path, kind: ApiProfileKind, id: &str) -> ProfileTestResult {
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
    persist_tikhub(
      root,
      &profile,
      None,
      ApiProfileStatus::NeedsRebind,
      None,
      false,
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
  persist_tikhub(
    root,
    &profile,
    Some(credential.revision),
    status,
    summary,
    status == ApiProfileStatus::Success,
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
  may_auto_activate: bool,
  audit_details: Value,
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  mutate_registry(root, |transaction, registry| {
    let revision = registry
      .credentials
      .get(&expected.credential_ref_id)
      .map(|credential| credential.revision);
    let auto_activate = may_auto_activate
      && auto_activation_allowed(
        transaction,
        ApiProfileKind::Tikhub,
        &expected.id,
        registry.tikhub_profiles.len(),
      )?;
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
    profile.status = status;
    profile.last_tested_at = Some(now.clone());
    profile.test_summary = summary;
    profile.updated_at = now.clone();
    if auto_activate && registry.active_profile_ids.tikhub.is_none() {
      registry.active_profile_ids.tikhub = Some(expected.id.clone());
    } else if status != ApiProfileStatus::Success
      && registry.active_profile_ids.tikhub.as_deref() == Some(expected.id.as_str())
    {
      registry.active_profile_ids.tikhub = None;
    }
    audit(transaction, "test_api_profile", &expected.id, audit_details)?;
    Ok(())
  })
}

fn test_ai(root: &Path, id: &str) -> AppResult<ServiceTestResult> {
  let registry = get_registry(root)?;
  let profile = registry
    .ai_profiles
    .get(id)
    .cloned()
    .ok_or_else(|| error("AI 配置不存在"))?;
  let expected_key_revision = credential_revision(&registry, &profile);
  let outcome = test_ai_connection(&registry, &profile);
  let success = outcome.is_ok();
  let (message, error_code) = match outcome {
    Ok(message) => (message, None),
    Err(failure) => (failure.message, Some(format!("{:?}", failure.code))),
  };
  let status = if success {
    ApiProfileStatus::Success
  } else if may_store_failed(&registry, &profile) {
    ApiProfileStatus::Failed
  } else {
    ApiProfileStatus::NeedsRebind
  };
  let now = Utc::now().to_rfc3339();
  mutate_registry(root, |transaction, registry| {
    let current_revision = credential_revision(registry, &profile);
    let auto_activate = success
      && auto_activation_allowed(
        transaction,
        ApiProfileKind::Ai,
        id,
        registry.ai_profiles.len(),
      )?;
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
    current.status = status;
    current.last_tested_at = Some(now.clone());
    current.updated_at = now.clone();
    if auto_activate && registry.active_profile_ids.ai.is_none() {
      registry.active_profile_ids.ai = Some(id.to_string());
    } else if !success && registry.active_profile_ids.ai.as_deref() == Some(id) {
      registry.active_profile_ids.ai = None;
    }
    audit(
      transaction,
      "test_api_profile",
      id,
      serde_json::json!({"kind":"ai", "success":success, "error_code":error_code}),
    )?;
    Ok(())
  })?;
  result(root, success, &message)
}

pub(super) fn activate_profile(
  root: &Path,
  kind: ApiProfileKind,
  id: &str,
) -> AppResult<ApiProfileRegistry> {
  mutate_registry(root, |transaction, registry| {
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
    audit(
      transaction,
      "activate_api_profile",
      id,
      serde_json::json!({"kind":kind_name(kind)}),
    )?;
    Ok(())
  })?;
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
    let was_active = match kind {
      ApiProfileKind::Tikhub => registry.active_profile_ids.tikhub.as_deref() == Some(id),
      ApiProfileKind::Ai => registry.active_profile_ids.ai.as_deref() == Some(id),
    };
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
    audit(
      transaction,
      "delete_api_profile",
      id,
      serde_json::json!({"kind":kind_name(kind), "was_active":was_active}),
    )?;
    Ok(())
  })?;
  get_registry(root)
}

fn mutate_registry<T>(
  root: &Path,
  update: impl FnOnce(&Transaction<'_>, &mut ApiProfileRegistry) -> AppResult<T>,
) -> AppResult<T> {
  with_api_profile_mirror_lock(|| {
    let previous_registry = get_registry(root)?;
    let mut connection = open_workspace_database(root.join(DATABASE_FILE_NAME))?;
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    let (result, updated_registry) = update_api_profile_registry(root, |registry| {
      let result = update(&transaction, registry)?;
      Ok((result, registry.clone()))
    })?;
    let database_result = rebuild_api_profile_mirror(&transaction, &updated_registry)
      .and_then(|_| transaction.commit().map_err(database_error));
    if let Err(operation_error) = database_result {
      if let Err(restore_error) = save_api_profile_registry(root, &previous_registry) {
        return Err(AppError::new(
          AppErrorCode::SecretStoreError,
          format!(
            "API 配置提交失败且旧 JSON 恢复失败：{}；恢复错误：{}",
            operation_error.message, restore_error.message
          ),
          AppErrorStage::SecretStore,
          false,
        ));
      }
      return Err(operation_error);
    }
    Ok(result)
  })
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
fn test_ai_connection(registry: &ApiProfileRegistry, profile: &AiApiProfile) -> AppResult<String> {
  validate_ai_format(profile.provider_type, profile.api_format)?;
  if profile.default_model_id.trim().is_empty() {
    return Err(error("AI 默认模型 ID 不能为空"));
  }
  validate_ai_url(profile.provider_type, &profile.base_url)?;
  let api_key = profile
    .credential_ref_id
    .as_ref()
    .and_then(|value| registry.credentials.get(value))
    .map(|credential| credential.secret.clone());
  if profile.provider_type != AiProviderType::Ollama && api_key.is_none() {
    return Err(error("AI API Key 需要重新输入后才能测试"));
  }
  #[cfg(test)]
  if profile.base_url == "https://api.openai.com/v1" {
    return Ok("AI 单元测试桩连通成功".to_string());
  }
  let response = call_model(
    &ProviderConfig {
      provider_type: profile.provider_type,
      api_format: profile.api_format,
      base_url: profile.base_url.clone(),
      model_id: profile.default_model_id.clone(),
      api_key,
    },
    &connection_test_request(),
  )?;
  if response.output_json.get("ok").and_then(Value::as_bool) != Some(true) {
    return Err(AppError::new(
      AppErrorCode::ModelSchemaError,
      "AI 服务已响应，但未返回连通性测试要求的结构化结果",
      AppErrorStage::Ai,
      false,
    ));
  }
  Ok(format!(
    "AI 服务连通成功，模型 {} 已返回结构化 JSON",
    profile.default_model_id
  ))
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

fn safe_snapshot_error(message: &str) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    message,
    AppErrorStage::Database,
    false,
  )
}

fn auto_activation_allowed(
  transaction: &Transaction<'_>,
  kind: ApiProfileKind,
  profile_id: &str,
  profile_count: usize,
) -> AppResult<bool> {
  let mut statement = transaction
    .prepare(
      "SELECT entity_id,action,safe_details_json FROM audit_log
       WHERE entity_type = 'api_profile'
         AND action IN ('save_api_profile','delete_api_profile')
       ORDER BY rowid",
    )
    .map_err(database_error)?;
  let entries = statement
    .query_map([], |row| {
      Ok((
        row.get::<_, Option<String>>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
      ))
    })
    .map_err(database_error)?;
  let expected_kind = kind_name(kind);
  let mut first_created_id = None;
  let mut any_delete = false;
  let mut deleted_current = false;
  for entry in entries {
    let (entity_id, action, details) = entry.map_err(database_error)?;
    let Ok(details) = serde_json::from_str::<Value>(&details) else {
      return Ok(false);
    };
    if details.get("kind").and_then(Value::as_str) != Some(expected_kind) {
      continue;
    }
    if action == "save_api_profile" && details.get("created").and_then(Value::as_bool) == Some(true)
    {
      let Some(entity_id) = entity_id else {
        return Ok(false);
      };
      first_created_id.get_or_insert(entity_id);
    } else if action == "delete_api_profile" {
      any_delete = true;
      deleted_current |= details
        .get("was_active")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    }
  }
  if deleted_current {
    return Ok(false);
  }
  if let Some(first_created_id) = first_created_id {
    return Ok(first_created_id == profile_id);
  }
  Ok(!any_delete && profile_count == 1)
}

fn audit(transaction: &Transaction<'_>, action: &str, id: &str, details: Value) -> AppResult<()> {
  transaction
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
