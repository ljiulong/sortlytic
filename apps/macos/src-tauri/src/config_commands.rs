use crate::domain::{AppError, AppErrorStage, AppResult};
use crate::providers::{
  self, ModelProfileInput, ModelProfileView, ModelProviderInput, ModelProviderView,
  ProviderTestResult,
};
use crate::secrets::{self, SecretConnectionTestResult, SecretRefView};
use crate::tikhub::{
  self, TikhubConnectionTestResult, TikhubConnectorInput, TikhubConnectorView, TikhubPriceQuote,
};

use super::{resolve_workspace_root, AppState};

#[tauri::command]
pub(super) fn save_secret(
  provider_type: String,
  provider_id: String,
  secret: String,
  alias: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<SecretRefView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::save_secret(root_path, &provider_type, &provider_id, &secret, alias)
}

#[tauri::command]
pub(super) fn update_secret(
  secret_ref_id: String,
  secret: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<SecretRefView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::update_secret(root_path, &secret_ref_id, &secret)
}

#[tauri::command]
pub(super) fn delete_secret(
  secret_ref_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::delete_secret(root_path, &secret_ref_id)
}

#[tauri::command]
pub(super) fn list_secret_refs(
  provider_type: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<SecretRefView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::list_secret_refs(root_path, provider_type)
}

#[tauri::command]
pub(super) fn test_secret_connection(
  secret_ref_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<SecretConnectionTestResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::test_secret_connection(root_path, &secret_ref_id)
}

#[tauri::command]
pub(super) async fn test_tikhub_connection(
  secret_ref_id: String,
  base_url: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TikhubConnectionTestResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  run_tikhub_blocking(move || tikhub::test_tikhub_connection(root_path, &secret_ref_id, base_url))
    .await
}

#[tauri::command]
pub(super) fn get_tikhub_connector(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Option<TikhubConnectorView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tikhub::get_tikhub_connector(root_path)
}

#[tauri::command]
pub(super) fn save_tikhub_connector(
  input: TikhubConnectorInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TikhubConnectorView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tikhub::save_tikhub_connector(root_path, input)
}

#[tauri::command]
pub(super) async fn test_tikhub_connector(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TikhubConnectionTestResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  run_tikhub_blocking(move || tikhub::test_tikhub_connector(root_path)).await
}

#[tauri::command]
pub(super) async fn quote_tikhub_connector_price(
  endpoint: String,
  request_per_day: i64,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TikhubPriceQuote> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  run_tikhub_blocking(move || {
    tikhub::quote_tikhub_connector_price(root_path, &endpoint, request_per_day)
  })
  .await
}

async fn run_tikhub_blocking<T, F>(task: F) -> AppResult<T>
where
  T: Send + 'static,
  F: FnOnce() -> AppResult<T> + Send + 'static,
{
  tauri::async_runtime::spawn_blocking(task)
    .await
    .map_err(|_| {
      AppError::new(
        crate::domain::AppErrorCode::TikhubRequestError,
        "TikHub 后台任务意外终止",
        AppErrorStage::Collection,
        true,
      )
    })?
}

#[tauri::command]
pub(super) fn list_model_providers(
  enabled: Option<bool>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ModelProviderView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::list_model_providers(root_path, enabled)
}

#[tauri::command]
pub(super) fn create_model_provider(
  input: ModelProviderInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ModelProviderView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::create_model_provider(root_path, input)
}

#[tauri::command]
pub(super) fn update_model_provider(
  provider_id: String,
  input: ModelProviderInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ModelProviderView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::update_model_provider(root_path, &provider_id, input)
}

#[tauri::command]
pub(super) fn delete_model_provider(
  provider_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::delete_model_provider(root_path, &provider_id)
}

#[tauri::command]
pub(super) fn list_model_profiles(
  provider_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ModelProfileView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::list_model_profiles(root_path, &provider_id)
}

#[tauri::command]
pub(super) fn upsert_model_profile(
  input: ModelProfileInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ModelProfileView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::upsert_model_profile(root_path, input)
}

#[tauri::command]
pub(super) fn test_model_provider(
  provider_id: String,
  model_id: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ProviderTestResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::test_model_provider(root_path, &provider_id, model_id)
}

#[tauri::command]
pub(super) fn set_default_model(
  provider_id: String,
  model_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::set_default_model(root_path, &provider_id, &model_id)
}

#[tauri::command]
pub(super) fn set_active_model_provider(
  provider_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::set_active_model_provider(root_path, &provider_id)
}
