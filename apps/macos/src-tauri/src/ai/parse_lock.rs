use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use fs4::{FileExt, TryLockError};
use sha2::{Digest, Sha256};

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

const PRIVATE_FILE_MODE: u32 = 0o600;
const PERMISSION_BITS: u32 = 0o7777;

#[derive(Debug)]
pub(super) struct NaturalParseLock {
  _file: File,
}

impl NaturalParseLock {
  pub(super) fn acquire(root_path: &Path, task_id: &str) -> AppResult<Self> {
    let lock_path = lock_path(root_path, task_id)?;
    let file = open_private_lock_file(&lock_path)?;
    match FileExt::try_lock(&file) {
      Ok(()) => Ok(Self { _file: file }),
      Err(TryLockError::WouldBlock) => Err(
        AppError::new(
          AppErrorCode::ModelRequestError,
          "该任务正在解析，请等待当前尝试完成后再重新解析",
          AppErrorStage::Ai,
          true,
        )
        .with_safe_detail("reason", "natural_parse_locked")
        .with_safe_detail("transport_kind", "busy"),
      ),
      Err(TryLockError::Error(error)) => Err(lock_io_error("无法取得自然语言解析锁", error)),
    }
  }
}

fn lock_path(root_path: &Path, task_id: &str) -> AppResult<PathBuf> {
  let lock_directory = root_path.join("temp");
  let metadata = fs::symlink_metadata(&lock_directory)
    .map_err(|error| lock_io_error("无法读取自然语言解析锁目录", error))?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(lock_permission_error(
      "自然语言解析锁目录必须是工作区内的真实目录",
    ));
  }
  let task_hash = format!("{:x}", Sha256::digest(task_id.as_bytes()));
  Ok(lock_directory.join(format!("natural-parse-{task_hash}.lock")))
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
        .map_err(|error| lock_io_error("无法打开自然语言解析锁文件", error))?
    }
    Err(error) => return Err(lock_io_error("无法创建自然语言解析锁文件", error)),
  };
  file
    .set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))
    .map_err(|error| lock_io_error("无法收紧自然语言解析锁文件权限", error))?;
  validate_open_lock_file(lock_path, &file)?;
  Ok(file)
}

fn validate_existing_lock_path(lock_path: &Path) -> AppResult<()> {
  let metadata = fs::symlink_metadata(lock_path)
    .map_err(|error| lock_io_error("无法检查自然语言解析锁文件", error))?;
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    return Err(lock_permission_error(
      "自然语言解析锁必须是工作区内的普通文件",
    ));
  }
  Ok(())
}

fn validate_open_lock_file(lock_path: &Path, file: &File) -> AppResult<()> {
  let opened = file
    .metadata()
    .map_err(|error| lock_io_error("无法验证自然语言解析锁文件", error))?;
  let current = fs::symlink_metadata(lock_path)
    .map_err(|error| lock_io_error("无法复核自然语言解析锁文件", error))?;
  if current.file_type().is_symlink()
    || !opened.is_file()
    || !current.is_file()
    || opened.dev() != current.dev()
    || opened.ino() != current.ino()
    || opened.permissions().mode() & PERMISSION_BITS != PRIVATE_FILE_MODE
  {
    return Err(lock_permission_error(
      "自然语言解析锁文件身份或权限校验失败",
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
  .with_safe_detail("operation", "natural_parse_lock")
}

fn lock_permission_error(message: &str) -> AppError {
  AppError::new(
    AppErrorCode::PermissionError,
    message,
    AppErrorStage::Workspace,
    false,
  )
  .with_safe_detail("operation", "natural_parse_lock")
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::os::unix::fs::PermissionsExt;

  use uuid::Uuid;

  use super::*;
  use crate::domain::AppErrorCode;
  use crate::workspace::create_workspace;

  #[test]
  fn prevents_a_second_lock_for_the_same_task() {
    let root = workspace("same-task");
    let _first = NaturalParseLock::acquire(&root, "task-1").unwrap();

    let error = NaturalParseLock::acquire(&root, "task-1").unwrap_err();

    assert_eq!(error.code, AppErrorCode::ModelRequestError);
    assert!(error.retryable);
    assert_eq!(
      error.safe_details.get("reason").map(String::as_str),
      Some("natural_parse_locked")
    );
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn allows_different_tasks_to_lock_concurrently() {
    let root = workspace("different-tasks");

    let _first = NaturalParseLock::acquire(&root, "task-1").unwrap();
    let _second = NaturalParseLock::acquire(&root, "task-2").unwrap();

    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn releases_the_lock_when_the_guard_is_dropped() {
    let root = workspace("release");
    let first = NaturalParseLock::acquire(&root, "task-1").unwrap();
    drop(first);

    let _second = NaturalParseLock::acquire(&root, "task-1").unwrap();

    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn creates_private_lock_files_without_exposing_task_ids() {
    let root = workspace("permissions");
    let _lock = NaturalParseLock::acquire(&root, "sensitive-task-id").unwrap();
    let entries = fs::read_dir(root.join("temp"))
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();

    assert!(entries.iter().all(|entry| !entry
      .file_name()
      .to_string_lossy()
      .contains("sensitive-task-id")));
    let lock_entries = entries
      .iter()
      .filter(|entry| {
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        file_name.starts_with("natural-parse-") && file_name.ends_with(".lock")
      })
      .collect::<Vec<_>>();
    assert_eq!(lock_entries.len(), 1);
    let file_name = lock_entries[0].file_name().to_string_lossy().into_owned();
    assert!(file_name.starts_with("natural-parse-"));
    assert!(file_name.ends_with(".lock"));
    assert!(!file_name.contains("sensitive-task-id"));
    assert_eq!(
      lock_entries[0].metadata().unwrap().permissions().mode() & 0o7777,
      0o600
    );
    fs::remove_dir_all(root).ok();
  }

  fn workspace(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("natural-parse-lock-{label}-{}", Uuid::new_v4()));
    create_workspace("解析锁测试", &root).unwrap();
    root
  }
}
