pub mod accounts;
pub mod ai;
mod api_profile_commands;
pub mod api_profiles;
mod app_runtime;
pub mod app_state;
pub mod collection;
mod config_commands;
pub mod domain;
pub mod exports;
#[cfg(target_os = "macos")]
mod native_window;
pub mod prompts;
pub mod providers;
pub mod records;
pub mod secrets;
mod task_commands;
pub mod tasks;
pub mod tikhub;
pub mod workspace;

use std::{fs, path::PathBuf};

use ai::{
  AiRunView, GenerateCollectionPlanFromTextInput, GeneratedCollectionPlanView,
  NaturalParseAttemptView,
};
use api_profile_commands::{
  activate_api_profile, delete_api_profile, get_api_profile_registry, save_api_profile,
  test_api_profile,
};
use app_runtime::workspace_context_from_summary;
use app_state::{AppState, BackendStatus, WorkspaceContext};
use collection::{
  AccountCollectionCapabilityView, AccountFormCollectionPlanRequest,
  CollectionParamValidationResult, CollectionPlanDraftView, DataTypeCapabilityView,
  FormCollectionPlanRequest, PlatformCapabilityView,
};
use config_commands::quote_tikhub_connector_price;
use domain::{AppError, AppErrorStage, AppResult};
use exports::{ExportIntegrityResult, ExportJobView, ReportView};
use prompts::{
  CreatePromptVersionInput, PromptRegressionCaseView, PromptRegressionRunView, PromptTemplateView,
  PromptVersionView,
};
use task_commands::{
  cancel_task, confirm_collection_plan, copy_task, create_collection_task, delete_task,
  enqueue_task, estimate_task_cost, execute_next_task, get_latest_collection_plan, get_task,
  list_latest_task_runs, list_task_logs, list_tasks, retry_task, revise_collection_task,
  save_collection_plan, update_collection_task,
};
use tasks::CostEstimateView;
use tauri::Manager;
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
  let active_workspace = state.active_workspace();
  let is_initial_activation = active_workspace.is_none();
  let summary = if let Some(active) = active_workspace {
    workspace::open_workspace(active.root_path)?
  } else {
    workspace::ensure_workspace("本地研究工作区", root_path)?
  };
  if is_initial_activation {
    ai::mark_interrupted_task_intents(&summary.root_path)?;
  }
  state.set_active_workspace(workspace_context_from_summary(&summary));
  Ok(summary)
}

#[tauri::command]
fn open_workspace(
  root_path: String,
  state: tauri::State<'_, AppState>,
) -> AppResult<WorkspaceSummary> {
  let summary = workspace::open_workspace(root_path)?;
  ai::mark_interrupted_task_intents(&summary.root_path)?;
  state.set_active_workspace(workspace_context_from_summary(&summary));
  Ok(summary)
}

#[tauri::command]
fn get_active_workspace(state: tauri::State<'_, AppState>) -> AppResult<Option<WorkspaceContext>> {
  Ok(state.active_workspace())
}

#[tauri::command]
fn list_latest_task_intents(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<NaturalParseAttemptView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  ai::list_latest_task_intents(root_path)
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
fn list_task_record_counts(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<records::TaskRecordCountView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  records::list_task_record_counts(root_path)
}

#[tauri::command]
fn list_task_results(
  task_id: String,
  limit: Option<i64>,
  offset: Option<i64>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<records::TaskResultsPageView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  records::list_task_results(
    root_path,
    &task_id,
    limit.unwrap_or(100),
    offset.unwrap_or(0),
  )
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
fn get_account_collection_capabilities(
  platform: String,
) -> AppResult<AccountCollectionCapabilityView> {
  collection::get_account_collection_capabilities(&platform)
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
fn generate_account_collection_plan(
  request: AccountFormCollectionPlanRequest,
) -> AppResult<CollectionPlanDraftView> {
  collection::generate_account_collection_plan(request)
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
async fn activate_prompt_version(
  prompt_version_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<PromptVersionView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tauri::async_runtime::spawn_blocking(move || {
    prompts::activate_prompt_version(root_path, &prompt_version_id)
  })
  .await
  .map_err(|error| {
    AppError::new(
      domain::AppErrorCode::ModelProtocolError,
      format!("提示词回归后台任务异常结束：{error}"),
      AppErrorStage::Ai,
      true,
    )
  })?
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
async fn generate_collection_plan_from_text(
  input: GenerateCollectionPlanFromTextInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<GeneratedCollectionPlanView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tauri::async_runtime::spawn_blocking(move || {
    ai::generate_collection_plan_from_text(root_path, input)
  })
  .await
  .map_err(|error| {
    AppError::new(
      domain::AppErrorCode::ModelProtocolError,
      format!("AI 后台任务异常结束：{error}"),
      AppErrorStage::Ai,
      true,
    )
  })?
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
      list_latest_task_intents,
      run_workspace_health_check,
      close_workspace,
      get_api_profile_registry,
      save_api_profile,
      test_api_profile,
      activate_api_profile,
      delete_api_profile,
      quote_tikhub_connector_price,
      create_collection_task,
      update_collection_task,
      save_collection_plan,
      revise_collection_task,
      estimate_task_cost,
      confirm_collection_plan,
      get_latest_collection_plan,
      enqueue_task,
      execute_next_task,
      cancel_task,
      delete_task,
      retry_task,
      copy_task,
      get_task,
      list_tasks,
      list_latest_task_runs,
      list_task_logs,
      list_task_record_counts,
      list_task_results,
      list_supported_platforms,
      list_platform_data_types,
      get_account_collection_capabilities,
      validate_collection_params,
      generate_form_collection_plan,
      generate_account_collection_plan,
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
        app.handle().plugin(tauri_plugin_opener::init())?;
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
        if let Some(main_window) = app.get_webview_window("main") {
          if let Err(error) = native_window::apply_native_window_corner_radius(&main_window) {
            log::error!("macOS 原生圆角初始化失败：{error}");
          }
        } else {
          log::error!("macOS 原生圆角初始化失败：找不到主窗口");
        }
      }
      app_runtime::start_task_worker(app.handle());
      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod command_tests;
