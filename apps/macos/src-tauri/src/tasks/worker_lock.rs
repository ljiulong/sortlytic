use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use fs4::{FileExt, TryLockError};

use super::TaskRunView;
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PERMISSION_BITS: u32 = 0o7777;
const WORKER_LOCK_FILE: &str = "task-worker.lock";

struct TaskWorkerOwner {
  _file: File,
}

impl TaskWorkerOwner {
  fn try_acquire(root_path: &Path) -> AppResult<Option<Self>> {
    let lock_path = worker_lock_path(root_path)?;
    let file = open_private_lock_file(&lock_path)?;
    match FileExt::try_lock(&file) {
      Ok(()) => Ok(Some(Self { _file: file })),
      Err(TryLockError::WouldBlock) => Ok(None),
      Err(TryLockError::Error(error)) => Err(lock_io_error("无法取得本地任务执行器所有权", error)),
    }
  }
}

pub fn recover_interrupted_runs(root_path: impl AsRef<Path>) -> AppResult<i64> {
  let root_path = root_path.as_ref();
  let Some(_owner) = TaskWorkerOwner::try_acquire(root_path)? else {
    return Ok(0);
  };
  super::recovery::recover_interrupted_runs(root_path)
}

pub fn execute_next_task(root_path: impl AsRef<Path>) -> AppResult<Option<TaskRunView>> {
  let root_path = root_path.as_ref();
  let Some(_owner) = TaskWorkerOwner::try_acquire(root_path)? else {
    return Ok(None);
  };
  super::worker::execute_next_task(root_path)
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
