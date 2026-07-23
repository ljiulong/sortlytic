use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::Utc;
use fs4::{FileExt, TryLockError};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use uuid::Uuid;

use super::{database_error, TaskRunView, WorkerFence};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PERMISSION_BITS: u32 = 0o7777;
const WORKER_LOCK_FILE: &str = "task-worker.lock";
const WORKER_LEASE_MILLIS: i64 = 120_000;

struct TaskWorkerOwner {
  _file: File,
  root_path: PathBuf,
  fence: WorkerFence,
  released: bool,
}

impl TaskWorkerOwner {
  fn try_acquire(root_path: &Path) -> AppResult<Option<Self>> {
    let lock_path = worker_lock_path(root_path)?;
    let file = open_private_lock_file(&lock_path)?;
    match FileExt::try_lock(&file) {
      Ok(()) => {
        let owner_id = Uuid::new_v4().to_string();
        let Some(fence) = claim_database_lease(root_path, &owner_id)? else {
          return Ok(None);
        };
        Ok(Some(Self {
          _file: file,
          root_path: root_path.to_path_buf(),
          fence,
          released: false,
        }))
      }
      Err(TryLockError::WouldBlock) => Ok(None),
      Err(TryLockError::Error(error)) => Err(lock_io_error("无法取得本地任务执行器所有权", error)),
    }
  }

  fn ensure_current(&self) -> AppResult<()> {
    let now = Utc::now().timestamp_millis();
    let expires_at = lease_expiry(now)?;
    let connection = open_workspace_database(self.root_path.join(DATABASE_FILE_NAME))?;
    let changed = connection
      .execute(
        "UPDATE task_worker_lease
         SET lease_expires_at = ?1, updated_at = ?2
         WHERE id = 'task_worker' AND owner_id = ?3 AND generation = ?4
           AND lease_expires_at > ?5",
        params![
          expires_at,
          Utc::now().to_rfc3339(),
          self.fence.owner_id(),
          self.fence.generation(),
          now
        ],
      )
      .map_err(database_error)?;
    if changed == 1 {
      Ok(())
    } else {
      Err(worker_lease_error(
        "本地任务执行器租约已失效，已停止继续发送或提交采集请求",
      ))
    }
  }

  fn fence(&self) -> &WorkerFence {
    &self.fence
  }

  fn release(&mut self) -> AppResult<()> {
    if self.released {
      return Ok(());
    }
    let connection = open_workspace_database(self.root_path.join(DATABASE_FILE_NAME))?;
    connection
      .execute(
        "UPDATE task_worker_lease
         SET lease_expires_at = 0, updated_at = ?1
         WHERE id = 'task_worker' AND owner_id = ?2 AND generation = ?3",
        params![
          Utc::now().to_rfc3339(),
          self.fence.owner_id(),
          self.fence.generation()
        ],
      )
      .map_err(database_error)?;
    self.released = true;
    Ok(())
  }
}

impl Drop for TaskWorkerOwner {
  fn drop(&mut self) {
    let _ = self.release();
  }
}

pub fn recover_interrupted_runs(root_path: impl AsRef<Path>) -> AppResult<i64> {
  let root_path = root_path.as_ref();
  let Some(mut owner) = TaskWorkerOwner::try_acquire(root_path)? else {
    return Ok(0);
  };
  owner.ensure_current()?;
  let result = super::recovery::recover_interrupted_runs(root_path);
  finish_with_release(&mut owner, result)
}

pub fn execute_next_task(root_path: impl AsRef<Path>) -> AppResult<Option<TaskRunView>> {
  let root_path = root_path.as_ref();
  let Some(mut owner) = TaskWorkerOwner::try_acquire(root_path)? else {
    return Ok(None);
  };
  let result = super::worker::execute_next_task_with_owner(root_path, || owner.ensure_current());
  finish_with_release(&mut owner, result)
}

fn finish_with_release<T>(owner: &mut TaskWorkerOwner, result: AppResult<T>) -> AppResult<T> {
  let release = owner.release();
  match result {
    Err(error) => Err(error),
    Ok(value) => release.map(|()| value),
  }
}

fn claim_database_lease(root_path: &Path, owner_id: &str) -> AppResult<Option<WorkerFence>> {
  let now = Utc::now().timestamp_millis();
  let expires_at = lease_expiry(now)?;
  let mut connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let claimed = transaction
    .query_row(
      "INSERT INTO task_worker_lease (
         id, owner_id, lease_expires_at, created_at, updated_at, generation
       ) VALUES ('task_worker', ?1, ?2, ?3, ?3, 1)
       ON CONFLICT(id) DO UPDATE SET
         owner_id = excluded.owner_id,
         lease_expires_at = excluded.lease_expires_at,
         updated_at = excluded.updated_at,
         generation = task_worker_lease.generation + 1
       WHERE (task_worker_lease.lease_expires_at <= ?4
          OR task_worker_lease.owner_id = excluded.owner_id)
         AND task_worker_lease.generation < 9223372036854775807
      RETURNING owner_id, generation",
      params![owner_id, expires_at, Utc::now().to_rfc3339(), now],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )
    .optional()
    .map_err(database_error)?;
  transaction.commit().map_err(database_error)?;
  claimed
    .map(|(owner_id, generation)| WorkerFence::new(owner_id, generation))
    .transpose()
}

fn lease_expiry(now: i64) -> AppResult<i64> {
  now
    .checked_add(WORKER_LEASE_MILLIS)
    .ok_or_else(|| worker_lease_error("本地任务执行器租约时间超出可用范围"))
}

fn worker_lock_path(root_path: &Path) -> AppResult<PathBuf> {
  let lock_directory = root_path.join("temp");
  let metadata = fs::symlink_metadata(&lock_directory)
    .map_err(|error| lock_io_error("无法读取本地任务执行器锁目录", error))?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(lock_permission_error(
      "本地任务执行器锁目录必须是工作区内的真实目录",
    ));
  }
  Ok(lock_directory.join(WORKER_LOCK_FILE))
}

fn open_private_lock_file(lock_path: &Path) -> AppResult<File> {
  let file = match OpenOptions::new()
    .read(true)
    .write(true)
    .create_new(true)
    .mode(PRIVATE_FILE_MODE)
    .open(lock_path)
  {
    Ok(file) => file,
    Err(error) if error.kind() == ErrorKind::AlreadyExists => {
      validate_existing_lock_path(lock_path)?;
      OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|error| lock_io_error("无法打开本地任务执行器锁文件", error))?
    }
    Err(error) => return Err(lock_io_error("无法创建本地任务执行器锁文件", error)),
  };
  file
    .set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))
    .map_err(|error| lock_io_error("无法收紧本地任务执行器锁文件权限", error))?;
  validate_open_lock_file(lock_path, &file)?;
  Ok(file)
}

fn validate_existing_lock_path(lock_path: &Path) -> AppResult<()> {
  let metadata = fs::symlink_metadata(lock_path)
    .map_err(|error| lock_io_error("无法检查本地任务执行器锁文件", error))?;
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    return Err(lock_permission_error(
      "本地任务执行器锁必须是工作区内的普通文件",
    ));
  }
  Ok(())
}

fn validate_open_lock_file(lock_path: &Path, file: &File) -> AppResult<()> {
  let opened = file
    .metadata()
    .map_err(|error| lock_io_error("无法验证本地任务执行器锁文件", error))?;
  let current = fs::symlink_metadata(lock_path)
    .map_err(|error| lock_io_error("无法复核本地任务执行器锁文件", error))?;
  if current.file_type().is_symlink()
    || !opened.is_file()
    || !current.is_file()
    || opened.dev() != current.dev()
    || opened.ino() != current.ino()
    || opened.permissions().mode() & PERMISSION_BITS != PRIVATE_FILE_MODE
  {
    return Err(lock_permission_error(
      "本地任务执行器锁文件身份或权限校验失败",
    ));
  }
  Ok(())
}

fn lock_io_error(message: &str, error: std::io::Error) -> AppError {
  let retryable = matches!(
    error.kind(),
    ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut
  );
  AppError::new(
    if error.kind() == ErrorKind::PermissionDenied {
      AppErrorCode::PermissionError
    } else {
      AppErrorCode::WorkspaceError
    },
    format!("{message}：{error}"),
    AppErrorStage::Workspace,
    retryable,
  )
  .with_safe_detail("operation", "task_worker_lock")
}

fn lock_permission_error(message: &str) -> AppError {
  AppError::new(
    AppErrorCode::PermissionError,
    message,
    AppErrorStage::Workspace,
    false,
  )
  .with_safe_detail("operation", "task_worker_lock")
}

fn worker_lease_error(message: &str) -> AppError {
  AppError::new(
    AppErrorCode::WorkspaceError,
    message,
    AppErrorStage::Workspace,
    true,
  )
  .with_safe_detail("operation", "task_worker_lease")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::workspace::create_workspace;

  #[test]
  fn deleted_lock_path_cannot_create_a_second_worker_owner() {
    let root = std::env::temp_dir().join(format!("worker-lock-unlink-{}", Uuid::new_v4()));
    create_workspace("执行器锁删除回归", &root).expect("workspace should create");
    let first = TaskWorkerOwner::try_acquire(&root)
      .expect("first acquire should work")
      .expect("first owner should acquire");
    fs::remove_file(root.join("temp").join(WORKER_LOCK_FILE))
      .expect("visible lock path should be deleted for the regression");

    let second = TaskWorkerOwner::try_acquire(&root).expect("second acquire should be handled");

    assert!(
      second.is_none(),
      "database lease must reject a second owner"
    );
    drop(first);
    assert!(TaskWorkerOwner::try_acquire(&root)
      .expect("released lease should be available")
      .is_some());
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn stolen_or_expired_database_lease_stops_the_old_owner() {
    let root = std::env::temp_dir().join(format!("worker-lease-stolen-{}", Uuid::new_v4()));
    create_workspace("执行器租约失效回归", &root).expect("workspace should create");
    let owner = TaskWorkerOwner::try_acquire(&root)
      .expect("owner should acquire")
      .expect("lease should be available");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute(
        "UPDATE task_worker_lease SET owner_id = 'replacement', lease_expires_at = 0",
        [],
      )
      .unwrap();

    let error = owner
      .ensure_current()
      .expect_err("stale owner must fail closed");

    assert!(error.message.contains("租约已失效"));
    drop(owner);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn replacing_an_expired_owner_advances_the_fence_generation() {
    let root = std::env::temp_dir().join(format!("worker-fence-generation-{}", Uuid::new_v4()));
    create_workspace("执行器栅栏代次回归", &root).expect("workspace should create");
    let first = TaskWorkerOwner::try_acquire(&root)
      .expect("first owner should acquire")
      .expect("lease should be available");
    let first_generation = first.fence().generation();
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute(
        "UPDATE task_worker_lease SET lease_expires_at = 0 WHERE id = 'task_worker'",
        [],
      )
      .unwrap();
    fs::remove_file(root.join("temp").join(WORKER_LOCK_FILE)).unwrap();

    let second = TaskWorkerOwner::try_acquire(&root)
      .expect("replacement owner should acquire")
      .expect("expired lease should be replaceable");

    assert_eq!(second.fence().generation(), first_generation + 1);
    first
      .ensure_current()
      .expect_err("an older generation must remain fenced out");
    second
      .ensure_current()
      .expect("the latest generation must remain current");
    drop(first);
    drop(second);
    fs::remove_dir_all(root).ok();
  }
}
