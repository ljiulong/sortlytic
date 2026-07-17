use crate::domain::{AppError, AppErrorStage, AppResult};
use crate::providers::{self, ModelProfileView, ModelProviderView};
use crate::secrets::{self, SecretRefView};
use crate::tikhub::{self, TikhubConnectorView, TikhubPriceQuote};

use super::{resolve_workspace_root, AppState};

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
pub(super) fn get_tikhub_connector(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Option<TikhubConnectorView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tikhub::get_tikhub_connector(root_path)
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
pub(super) fn list_model_profiles(
  provider_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ModelProfileView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::list_model_profiles(root_path, &provider_id)
}
