use crate::domain::{AppError, AppErrorStage, AppResult};
use crate::tikhub::{self, TikhubPriceQuote};

use super::{resolve_workspace_root, AppState};

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
