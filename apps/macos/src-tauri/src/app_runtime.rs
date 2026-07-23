use std::{path::Path, thread, time::Duration};

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
        if let Err(error) = recover_interrupted_runs_if_needed(&workspace.root_path) {
          log::error!("恢复本地任务运行失败：{error:?}");
        }
        if let Err(error) = execute_next_task_if_needed(&workspace.root_path) {
          log::error!("执行本地任务失败：{error:?}");
        }
      }
      thread::sleep(Duration::from_secs(2));
    })
    .expect("本地任务执行器线程无法启动");
}

fn recover_interrupted_runs_if_needed(root_path: &Path) -> crate::domain::AppResult<i64> {
  if !tasks::task_worker_work_state(root_path)?.has_running_run {
    return Ok(0);
  }
  tasks::recover_interrupted_runs(root_path)
}

fn execute_next_task_if_needed(
  root_path: &Path,
) -> crate::domain::AppResult<Option<tasks::TaskRunView>> {
  if !tasks::task_worker_work_state(root_path)?.has_queued_run {
    return Ok(None);
  }
  tasks::execute_next_task(root_path)
}

#[cfg(test)]
mod tests {
  use std::fs;

  use uuid::Uuid;

  use super::{execute_next_task_if_needed, recover_interrupted_runs_if_needed};
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

  #[test]
  fn idle_worker_cycle_never_creates_or_renews_a_database_lease() {
    let root = std::env::temp_dir().join(format!("idle-worker-cycle-{}", Uuid::new_v4()));
    create_workspace("空闲执行器测试", &root).expect("workspace should create");

    assert_eq!(recover_interrupted_runs_if_needed(&root).unwrap(), 0);
    assert!(execute_next_task_if_needed(&root).unwrap().is_none());

    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let lease_count: i64 = connection
      .query_row("SELECT COUNT(*) FROM task_worker_lease", [], |row| {
        row.get(0)
      })
      .unwrap();
    assert_eq!(
      lease_count, 0,
      "stable idle polling must perform zero lease writes"
    );
    fs::remove_dir_all(root).ok();
  }
}
