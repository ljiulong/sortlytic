use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{database_error, open_workspace_database, DATABASE_FILE_NAME};
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
  directory: DispatchLockFileIdentity,
  shards: Vec<DispatchLockFileIdentity>,
}

#[derive(Debug, Clone)]
struct DispatchLockPoolSpec {
  version: u32,
  pool_id: String,
  shard_count: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct DispatchLockFileIdentity {
  device: u64,
  inode: u64,
}

pub(super) fn initialize_task_dispatch_lock_pool(
  root_path: &Path,
  connection: &mut Connection,
) -> AppResult<()> {
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let workspace_id = transaction
    .query_row("SELECT id FROM workspace LIMIT 1", [], |row| {
      row.get::<_, String>(0)
    })
    .map_err(database_error)?;
  let manifest = load_manifest(&transaction)?;
  match manifest {
    Some(manifest) => validate_lock_pool(root_path, &manifest)?,
    None => {
      let spec = dispatch_lock_pool_spec(&workspace_id);
      create_lock_pool(root_path, &spec)?;
      let manifest = capture_lock_pool_manifest(root_path, &spec)?;
      validate_lock_pool(root_path, &manifest)?;
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
  transaction.commit().map_err(database_error)?;
  open_task_dispatch_lock(root_path, "workspace-lock-pool-self-check").map(drop)
}

pub(crate) fn open_task_dispatch_lock(root_path: &Path, task_id: &str) -> AppResult<File> {
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))?;
  let manifest = load_manifest(&connection)?
    .ok_or_else(|| dispatch_lock_error("任务请求分发锁清单不存在", None))?;
  validate_manifest(&manifest)?;
  validate_lock_pool_directory(root_path, Some(&manifest.directory))?;
  let spec = manifest.pool_spec();
  open_and_validate_shard(
    &lock_path(root_path, task_id),
    &spec,
    shard_index(task_id),
    manifest.shards.get(shard_index(task_id)),
  )
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
  validate_pool_spec(&manifest.pool_spec())?;
  if manifest.shards.len() != DISPATCH_LOCK_SHARDS {
    return Err(dispatch_lock_error("任务请求分发锁清单无效", None));
  }
  Ok(())
}

impl DispatchLockManifest {
  fn pool_spec(&self) -> DispatchLockPoolSpec {
    DispatchLockPoolSpec {
      version: self.version,
      pool_id: self.pool_id.clone(),
      shard_count: self.shard_count,
    }
  }
}

fn validate_pool_spec(spec: &DispatchLockPoolSpec) -> AppResult<()> {
  if spec.version != DISPATCH_LOCK_POOL_VERSION
    || spec.shard_count != DISPATCH_LOCK_SHARDS
    || Uuid::parse_str(&spec.pool_id).is_err()
  {
    return Err(dispatch_lock_error("任务请求分发锁清单无效", None));
  }
  Ok(())
}

fn dispatch_lock_pool_spec(workspace_id: &str) -> DispatchLockPoolSpec {
  let mut hasher = Sha256::new();
  hasher.update(b"sortlytic-task-dispatch-pool-v1:");
  hasher.update(workspace_id.as_bytes());
  let digest = hasher.finalize();
  let mut bytes = [0_u8; 16];
  bytes.copy_from_slice(&digest[..16]);
  bytes[6] = (bytes[6] & 0x0f) | 0x50;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  DispatchLockPoolSpec {
    version: DISPATCH_LOCK_POOL_VERSION,
    pool_id: Uuid::from_bytes(bytes).to_string(),
    shard_count: DISPATCH_LOCK_SHARDS,
  }
}

fn create_lock_pool(root_path: &Path, spec: &DispatchLockPoolSpec) -> AppResult<()> {
  validate_pool_spec(spec)?;
  let directory = lock_directory(root_path);
  match fs::symlink_metadata(&directory) {
    Ok(_) => {
      validate_lock_pool_directory(root_path, None)?;
    }
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
    create_or_recover_bootstrap_shard(&path, spec, index)?;
  }
  sync_directory(&directory)?;
  sync_directory(&root_path.join("temp"))?;
  sync_directory(root_path)?;
  Ok(())
}

fn create_or_recover_bootstrap_shard(
  path: &Path,
  spec: &DispatchLockPoolSpec,
  index: usize,
) -> AppResult<()> {
  match create_shard(path, spec, index) {
    Ok(()) => Ok(()),
    Err(error) if error.kind() == ErrorKind::AlreadyExists => {
      if open_and_validate_shard(path, spec, index, None).is_ok() {
        return Ok(());
      }
      fs::remove_file(path)
        .map_err(|error| dispatch_lock_error("无法清理未登记的任务请求分发锁", Some(&error)))?;
      create_shard(path, spec, index)
        .map_err(|error| dispatch_lock_error("无法重建任务请求分发锁", Some(&error)))
    }
    Err(error) => Err(dispatch_lock_error("无法创建任务请求分发锁", Some(&error))),
  }
}

fn create_shard(path: &Path, spec: &DispatchLockPoolSpec, index: usize) -> std::io::Result<()> {
  let mut file = OpenOptions::new()
    .read(true)
    .write(true)
    .create_new(true)
    .mode(DISPATCH_LOCK_MODE)
    .custom_flags(NO_FOLLOW_FLAG)
    .open(path)?;
  file.write_all(shard_identity(spec, index).as_bytes())?;
  file.sync_all()?;
  validate_opened_shard(path, &file, spec, index, None)
    .map_err(|error| std::io::Error::other(error.message))
}

fn capture_lock_pool_manifest(
  root_path: &Path,
  spec: &DispatchLockPoolSpec,
) -> AppResult<DispatchLockManifest> {
  let directory = validate_lock_pool_directory(root_path, None)?;
  let mut shards = Vec::with_capacity(DISPATCH_LOCK_SHARDS);
  for index in 0..DISPATCH_LOCK_SHARDS {
    let file = open_and_validate_shard(&shard_path(root_path, index), spec, index, None)?;
    let metadata = file
      .metadata()
      .map_err(|error| dispatch_lock_error("无法登记任务请求分发锁身份", Some(&error)))?;
    shards.push(file_identity(&metadata));
  }
  Ok(DispatchLockManifest {
    version: spec.version,
    pool_id: spec.pool_id.clone(),
    shard_count: spec.shard_count,
    directory: file_identity(&directory),
    shards,
  })
}

fn validate_lock_pool(root_path: &Path, manifest: &DispatchLockManifest) -> AppResult<()> {
  validate_manifest(manifest)?;
  validate_lock_pool_directory(root_path, Some(&manifest.directory))?;
  let spec = manifest.pool_spec();
  for index in 0..DISPATCH_LOCK_SHARDS {
    open_and_validate_shard(
      &shard_path(root_path, index),
      &spec,
      index,
      manifest.shards.get(index),
    )?;
  }
  Ok(())
}

fn validate_lock_pool_directory(
  root_path: &Path,
  expected: Option<&DispatchLockFileIdentity>,
) -> AppResult<fs::Metadata> {
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
  if expected.is_some_and(|expected| !identity_matches(&metadata, expected)) {
    return Err(dispatch_lock_error("任务请求分发锁池登记身份不匹配", None));
  }
  Ok(metadata)
}

fn open_and_validate_shard(
  path: &Path,
  spec: &DispatchLockPoolSpec,
  index: usize,
  expected: Option<&DispatchLockFileIdentity>,
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
  validate_opened_shard(path, &file, spec, index, expected)?;
  Ok(file)
}

fn validate_opened_shard(
  path: &Path,
  file: &File,
  spec: &DispatchLockPoolSpec,
  index: usize,
  expected: Option<&DispatchLockFileIdentity>,
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
    || expected.is_some_and(|expected| !identity_matches(&opened, expected))
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
  if identity != shard_identity(spec, index) {
    return Err(dispatch_lock_error("任务请求分发锁身份不匹配", None));
  }
  Ok(())
}

fn sync_directory(path: &Path) -> AppResult<()> {
  File::open(path)
    .and_then(|directory| directory.sync_all())
    .map_err(|error| dispatch_lock_error("无法同步任务请求分发锁目录", Some(&error)))
}

fn file_identity(metadata: &fs::Metadata) -> DispatchLockFileIdentity {
  DispatchLockFileIdentity {
    device: metadata.dev(),
    inode: metadata.ino(),
  }
}

fn identity_matches(metadata: &fs::Metadata, expected: &DispatchLockFileIdentity) -> bool {
  metadata.dev() == expected.device && metadata.ino() == expected.inode
}

fn lock_directory(root_path: &Path) -> PathBuf {
  root_path.join("temp").join(DISPATCH_LOCK_DIRECTORY)
}

fn lock_path(root_path: &Path, task_id: &str) -> PathBuf {
  shard_path(root_path, shard_index(task_id))
}

fn shard_path(root_path: &Path, index: usize) -> PathBuf {
  lock_directory(root_path).join(format!("task-dispatch-{index:02x}.lock"))
}

fn shard_index(task_id: &str) -> usize {
  usize::from(Sha256::digest(task_id.as_bytes())[0]) % DISPATCH_LOCK_SHARDS
}

fn shard_identity(spec: &DispatchLockPoolSpec, index: usize) -> String {
  format!(
    "sortlytic-task-dispatch:{}:{}:{index:02x}\n",
    spec.version, spec.pool_id
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
  use fs4::FileExt;

  #[test]
  fn workspace_initializes_a_bounded_dispatch_lock_pool() {
    let root = std::env::temp_dir().join(format!("dispatch-lock-pool-{}", Uuid::new_v4()));
    create_workspace("固定分发锁池", &root).expect("workspace should be created");

    for index in 0..96 {
      open_task_dispatch_lock(&root, &format!("task-{index}"))
        .expect("every task should resolve to a preinitialized shard");
    }

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

  #[test]
  fn opening_workspace_recovers_a_partial_pool_without_a_manifest() {
    let root = std::env::temp_dir().join(format!("dispatch-lock-recovery-{}", Uuid::new_v4()));
    create_workspace("分发锁恢复", &root).expect("workspace should be created");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let deleted = connection
      .execute(
        "DELETE FROM audit_log WHERE action = ?1",
        [DISPATCH_LOCK_ACTION],
      )
      .expect("dispatch lock manifest should delete");
    assert_eq!(deleted, 1);
    drop(connection);
    fs::remove_file(shard_path(&root, DISPATCH_LOCK_SHARDS - 1))
      .expect("one bootstrap shard should be removed");
    OpenOptions::new()
      .write(true)
      .truncate(true)
      .open(shard_path(&root, DISPATCH_LOCK_SHARDS - 2))
      .expect("one bootstrap shard should be left partially written");

    open_workspace(&root).expect("partial bootstrap should recover idempotently");

    assert_eq!(pool_lock_count(&root), DISPATCH_LOCK_SHARDS);
    open_task_dispatch_lock(&root, "recovered-task")
      .expect("recovered pool should serve task locks");
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn opening_a_task_lock_rejects_a_replaced_registered_shard() {
    let root = std::env::temp_dir().join(format!("dispatch-lock-replace-{}", Uuid::new_v4()));
    create_workspace("分发锁文件替换", &root).expect("workspace should be created");
    let task_id = "replaced-shard-task";
    let path = lock_path(&root, task_id);
    let original = open_task_dispatch_lock(&root, task_id).expect("original shard should open");
    FileExt::lock(&original).expect("original shard should lock");
    let identity = fs::read_to_string(&path).expect("registered identity should read");
    fs::remove_file(&path).expect("registered shard path should unlink");
    let mut replacement = OpenOptions::new()
      .read(true)
      .write(true)
      .create_new(true)
      .mode(DISPATCH_LOCK_MODE)
      .open(&path)
      .expect("replacement shard should create");
    replacement
      .write_all(identity.as_bytes())
      .and_then(|()| replacement.sync_all())
      .expect("replacement identity should persist");
    assert_ne!(
      original.metadata().expect("original metadata").ino(),
      replacement.metadata().expect("replacement metadata").ino()
    );

    let second = open_task_dispatch_lock(&root, task_id);
    if let Ok(file) = &second {
      assert!(
        FileExt::try_lock(file).is_err(),
        "a copied replacement must not create a second lock domain"
      );
    }
    let error = second.expect_err("a replaced registered shard must fail closed");
    assert_eq!(
      error.safe_details.get("operation").map(String::as_str),
      Some("task_dispatch_gate")
    );
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn opening_a_task_lock_rejects_a_replaced_pool_directory() {
    let root = std::env::temp_dir().join(format!("dispatch-lock-dir-replace-{}", Uuid::new_v4()));
    create_workspace("分发锁目录替换", &root).expect("workspace should be created");
    let task_id = "replaced-directory-task";
    let original_directory = lock_directory(&root);
    let displaced_directory = root.join("temp/task-dispatch-gates-v1-displaced");
    let original = open_task_dispatch_lock(&root, task_id).expect("original shard should open");
    FileExt::lock(&original).expect("original shard should lock");
    fs::rename(&original_directory, &displaced_directory)
      .expect("registered pool should move aside");
    let mut builder = DirBuilder::new();
    builder.mode(PRIVATE_DIRECTORY_MODE);
    builder
      .create(&original_directory)
      .expect("replacement pool should create");
    for index in 0..DISPATCH_LOCK_SHARDS {
      let source = displaced_directory.join(format!("task-dispatch-{index:02x}.lock"));
      let destination = shard_path(&root, index);
      fs::copy(source, &destination).expect("registered identity should copy");
      fs::set_permissions(&destination, fs::Permissions::from_mode(DISPATCH_LOCK_MODE))
        .expect("replacement shard permissions should set");
    }

    let second = open_task_dispatch_lock(&root, task_id);
    if let Ok(file) = &second {
      assert!(
        FileExt::try_lock(file).is_err(),
        "a copied pool must not create a second lock domain"
      );
    }
    let error = second.expect_err("a replaced registered pool must fail closed");
    assert_eq!(
      error.safe_details.get("operation").map(String::as_str),
      Some("task_dispatch_gate")
    );
    std::fs::remove_dir_all(root).ok();
  }

  fn pool_lock_count(root_path: &Path) -> usize {
    fs::read_dir(lock_directory(root_path))
      .expect("lock pool should be readable")
      .filter_map(Result::ok)
      .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_file()))
      .count()
  }
}
