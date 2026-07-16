use std::{thread, time::Duration};

use tauri::Manager;

use crate::{
  app_state::{AppState, WorkspaceContext},
  tasks,
  workspace::WorkspaceSummary,
};

pub(super) fn workspace_context_from_summary(summary: &WorkspaceSummary) -> WorkspaceContext {
  WorkspaceContext {
    id: summary.id.clone(),
    name: summary.name.clone(),
    root_path: summary.root_path.clone(),
    schema_version: summary.schema_version,
  }
}

pub(super) fn start_task_worker(app: &tauri::AppHandle) {
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
