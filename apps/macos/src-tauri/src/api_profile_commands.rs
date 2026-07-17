use serde::{Deserialize, Serialize};

use crate::api_profiles::{
  AiApiFormat, AiProviderType, ApiProfileRegistry, ApiProfileStatus, TikhubSafeTestSummary,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::secrets::mask_secret;

use super::{resolve_workspace_root, AppState};

#[cfg(test)]
mod activation_tests;
#[cfg(test)]
mod audit_atomicity_tests;
#[cfg(test)]
mod mutation_race_tests;
mod service;
mod validation;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiProfileKind {
  Tikhub,
  Ai,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SaveApiProfileInput {
  Tikhub {
    id: Option<String>,
    name: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
  },
  Ai {
    id: Option<String>,
    name: String,
    #[serde(rename = "providerType")]
    provider_type: AiProviderType,
    #[serde(rename = "apiFormat")]
    api_format: AiApiFormat,
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "defaultModelId")]
    default_model_id: String,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
  },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiProfileRegistryView {
  pub active_profile_ids: SafeActiveProfileIds,
  pub tikhub_profiles: Vec<TikhubApiProfileView>,
  pub ai_profiles: Vec<AiApiProfileView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SafeActiveProfileIds {
  pub tikhub: Option<String>,
  pub ai: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TikhubApiProfileView {
  pub kind: ApiProfileKind,
  pub id: String,
  pub name: String,
  pub base_url: String,
  pub status: ApiProfileStatus,
  pub revision: u64,
  pub masked_key: Option<String>,
  pub has_credential: bool,
  pub is_active: bool,
  pub last_tested_at: Option<String>,
  pub test_summary: Option<TikhubTestSummaryView>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TikhubTestSummaryView {
  pub masked_account: Option<String>,
  pub balance: Option<f64>,
  pub free_credit: Option<f64>,
  pub available_credit: Option<f64>,
  pub today_usage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiApiProfileView {
  pub kind: ApiProfileKind,
  pub id: String,
  pub name: String,
  pub provider_type: AiProviderType,
  pub api_format: AiApiFormat,
  pub base_url: String,
  pub default_model_id: String,
  pub status: ApiProfileStatus,
  pub revision: u64,
  pub masked_key: Option<String>,
  pub has_credential: bool,
  pub is_active: bool,
  pub last_tested_at: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestApiProfileResult {
  pub success: bool,
  pub message: String,
  pub registry: ApiProfileRegistryView,
}

pub(super) struct ServiceTestResult {
  pub success: bool,
  pub message: String,
  pub registry: ApiProfileRegistry,
}

#[tauri::command]
pub(super) fn get_api_profile_registry(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::get_registry(&root).map(|registry| safe_registry_view(&registry))
}

#[tauri::command]
pub(super) fn save_api_profile(
  input: SaveApiProfileInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::save_profile(&root, input).map(|registry| safe_registry_view(&registry))
}

#[tauri::command]
pub(super) async fn test_api_profile(
  kind: ApiProfileKind,
  profile_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TestApiProfileResult> {
  let root = resolve_workspace_root(root_path, &state)?;
  let result =
    tauri::async_runtime::spawn_blocking(move || service::test_profile(&root, kind, &profile_id))
      .await
      .map_err(|_| command_error("API 配置测试后台任务意外终止"))??;
  Ok(TestApiProfileResult {
    success: result.success,
    message: result.message,
    registry: safe_registry_view(&result.registry),
  })
}

#[tauri::command]
pub(super) fn activate_api_profile(
  kind: ApiProfileKind,
  profile_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::activate_profile(&root, kind, &profile_id).map(|registry| safe_registry_view(&registry))
}

#[tauri::command]
pub(super) fn delete_api_profile(
  kind: ApiProfileKind,
  profile_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::delete_profile(&root, kind, &profile_id).map(|registry| safe_registry_view(&registry))
}

fn safe_registry_view(registry: &ApiProfileRegistry) -> ApiProfileRegistryView {
  let mut tikhub_profiles = registry
    .tikhub_profiles
    .values()
    .map(|profile| {
      let credential = registry.credentials.get(&profile.credential_ref_id);
      TikhubApiProfileView {
        kind: ApiProfileKind::Tikhub,
        id: profile.id.clone(),
        name: profile.name.clone(),
        base_url: profile.base_url.clone(),
        status: profile.status,
        revision: profile.revision,
        masked_key: credential.map(|value| mask_secret(&value.secret)),
        has_credential: credential.is_some(),
        is_active: registry.active_profile_ids.tikhub.as_deref() == Some(profile.id.as_str()),
        last_tested_at: profile.last_tested_at.clone(),
        test_summary: profile.test_summary.as_ref().map(safe_tikhub_summary),
        created_at: profile.created_at.clone(),
        updated_at: profile.updated_at.clone(),
      }
    })
    .collect::<Vec<_>>();
  tikhub_profiles.sort_by(|left, right| {
    profile_order(&left.created_at, &left.name, &right.created_at, &right.name)
  });

  let mut ai_profiles = registry
    .ai_profiles
    .values()
    .map(|profile| {
      let credential = profile
        .credential_ref_id
        .as_ref()
        .and_then(|id| registry.credentials.get(id));
      AiApiProfileView {
        kind: ApiProfileKind::Ai,
        id: profile.id.clone(),
        name: profile.name.clone(),
        provider_type: profile.provider_type,
        api_format: profile.api_format,
        base_url: profile.base_url.clone(),
        default_model_id: profile.default_model_id.clone(),
        status: profile.status,
        revision: profile.revision,
        masked_key: credential.map(|value| mask_secret(&value.secret)),
        has_credential: credential.is_some(),
        is_active: registry.active_profile_ids.ai.as_deref() == Some(profile.id.as_str()),
        last_tested_at: profile.last_tested_at.clone(),
        created_at: profile.created_at.clone(),
        updated_at: profile.updated_at.clone(),
      }
    })
    .collect::<Vec<_>>();
  ai_profiles.sort_by(|left, right| {
    profile_order(&left.created_at, &left.name, &right.created_at, &right.name)
  });

  ApiProfileRegistryView {
    active_profile_ids: SafeActiveProfileIds {
      tikhub: registry.active_profile_ids.tikhub.clone(),
      ai: registry.active_profile_ids.ai.clone(),
    },
    tikhub_profiles,
    ai_profiles,
  }
}

fn safe_tikhub_summary(summary: &TikhubSafeTestSummary) -> TikhubTestSummaryView {
  TikhubTestSummaryView {
    masked_account: summary.masked_account.clone(),
    balance: summary.balance,
    free_credit: summary.free_credit,
    available_credit: summary.available_credit,
    today_usage: summary.today_usage,
  }
}

fn profile_order(
  left_created: &str,
  left_name: &str,
  right_created: &str,
  right_name: &str,
) -> std::cmp::Ordering {
  left_created
    .cmp(right_created)
    .then_with(|| left_name.cmp(right_name))
}

fn command_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::SecretStore,
    false,
  )
}

#[cfg(test)]
mod tests;
