use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::{collections::BTreeMap, sync::LazyLock, sync::Mutex};

use uuid::Uuid;

use super::{registry_error, validate_registry, ApiProfileRegistry};
use crate::domain::AppResult;

const REGISTRY_DIRECTORY_NAME: &str = "secrets";
const REGISTRY_FILE_NAME: &str = "api-config.json";
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PERMISSION_BITS: u32 = 0o7777;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WriteFailurePoint {
  TempPermissions,
  TempWrite,
  TempSync,
  Rename,
}

#[cfg(test)]
impl WriteFailurePoint {
  pub(super) const fn label(self) -> &'static str {
    match self {
      Self::TempPermissions => "temp-permissions",
      Self::TempWrite => "temp-write",
      Self::TempSync => "temp-sync",
      Self::Rename => "rename",
    }
  }

  const fn error_context(self) -> &'static str {
    match self {
      Self::TempPermissions => "无法设置 API 配置临时文件权限",
      Self::TempWrite => "无法写入 API 配置临时文件",
      Self::TempSync => "无法同步 API 配置临时文件",
      Self::Rename => "无法原子替换 API 配置文件",
    }
  }
}

#[cfg(test)]
static WRITE_FAILURES: LazyLock<Mutex<BTreeMap<PathBuf, WriteFailurePoint>>> =
  LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[cfg(test)]
pub(super) struct WriteFailureGuard {
  final_path: PathBuf,
}

#[cfg(test)]
impl Drop for WriteFailureGuard {
  fn drop(&mut self) {
    WRITE_FAILURES
      .lock()
      .unwrap_or_else(|poisoned| poisoned.into_inner())
      .remove(&self.final_path);
  }
}

#[cfg(test)]
pub(super) fn install_write_failure(
  root_path: &Path,
  failure: WriteFailurePoint,
) -> WriteFailureGuard {
  let final_path = registry_path(root_path);
  WRITE_FAILURES
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .insert(final_path.clone(), failure);
  WriteFailureGuard { final_path }
}

pub(super) fn registry_path(root_path: &Path) -> PathBuf {
  root_path
    .join(REGISTRY_DIRECTORY_NAME)
    .join(REGISTRY_FILE_NAME)
}

pub(super) fn registry_exists(root_path: &Path) -> AppResult<bool> {
  validate_workspace_root(root_path)?;
  let directory = root_path.join(REGISTRY_DIRECTORY_NAME);
  match fs::symlink_metadata(&directory) {
    Ok(metadata) => validate_private_directory(&directory, &metadata)?,
    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
    Err(error) => return Err(io_error("无法检查 API 配置目录", error)),
  }
  let path = registry_path(root_path);
  match fs::symlink_metadata(&path) {
    Ok(metadata) => {
      validate_private_file(&path, &metadata)?;
      Ok(true)
    }
    Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
    Err(error) => Err(io_error("无法检查 API 配置文件", error)),
  }
}

pub(super) fn load_optional_registry(root_path: &Path) -> AppResult<Option<ApiProfileRegistry>> {
  if !registry_exists(root_path)? {
    return Ok(None);
  }
  read_registry_from_path(&registry_path(root_path)).map(Some)
}

pub(super) fn write_registry(root_path: &Path, registry: &ApiProfileRegistry) -> AppResult<()> {
  validate_workspace_root(root_path)?;
  let directory = ensure_private_registry_directory(root_path)?;
  let path = registry_path(root_path);
  match fs::symlink_metadata(&path) {
    Ok(metadata) => {
      validate_private_file(&path, &metadata)?;
      read_registry_from_path(&path)?;
    }
    Err(error) if error.kind() == ErrorKind::NotFound => {}
    Err(error) => return Err(io_error("无法检查 API 配置文件", error)),
  }

  let mut contents = serde_json::to_vec_pretty(registry)
    .map_err(|_| registry_error("API 配置无法序列化，未写入磁盘"))?;
  contents.push(b'\n');
  let temp_path = directory.join(format!(".api-config-{}.tmp", Uuid::new_v4()));
  let result = write_and_replace(&temp_path, &path, &directory, &contents);
  if result.is_err() {
    fs::remove_file(&temp_path).ok();
  }
  result
}

fn write_and_replace(
  temp_path: &Path,
  final_path: &Path,
  directory: &Path,
  contents: &[u8],
) -> AppResult<()> {
  let mut temp_file = OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(PRIVATE_FILE_MODE)
    .open(temp_path)
    .map_err(|error| io_error("无法创建 API 配置临时文件", error))?;
  #[cfg(test)]
  maybe_fail_write(final_path, WriteFailurePoint::TempPermissions)?;
  temp_file
    .set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))
    .map_err(|error| io_error("无法设置 API 配置临时文件权限", error))?;
  #[cfg(test)]
  maybe_fail_write(final_path, WriteFailurePoint::TempWrite)?;
  temp_file
    .write_all(contents)
    .map_err(|error| io_error("无法写入 API 配置临时文件", error))?;
  #[cfg(test)]
  maybe_fail_write(final_path, WriteFailurePoint::TempSync)?;
  temp_file
    .sync_all()
    .map_err(|error| io_error("无法同步 API 配置临时文件", error))?;
  validate_private_file(
    temp_path,
    &temp_file
      .metadata()
      .map_err(|error| io_error("无法检查 API 配置临时文件", error))?,
  )?;
  drop(temp_file);

  #[cfg(test)]
  maybe_fail_write(final_path, WriteFailurePoint::Rename)?;
  fs::rename(temp_path, final_path)
    .map_err(|error| io_error("无法原子替换 API 配置文件", error))?;
  let metadata = fs::symlink_metadata(final_path)
    .map_err(|error| io_error("无法检查写入后的 API 配置文件", error))?;
  validate_private_file(final_path, &metadata)?;
  File::open(directory)
    .and_then(|file| file.sync_all())
    .map_err(|error| io_error("无法同步 API 配置目录", error))
}

#[cfg(test)]
fn maybe_fail_write(final_path: &Path, failure: WriteFailurePoint) -> AppResult<()> {
  let should_fail = WRITE_FAILURES
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .get(final_path)
    .copied()
    == Some(failure);
  if should_fail {
    return Err(io_error(
      failure.error_context(),
      std::io::Error::other("injected storage failure"),
    ));
  }
  Ok(())
}

fn read_registry_from_path(path: &Path) -> AppResult<ApiProfileRegistry> {
  let path_metadata =
    fs::symlink_metadata(path).map_err(|error| io_error("无法检查 API 配置文件", error))?;
  validate_private_file(path, &path_metadata)?;
  let mut file = File::open(path).map_err(|error| io_error("无法打开 API 配置文件", error))?;
  let opened_metadata = file
    .metadata()
    .map_err(|error| io_error("无法检查已打开的 API 配置文件", error))?;
  validate_same_file(&path_metadata, &opened_metadata)?;
  let mut contents = Vec::new();
  file
    .read_to_end(&mut contents)
    .map_err(|error| io_error("无法读取 API 配置文件", error))?;
  let registry: ApiProfileRegistry = serde_json::from_slice(&contents)
    .map_err(|_| registry_error("API 配置文件已损坏，已拒绝读取和覆盖"))?;
  validate_registry(&registry)?;
  Ok(registry)
}

fn ensure_private_registry_directory(root_path: &Path) -> AppResult<PathBuf> {
  let directory = root_path.join(REGISTRY_DIRECTORY_NAME);
  match fs::symlink_metadata(&directory) {
    Ok(metadata) => validate_private_directory(&directory, &metadata)?,
    Err(error) if error.kind() == ErrorKind::NotFound => {
      let mut builder = DirBuilder::new();
      builder.mode(PRIVATE_DIRECTORY_MODE);
      builder
        .create(&directory)
        .map_err(|error| io_error("无法创建 API 配置目录", error))?;
      fs::set_permissions(
        &directory,
        fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
      )
      .map_err(|error| io_error("无法设置 API 配置目录权限", error))?;
      let metadata = fs::symlink_metadata(&directory)
        .map_err(|error| io_error("无法检查 API 配置目录", error))?;
      validate_private_directory(&directory, &metadata)?;
    }
    Err(error) => return Err(io_error("无法检查 API 配置目录", error)),
  }
  Ok(directory)
}

fn validate_workspace_root(root_path: &Path) -> AppResult<()> {
  let metadata = fs::symlink_metadata(root_path)
    .map_err(|error| io_error("无法检查 API 配置所属工作区", error))?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(registry_error("API 配置所属工作区必须是真实目录"));
  }
  Ok(())
}

fn validate_private_directory(path: &Path, metadata: &fs::Metadata) -> AppResult<()> {
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(registry_error(format!(
      "API 配置目录不是普通目录：{}",
      path.display()
    )));
  }
  validate_mode(metadata, PRIVATE_DIRECTORY_MODE, "API 配置目录")
}

fn validate_private_file(path: &Path, metadata: &fs::Metadata) -> AppResult<()> {
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    return Err(registry_error(format!(
      "API 配置文件不是普通文件：{}",
      path.display()
    )));
  }
  validate_mode(metadata, PRIVATE_FILE_MODE, "API 配置文件")
}

fn validate_mode(metadata: &fs::Metadata, expected: u32, label: &str) -> AppResult<()> {
  let actual = metadata.permissions().mode() & PERMISSION_BITS;
  if actual != expected {
    return Err(registry_error(format!(
      "{label}权限必须为 {expected:o}，当前为 {actual:o}"
    )));
  }
  Ok(())
}

fn validate_same_file(
  path_metadata: &fs::Metadata,
  opened_metadata: &fs::Metadata,
) -> AppResult<()> {
  if path_metadata.dev() != opened_metadata.dev() || path_metadata.ino() != opened_metadata.ino() {
    return Err(registry_error("API 配置文件在读取时发生替换，已拒绝继续"));
  }
  Ok(())
}

fn io_error(context: &str, error: std::io::Error) -> crate::domain::AppError {
  registry_error(format!("{context}：{error}"))
}
