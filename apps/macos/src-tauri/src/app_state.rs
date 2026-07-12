use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceContext {
  pub id: String,
  pub name: String,
  pub root_path: PathBuf,
  pub schema_version: i64,
}

#[derive(Debug)]
pub struct AppState {
  pub started_at: SystemTime,
  active_workspace: Mutex<Option<WorkspaceContext>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BackendStatus {
  pub service: String,
  pub backend_version: String,
  pub has_active_workspace: bool,
  pub uptime_ms: u64,
}

impl AppState {
  pub fn new() -> Self {
    Self {
      started_at: SystemTime::now(),
      active_workspace: Mutex::new(None),
    }
  }

  pub fn backend_status(&self) -> BackendStatus {
    let uptime_ms = self
      .started_at
      .elapsed()
      .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
      .unwrap_or(0);

    BackendStatus {
      service: "local-tauri-backend".to_string(),
      backend_version: env!("CARGO_PKG_VERSION").to_string(),
      has_active_workspace: self.active_workspace().is_some(),
      uptime_ms,
    }
  }

  pub fn active_workspace(&self) -> Option<WorkspaceContext> {
    self
      .active_workspace
      .lock()
      .expect("active workspace state poisoned")
      .clone()
  }

  pub fn set_active_workspace(&self, workspace: WorkspaceContext) {
    *self
      .active_workspace
      .lock()
      .expect("active workspace state poisoned") = Some(workspace);
  }

  pub fn clear_active_workspace(&self) {
    *self
      .active_workspace
      .lock()
      .expect("active workspace state poisoned") = None;
  }
}

impl Default for AppState {
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn backend_status_reports_no_workspace_by_default() {
    let state = AppState::new();
    let status = state.backend_status();

    assert_eq!(status.service, "local-tauri-backend");
    assert_eq!(status.backend_version, env!("CARGO_PKG_VERSION"));
    assert!(!status.has_active_workspace);
    assert!(status.uptime_ms < 1_000);
  }

  #[test]
  fn active_workspace_can_be_set_and_cleared() {
    let state = AppState::new();
    let workspace = WorkspaceContext {
      id: "workspace-id".to_string(),
      name: "测试工作区".to_string(),
      root_path: PathBuf::from("/tmp/workspace"),
      schema_version: 1,
    };

    state.set_active_workspace(workspace.clone());
    assert_eq!(state.active_workspace(), Some(workspace));
    assert!(state.backend_status().has_active_workspace);

    state.clear_active_workspace();
    assert!(state.active_workspace().is_none());
    assert!(!state.backend_status().has_active_workspace);
  }
}
