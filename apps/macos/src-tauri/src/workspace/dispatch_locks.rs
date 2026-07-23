use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::database_error;
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

const DISPATCH_LOCK_ACTION: &str = "initialize_task_dispatch_lock_pool";
const DISPATCH_LOCK_DIRECTORY: &str = "task-dispatch-gates-v1";
const DISPATCH_LOCK_MODE: u32 = 0o600;
const DISPATCH_LOCK_POOL_VERSION: u32 = 1;
const DISPATCH_LOCK_SHARDS: usize = 64;
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;

#[cfg(target_os = "macos")]
const NO_FOLLOW_FLAG: i32 = 0x0000_0100;
#[cfg(target_os = "linux")]
const NO_FOLLOW_FLAG: i32 = 0x0002_0000;
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const NO_FOLLOW_FLAG: i32 = 0;

#[derive(Debug, Clone, Deserialize, Serialize)]
struct DispatchLockManifest {
  version: u32,
  pool_id: String,
  shard_count: usize,
}

pub(super) fn initialize_task_dispatch_lock_pool(
  root_path: &Path,
  connection: &mut Connection,
) -> AppResult<()> {
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let manifest = load_manifest(&transaction)?;
  match manifest {
    Some(manifest) => validate_lock_pool(root_path, &manifest)?,
    None => {
      let manifest = DispatchLockManifest {
        version: DISPATCH_LOCK_POOL_VERSION,
        pool_id: Uuid::new_v4().to_string(),
        shard_count: DISPATCH_LOCK_SHARDS,
      };
      create_lock_pool(root_path, &manifest)?;
      let workspace_id = transaction
        .query_row("SELECT id FROM workspace LIMIT 1", [], |row| {
          row.get::<_, String>(0)
        })
        .map_err(database_error)?;
      transaction
        .execute(
          "INSERT INTO audit_log (
             id, entity_type, entity_id, action, safe_details_json, created_at
           ) VALUES (?1, 'workspace', ?2, ?3, ?4, ?5)",
          params![
            Uuid::new_v4().to_string(),
            workspace_id,
            DISPATCH_LOCK_ACTION,
            serde_json::to_string(&manifest).map_err(|error| {
              dispatch_lock_error("无法序列化任务请求分发锁清单", Some(&error))
            })?,
            Utc::now().to_rfc3339()
          ],
        )
        .map_err(database_error)?;
    }
  }
  transaction.commit().map_err(database_error)
}

fn load_manifest(connection: &Connection) -> AppResult<Option<DispatchLockManifest>> {
  let manifest = connection
    .query_row(
      "SELECT safe_details_json
       FROM audit_log
       WHERE entity_type = 'workspace' AND action = ?1
       ORDER BY rowid DESC
       LIMIT 1",
      [DISPATCH_LOCK_ACTION],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?;
  manifest
    .map(|manifest| {
      serde_json::from_str::<DispatchLockManifest>(&manifest)
        .map_err(|error| dispatch_lock_error("任务请求分发锁清单无法解析", Some(&error)))
        .and_then(|manifest| {
          validate_manifest(&manifest)?;
          Ok(manifest)
        })
    })
    .transpose()
}

fn validate_manifest(manifest: &DispatchLockManifest) -> AppResult<()> {
  if manifest.version != DISPATCH_LOCK_POOL_VERSION
    || manifest.shard_count != DISPATCH_LOCK_SHARDS
    || Uuid::parse_str(&manifest.pool_id).is_err()
  {
    return Err(dispatch_lock_error("任务请求分发锁清单无效", None));
  }
  Ok(())
}

fn create_lock_pool(root_path: &Path, manifest: &DispatchLockManifest) -> AppResult<()> {
  validate_manifest(manifest)?;
  let directory = lock_directory(root_path);
  match fs::symlink_metadata(&directory) {
    Ok(_) => validate_lock_pool_directory(root_path)?,
    Err(error) if error.kind() == ErrorKind::NotFound => {
      let mut builder = DirBuilder::new();
      builder.mode(PRIVATE_DIRECTORY_MODE);
      builder
        .create(&directory)
        .map_err(|error| dispatch_lock_error("无法创建任务请求分发锁池", Some(&error)))?;
    }
    Err(error) => {
      return Err(dispatch_lock_error(
        "无法检查任务请求分发锁池",
        Some(&error),
      ))
    }
  }
  for index in 0..DISPATCH_LOCK_SHARDS {
    let path = shard_path(root_path, index);
    match OpenOptions::new()
      .read(true)
      .write(true)
      .create_new(true)
      .mode(DISPATCH_LOCK_MODE)
      .custom_flags(NO_FOLLOW_FLAG)
      .open(&path)
    {
      Ok(mut file) => {
        file
          .write_all(shard_identity(manifest, index).as_bytes())
          .and_then(|()| file.sync_all())
          .map_err(|error| dispatch_lock_error("无法写入任务请求分发锁身份", Some(&error)))?;
        validate_opened_shard(&path, &file, manifest, index)?;
      }
      Err(error) if error.kind() == ErrorKind::AlreadyExists => {
        open_and_validate_shard(&path, manifest, index)?;
      }
      Err(error) => return Err(dispatch_lock_error("无法创建任务请求分发锁", Some(&error))),
    }
  }
  Ok(())
}

fn validate_lock_pool(root_path: &Path, manifest: &DispatchLockManifest) -> AppResult<()> {
  validate_manifest(manifest)?;
  validate_lock_pool_directory(root_path)?;
  for index in 0..DISPATCH_LOCK_SHARDS {
    open_and_validate_shard(&shard_path(root_path, index), manifest, index)?;
  }
  Ok(())
}

fn validate_lock_pool_directory(root_path: &Path) -> AppResult<()> {
  let directory = lock_directory(root_path);
  let metadata = fs::symlink_metadata(&directory)
    .map_err(|error| dispatch_lock_error("无法读取任务请求分发锁池", Some(&error)))?;
  if metadata.file_type().is_symlink()
    || !metadata.is_dir()
    || metadata.permissions().mode() & 0o7777 != PRIVATE_DIRECTORY_MODE
  {
    return Err(dispatch_lock_error(
      "任务请求分发锁池必须是权限为 0700 的真实目录",
      None,
    ));
  }
  Ok(())
}

fn open_and_validate_shard(
  path: &Path,
  manifest: &DispatchLockManifest,
  index: usize,
) -> AppResult<File> {
  let current = fs::symlink_metadata(path)
    .map_err(|error| dispatch_lock_error("无法检查任务请求分发锁", Some(&error)))?;
  if current.file_type().is_symlink() || !current.is_file() {
    return Err(dispatch_lock_error(
      "任务请求分发锁必须是工作区内的普通文件",
      None,
    ));
  }
  let file = OpenOptions::new()
    .read(true)
    .write(true)
    .custom_flags(NO_FOLLOW_FLAG)
    .open(path)
    .map_err(|error| dispatch_lock_error("无法打开任务请求分发锁", Some(&error)))?;
  validate_opened_shard(path, &file, manifest, index)?;
  Ok(file)
}

fn validate_opened_shard(
  path: &Path,
  file: &File,
  manifest: &DispatchLockManifest,
  index: usize,
) -> AppResult<()> {
  let opened = file
    .metadata()
    .map_err(|error| dispatch_lock_error("无法验证任务请求分发锁", Some(&error)))?;
  let current = fs::symlink_metadata(path)
    .map_err(|error| dispatch_lock_error("无法复核任务请求分发锁", Some(&error)))?;
  if current.file_type().is_symlink()
    || !opened.is_file()
    || !current.is_file()
    || opened.dev() != current.dev()
    || opened.ino() != current.ino()
    || opened.nlink() != 1
    || opened.permissions().mode() & 0o7777 != DISPATCH_LOCK_MODE
  {
    return Err(dispatch_lock_error(
      "任务请求分发锁身份、链接数或权限校验失败",
      None,
    ));
  }
  let mut reader = file
    .try_clone()
    .map_err(|error| dispatch_lock_error("无法读取任务请求分发锁身份", Some(&error)))?;
  reader
    .seek(SeekFrom::Start(0))
    .map_err(|error| dispatch_lock_error("无法定位任务请求分发锁身份", Some(&error)))?;
  let mut identity = String::new();
  reader
    .take(256)
    .read_to_string(&mut identity)
    .map_err(|error| dispatch_lock_error("无法读取任务请求分发锁身份", Some(&error)))?;
  if identity != shard_identity(manifest, index) {
    return Err(dispatch_lock_error("任务请求分发锁身份不匹配", None));
  }
  Ok(())
}

fn lock_directory(root_path: &Path) -> PathBuf {
  root_path.join("temp").join(DISPATCH_LOCK_DIRECTORY)
}

fn shard_path(root_path: &Path, index: usize) -> PathBuf {
  lock_directory(root_path).join(format!("task-dispatch-{index:02x}.lock"))
}

fn shard_identity(manifest: &DispatchLockManifest, index: usize) -> String {
  format!(
    "sortlytic-task-dispatch:{}:{}:{index:02x}\n",
    manifest.version, manifest.pool_id
  )
}

fn dispatch_lock_error(message: &str, error: Option<&dyn std::fmt::Display>) -> AppError {
  AppError::new(
    if error.is_none() {
      AppErrorCode::PermissionError
    } else {
      AppErrorCode::WorkspaceError
    },
    error.map_or(message.to_string(), |error| format!("{message}：{error}")),
    AppErrorStage::Workspace,
    false,
  )
  .with_safe_detail("operation", "task_dispatch_gate")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::workspace::{create_workspace, open_workspace};

  #[test]
  fn workspace_initializes_a_bounded_dispatch_lock_pool() {
    let root = std::env::temp_dir().join(format!("dispatch-lock-pool-{}", Uuid::new_v4()));
    create_workspace("固定分发锁池", &root).expect("workspace should be created");

    let pool_locks = fs::read_dir(lock_directory(&root))
      .expect("lock pool should be readable")
      .filter_map(Result::ok)
      .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
      .count();

    assert_eq!(pool_locks, DISPATCH_LOCK_SHARDS);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn opening_workspace_rejects_a_removed_registered_shard() {
    let root = std::env::temp_dir().join(format!("dispatch-lock-tamper-{}", Uuid::new_v4()));
    create_workspace("分发锁篡改", &root).expect("workspace should be created");
    fs::remove_file(shard_path(&root, 0)).expect("registered lock shard should be removed");

    let error = open_workspace(&root).expect_err("missing registered shard must fail closed");

    assert_eq!(
      error.safe_details.get("operation").map(String::as_str),
      Some("task_dispatch_gate")
    );
    std::fs::remove_dir_all(root).ok();
  }
}
