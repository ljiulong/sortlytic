use crate::domain::AppResult;
use crate::tasks::{
  self, CollectionPlanView, CollectionTaskView, CostEstimateView, CreateCollectionTaskInput,
  SaveCollectionPlanInput, TaskLogView, TaskRunView, UpdateCollectionTaskInput,
};

use super::{resolve_workspace_root, AppState};

#[tauri::command]
pub(super) fn create_collection_task(
  input: CreateCollectionTaskInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::create_collection_task(root_path, input)
}

#[tauri::command]
pub(super) fn update_collection_task(
  task_id: String,
  input: UpdateCollectionTaskInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::update_collection_task(root_path, &task_id, input)
}

#[tauri::command]
pub(super) fn save_collection_plan(
  input: SaveCollectionPlanInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionPlanView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::save_collection_plan(root_path, input)
}

#[tauri::command]
pub(super) fn estimate_task_cost(
  task_id: Option<String>,
  plan_json: Option<serde_json::Value>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CostEstimateView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::estimate_task_cost(root_path, task_id, plan_json)
}

#[tauri::command]
pub(super) fn confirm_collection_plan(
  task_id: String,
  plan_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::confirm_collection_plan(root_path, &task_id, &plan_id)
}

#[tauri::command]
pub(super) fn get_latest_collection_plan(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionPlanView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::get_latest_collection_plan(root_path, &task_id)
}

#[tauri::command]
pub(super) fn enqueue_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TaskRunView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::enqueue_task(root_path, &task_id)
}

#[tauri::command]
pub(super) fn execute_next_task(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Option<TaskRunView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::execute_next_task(root_path)
}

#[tauri::command]
pub(super) fn cancel_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::cancel_task(root_path, &task_id)
}

#[tauri::command]
pub(super) fn delete_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<()> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::delete_task(root_path, &task_id)
}

#[tauri::command]
pub(super) fn retry_task(
  task_id: String,
  stage: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TaskRunView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::retry_task(root_path, &task_id, stage)
}

#[tauri::command]
pub(super) fn copy_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::copy_task(root_path, &task_id)
}

#[tauri::command]
pub(super) fn get_task(
  task_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<CollectionTaskView> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::get_task(root_path, &task_id)
}

#[tauri::command]
pub(super) fn list_tasks(
  status: Option<String>,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<CollectionTaskView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::list_tasks(root_path, status)
}

#[tauri::command]
pub(super) fn list_latest_task_runs(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<TaskRunView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::list_latest_task_runs(root_path)
}

#[tauri::command]
pub(super) fn list_task_logs(
  task_run_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<Vec<TaskLogView>> {
  let root_path = resolve_workspace_root(root_path, &state)?;
  tasks::list_task_logs(root_path, &task_run_id)
}
