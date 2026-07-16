pub mod accounts;
pub mod ai;
pub mod app_state;
pub mod collection;
pub mod domain;
pub mod exports;
mod planning;
pub mod prompts;
pub mod providers;
pub mod records;
pub mod secrets;
pub mod tasks;
pub mod tikhub;
pub mod workspace;

use std::{fs, path::PathBuf, thread, time::Duration};

#[cfg(target_os = "macos")]
use std::ffi::{c_char, c_void};

use ai::{AiRunView, GenerateCollectionPlanFromTextInput, GeneratedCollectionPlanView};
use app_state::{AppState, BackendStatus, WorkspaceContext};
use collection::{
  CollectionParamValidationResult, CollectionPlanDraftView, DataTypeCapabilityView,
  FormCollectionPlanRequest, PlatformCapabilityView,
};
use domain::{AppError, AppErrorStage, AppResult};
use exports::{ExportIntegrityResult, ExportJobView, ReportView};
use prompts::{
  CreatePromptVersionInput, PromptRegressionCaseView, PromptRegressionRunView, PromptTemplateView,
  PromptVersionView,
};
use providers::{
  ModelProfileInput, ModelProfileView, ModelProviderInput, ModelProviderView, ProviderTestResult,
};
use secrets::{SecretConnectionTestResult, SecretRefView};
use tasks::{
  CollectionPlanView, CollectionTaskView, CostEstimateView, CreateCollectionTaskInput,
  SaveCollectionPlanInput, TaskLogView, TaskRunView, UpdateCollectionTaskInput,
};
use tauri::Manager;
use tikhub::{
  TikhubConnectionTestResult, TikhubConnectorInput, TikhubConnectorView, TikhubPriceQuote,
};
use workspace::{WorkspaceHealthCheck, WorkspaceSummary};

#[tauri::command]
fn get_backend_status(state: tauri::State<'_, AppState>) -> AppResult<BackendStatus> {
  Ok(state.backend_status())
}

#[tauri::command]
fn create_workspace(
  name: String,
  root_path: String,
  state: tauri::State<'_, AppState>,
) -> AppResult<WorkspaceSummary> {
  let summary = workspace::create_workspace(&name, root_path)?;
  state.set_active_workspace(workspace_context_from_summary(&summary));
  Ok(summary)
}

#[tauri::command]
fn ensure_default_workspace(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
) -> AppResult<WorkspaceSummary> {
  let app_data_dir = app.path().app_data_dir().map_err(|error| {
    AppError::new(
      domain::AppErrorCode::WorkspaceError,
      format!("无法解析默认工作区目录：{error}"),
      AppErrorStage::Workspace,
      false,
    )
  })?;
  let root_path = app_data_dir.join("default-workspace");
  let summary = ensure_default_workspace_for_state(root_path, &state)?;
  prompts::seed_builtin_prompts(&summary.root_path)?;
  Ok(summary)
}

fn ensure_default_workspace_for_state(
  root_path: PathBuf,
  state: &AppState,
) -> AppResult<WorkspaceSummary> {
  let summary = if let Some(active) = state.active_workspace() {
    workspace::open_workspace(active.root_path)?
  } else {
    workspace::ensure_workspace("本地研究工作区", root_path)?
  };
  state.set_active_workspace(workspace_context_from_summary(&summary));
  Ok(summary)
}

#[tauri::command]
fn open_workspace(
  root_path: String,
  state: tauri::State<'_, AppState>,
) -> AppResult<WorkspaceSummary> {
  let summary = workspace::open_workspace(root_path)?;
  state.set_active_workspace(workspace_context_from_summary(&summary));
  Ok(summary)
}

#[tauri::command]
fn get_active_workspace(state: tauri::State<'_, AppState>) -> AppResult<Option<WorkspaceContext>> {
  Ok(state.active_workspace())
}

#[tauri::command]
fn run_workspace_health_check(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<WorkspaceHealthCheck> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  workspace::run_workspace_health_check(root_path)
}

#[tauri::command]
fn close_workspace(workspace_id: String, state: tauri::State<'_, AppState>) -> AppResult<bool> {
  let active_workspace = state
    .active_workspace()
    .ok_or_else(|| AppError::validation("当前没有打开的工作区", AppErrorStage::Workspace))?;

  if active_workspace.id != workspace_id {
    return Err(AppError::validation(
      "要关闭的工作区不是当前活动工作区",
      AppErrorStage::Workspace,
    ));
  }

  state.clear_active_workspace();
  Ok(true)
}

#[tauri::command]
fn save_secret(
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
fn update_secret(
  secret_ref_id: String,
  secret: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<SecretRefView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::update_secret(root_path, &secret_ref_id, &secret)
}

#[tauri::command]
fn delete_secret(
  secret_ref_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::delete_secret(root_path, &secret_ref_id)
}

#[tauri::command]
fn list_secret_refs(
  provider_type: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<SecretRefView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::list_secret_refs(root_path, provider_type)
}

#[tauri::command]
fn test_secret_connection(
  secret_ref_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<SecretConnectionTestResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  secrets::test_secret_connection(root_path, &secret_ref_id)
}

#[tauri::command]
async fn test_tikhub_connection(
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
fn get_tikhub_connector(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Option<TikhubConnectorView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tikhub::get_tikhub_connector(root_path)
}

#[tauri::command]
fn save_tikhub_connector(
  input: TikhubConnectorInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TikhubConnectorView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tikhub::save_tikhub_connector(root_path, input)
}

#[tauri::command]
async fn test_tikhub_connector(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TikhubConnectionTestResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  run_tikhub_blocking(move || tikhub::test_tikhub_connector(root_path)).await
}

#[tauri::command]
async fn quote_tikhub_connector_price(
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
        domain::AppErrorCode::TikhubRequestError,
        "TikHub 后台任务意外终止",
        AppErrorStage::Collection,
        true,
      )
    })?
}

#[tauri::command]
fn list_model_providers(
  enabled: Option<bool>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ModelProviderView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::list_model_providers(root_path, enabled)
}

#[tauri::command]
fn create_model_provider(
  input: ModelProviderInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ModelProviderView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::create_model_provider(root_path, input)
}

#[tauri::command]
fn update_model_provider(
  provider_id: String,
  input: ModelProviderInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ModelProviderView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::update_model_provider(root_path, &provider_id, input)
}

#[tauri::command]
fn delete_model_provider(
  provider_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::delete_model_provider(root_path, &provider_id)
}

#[tauri::command]
fn list_model_profiles(
  provider_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ModelProfileView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::list_model_profiles(root_path, &provider_id)
}

#[tauri::command]
fn upsert_model_profile(
  input: ModelProfileInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ModelProfileView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::upsert_model_profile(root_path, input)
}

#[tauri::command]
fn test_model_provider(
  provider_id: String,
  model_id: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ProviderTestResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::test_model_provider(root_path, &provider_id, model_id)
}

#[tauri::command]
fn set_default_model(
  provider_id: String,
  model_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::set_default_model(root_path, &provider_id, &model_id)
}

#[tauri::command]
fn set_active_model_provider(
  provider_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<bool> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  providers::set_active_model_provider(root_path, &provider_id)
}

#[tauri::command]
fn create_collection_task(
  input: CreateCollectionTaskInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::create_collection_task(root_path, input)
}

#[tauri::command]
fn update_collection_task(
  task_id: String,
  input: UpdateCollectionTaskInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::update_collection_task(root_path, &task_id, input)
}

#[tauri::command]
fn save_collection_plan(
  input: SaveCollectionPlanInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionPlanView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::save_collection_plan(root_path, input)
}

#[tauri::command]
fn estimate_task_cost(
  task_id: Option<String>,
  plan_json: Option<serde_json::Value>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CostEstimateView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::estimate_task_cost(root_path, task_id, plan_json)
}

#[tauri::command]
fn confirm_collection_plan(
  task_id: String,
  plan_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::confirm_collection_plan(root_path, &task_id, &plan_id)
}

#[tauri::command]
fn enqueue_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TaskRunView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::enqueue_task(root_path, &task_id)
}

#[tauri::command]
fn execute_next_task(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Option<TaskRunView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::execute_next_task(root_path)
}

#[tauri::command]
fn cancel_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::cancel_task(root_path, &task_id)
}

#[tauri::command]
fn retry_task(
  task_id: String,
  stage: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TaskRunView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::retry_task(root_path, &task_id, stage)
}

#[tauri::command]
fn copy_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::copy_task(root_path, &task_id)
}

#[tauri::command]
fn get_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::get_task(root_path, &task_id)
}

#[tauri::command]
fn list_tasks(
  status: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<CollectionTaskView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::list_tasks(root_path, status)
}

#[tauri::command]
fn list_task_logs(
  task_run_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<TaskLogView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::list_task_logs(root_path, &task_run_id)
}

#[tauri::command]
fn list_supported_platforms() -> AppResult<Vec<PlatformCapabilityView>> {
  Ok(collection::list_supported_platforms())
}

#[tauri::command]
fn list_platform_data_types(platform: String) -> AppResult<Vec<DataTypeCapabilityView>> {
  collection::list_platform_data_types(&platform)
}

#[tauri::command]
fn validate_collection_params(
  platform: String,
  data_type: String,
  params: serde_json::Value,
) -> AppResult<CollectionParamValidationResult> {
  collection::validate_collection_params(&platform, &data_type, params)
}

#[tauri::command]
fn generate_form_collection_plan(
  request: FormCollectionPlanRequest,
) -> AppResult<CollectionPlanDraftView> {
  collection::generate_form_collection_plan(request)
}

#[tauri::command]
fn preview_collection_plan(plan_json: serde_json::Value) -> AppResult<CostEstimateView> {
  collection::preview_collection_plan(plan_json)
}

#[tauri::command]
fn seed_builtin_prompts(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<PromptTemplateView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  prompts::seed_builtin_prompts(root_path)
}

#[tauri::command]
fn list_prompt_templates(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<PromptTemplateView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  prompts::list_prompt_templates(root_path)
}

#[tauri::command]
fn list_prompt_versions(
  template_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<PromptVersionView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  prompts::list_prompt_versions(root_path, &template_id)
}

#[tauri::command]
fn create_prompt_version(
  input: CreatePromptVersionInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<PromptVersionView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  prompts::create_prompt_version(root_path, input)
}

#[tauri::command]
fn activate_prompt_version(
  prompt_version_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<PromptVersionView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  prompts::activate_prompt_version(root_path, &prompt_version_id)
}

#[tauri::command]
fn list_prompt_regression_cases(
  template_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<PromptRegressionCaseView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  prompts::list_prompt_regression_cases(root_path, &template_id)
}

#[tauri::command]
fn list_prompt_regression_runs(
  prompt_version_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<PromptRegressionRunView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  prompts::list_prompt_regression_runs(root_path, &prompt_version_id)
}

#[tauri::command]
fn generate_collection_plan_from_text(
  input: GenerateCollectionPlanFromTextInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<GeneratedCollectionPlanView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  ai::generate_collection_plan_from_text(root_path, input)
}

#[tauri::command]
fn get_ai_run(
  ai_run_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<AiRunView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  ai::get_ai_run(root_path, &ai_run_id)
}

#[tauri::command]
fn list_ai_runs(
  task_id: String,
  run_type: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<AiRunView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  ai::list_ai_runs(root_path, task_id, run_type)
}

#[tauri::command]
fn build_report_model(
  task_id: String,
  report_type: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ReportView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  exports::build_report_model(root_path, &task_id, &report_type)
}

#[tauri::command]
fn validate_export_integrity(
  report_id: String,
  export_type: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ExportIntegrityResult> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  exports::validate_export_integrity(root_path, &report_id, &export_type)
}

#[tauri::command]
fn create_export_job(
  report_id: String,
  export_type: String,
  target_path: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ExportJobView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  exports::create_export_job(root_path, &report_id, &export_type, target_path)
}

#[tauri::command]
fn get_export_job(
  export_job_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ExportJobView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  exports::get_export_job(root_path, &export_job_id)
}

#[tauri::command]
fn list_export_jobs(
  report_id: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<ExportJobView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  exports::list_export_jobs(root_path, report_id)
}

fn resolve_workspace_root(root_path: Option<String>, state: &AppState) -> AppResult<PathBuf> {
  let active = state
    .active_workspace()
    .ok_or_else(|| AppError::validation("当前没有打开的工作区", AppErrorStage::Workspace))?;
  if let Some(requested_root) = root_path {
    let requested_root = canonical_command_root(PathBuf::from(requested_root))?;
    let active_root = canonical_command_root(&active.root_path)?;
    if requested_root != active_root {
      return Err(AppError::new(
        domain::AppErrorCode::PermissionError,
        "命令指定的工作区与当前活动工作区不一致",
        AppErrorStage::Workspace,
        false,
      ));
    }
  }
  Ok(active.root_path)
}

fn canonical_command_root(root_path: impl AsRef<std::path::Path>) -> AppResult<PathBuf> {
  fs::canonicalize(root_path).map_err(|_| {
    AppError::new(
      domain::AppErrorCode::WorkspaceError,
      "工作区根目录不存在或无法访问",
      AppErrorStage::Workspace,
      false,
    )
  })
}

fn workspace_context_from_summary(summary: &WorkspaceSummary) -> WorkspaceContext {
  WorkspaceContext {
    id: summary.id.clone(),
    name: summary.name.clone(),
    root_path: summary.root_path.clone(),
    schema_version: summary.schema_version,
  }
}

fn start_task_worker(app: &tauri::AppHandle) {
  let app = app.clone();
  thread::Builder::new()
    .name("local-task-worker".to_string())
    .spawn(move || loop {
      if let Some(workspace) = app.state::<AppState>().active_workspace() {
        if let Err(error) = tasks::recover_interrupted_runs(&workspace.root_path) {
          log::error!("恢复本地任务运行失败：{:?}", error);
        }
        if let Err(error) = tasks::execute_next_task(&workspace.root_path) {
          log::error!("执行本地任务失败：{:?}", error);
        }
      }
      thread::sleep(Duration::from_secs(2));
    })
    .expect("本地任务执行器线程无法启动");
}

#[cfg(target_os = "macos")]
#[link(name = "objc")]
unsafe extern "C" {
  fn objc_getClass(name: *const c_char) -> *mut c_void;
  fn objc_msgSend();
  fn sel_registerName(name: *const c_char) -> *const c_void;
}

#[cfg(target_os = "macos")]
unsafe fn objc_send_id(receiver: *mut c_void, selector: &'static [u8]) -> *mut c_void {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void) -> *mut c_void =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector)
}

#[cfg(target_os = "macos")]
unsafe fn objc_send_bool(receiver: *mut c_void, selector: &'static [u8], value: bool) {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void, bool) =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector, value);
}

#[cfg(target_os = "macos")]
unsafe fn objc_send_id_arg(receiver: *mut c_void, selector: &'static [u8], value: *mut c_void) {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void) =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector, value);
}

#[cfg(target_os = "macos")]
unsafe fn objc_send_f64(receiver: *mut c_void, selector: &'static [u8], value: f64) {
  let selector = sel_registerName(selector.as_ptr().cast());
  let send: unsafe extern "C" fn(*mut c_void, *const c_void, f64) =
    std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
  send(receiver, selector, value);
}

#[cfg(target_os = "macos")]
fn apply_native_window_corner_radius(window: &tauri::WebviewWindow) -> Result<(), String> {
  let native_window = window.ns_window().map_err(|error| error.to_string())?;
  let (clear_color, content_view) = unsafe {
    let ns_color = objc_getClass(b"NSColor\0".as_ptr().cast());
    if ns_color.is_null() {
      return Err("无法获取 macOS 原生颜色类".to_string());
    }
    (
      objc_send_id(ns_color, b"clearColor\0"),
      objc_send_id(native_window, b"contentView\0"),
    )
  };
  if clear_color.is_null() {
    return Err("无法获取 macOS 透明背景色".to_string());
  }
  if content_view.is_null() {
    return Err("无法获取 macOS 窗口内容视图".to_string());
  }

  unsafe {
    objc_send_bool(native_window, b"setOpaque:\0", false);
    objc_send_id_arg(native_window, b"setBackgroundColor:\0", clear_color);
    objc_send_bool(content_view, b"setWantsLayer:\0", true);
    let layer = objc_send_id(content_view, b"layer\0");
    if layer.is_null() {
      return Err("无法获取 macOS 窗口内容图层".to_string());
    }
    objc_send_f64(layer, b"setCornerRadius:\0", 16.0);
    objc_send_bool(layer, b"setMasksToBounds:\0", true);
  }
  Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .manage(AppState::new())
    .plugin(tauri_plugin_fs::init())
    .invoke_handler(tauri::generate_handler![
      get_backend_status,
      create_workspace,
      ensure_default_workspace,
      open_workspace,
      get_active_workspace,
      run_workspace_health_check,
      close_workspace,
      save_secret,
      update_secret,
      delete_secret,
      list_secret_refs,
      test_secret_connection,
      test_tikhub_connection,
      get_tikhub_connector,
      save_tikhub_connector,
      test_tikhub_connector,
      quote_tikhub_connector_price,
      list_model_providers,
      create_model_provider,
      update_model_provider,
      delete_model_provider,
      list_model_profiles,
      upsert_model_profile,
      test_model_provider,
      set_default_model,
      set_active_model_provider,
      create_collection_task,
      update_collection_task,
      save_collection_plan,
      estimate_task_cost,
      confirm_collection_plan,
      enqueue_task,
      execute_next_task,
      cancel_task,
      retry_task,
      copy_task,
      get_task,
      list_tasks,
      list_task_logs,
      list_supported_platforms,
      list_platform_data_types,
      validate_collection_params,
      generate_form_collection_plan,
      preview_collection_plan,
      seed_builtin_prompts,
      list_prompt_templates,
      list_prompt_versions,
      create_prompt_version,
      activate_prompt_version,
      list_prompt_regression_cases,
      list_prompt_regression_runs,
      generate_collection_plan_from_text,
      get_ai_run,
      list_ai_runs,
      build_report_model,
      validate_export_integrity,
      create_export_job,
      get_export_job,
      list_export_jobs
    ])
    .setup(|app| {
      #[cfg(desktop)]
      {
        app.handle().plugin(tauri_plugin_process::init())?;
        app
          .handle()
          .plugin(tauri_plugin_updater::Builder::new().build())?;
      }
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      #[cfg(target_os = "macos")]
      {
        let main_window = app
          .get_webview_window("main")
          .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "找不到主窗口"))?;
        apply_native_window_corner_radius(&main_window)
          .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
      }
      start_task_worker(app.handle());
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod command_tests;
