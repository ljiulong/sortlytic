use chrono::Utc;
use rusqlite::{params, Connection};

use super::database_error;
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkerFence {
  owner_id: String,
  generation: i64,
}

impl WorkerFence {
  pub(crate) fn new(owner_id: String, generation: i64) -> AppResult<Self> {
    if owner_id.trim().is_empty() || generation <= 0 {
      return Err(lease_error("本地任务执行器栅栏身份或代次无效"));
    }
    Ok(Self {
      owner_id,
      generation,
    })
  }

  pub(crate) fn owner_id(&self) -> &str {
    &self.owner_id
  }

  pub(crate) fn generation(&self) -> i64 {
    self.generation
  }

  pub(crate) fn ensure_current(&self, connection: &Connection) -> AppResult<()> {
    let now = Utc::now().timestamp_millis();
    let current: bool = connection
      .query_row(
        "SELECT EXISTS (
           SELECT 1 FROM task_worker_lease
           WHERE id = 'task_worker' AND owner_id = ?1
             AND generation = ?2 AND lease_expires_at > ?3
         )",
        params![self.owner_id, self.generation, now],
        |row| row.get(0),
      )
      .map_err(database_error)?;
    if current {
      Ok(())
    } else {
      Err(lease_error(
        "本地任务执行器栅栏已失效，已拒绝旧代执行器提交业务副作用",
      ))
    }
  }
}

fn lease_error(message: &str) -> AppError {
  AppError::new(
    AppErrorCode::WorkspaceError,
    message,
    AppErrorStage::Workspace,
    true,
  )
  .with_safe_detail("operation", "task_worker_fence")
}
