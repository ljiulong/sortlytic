use std::fs::{self, DirBuilder, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};

use crate::domain::{AppError, AppErrorStage, AppResult};

use super::{
  database_error, workspace_error, CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME, WORKSPACE_DIRS,
};

const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PERMISSION_BITS: u32 = 0o7777;

#[derive(Clone, Copy)]
enum PrivatePathKind {
  Directory,
  RegularFile,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DatabaseSidecarState {
  Absent,
  Present,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DatabaseFileSnapshot {
  device: u64,
  inode: u64,
  length: u64,
  modified_at: i64,
  modified_at_nanos: i64,
}

pub(super) fn create_private_workspace_root(root_path: &Path) -> AppResult<()> {
  let mut builder = DirBuilder::new();
  builder.recursive(true).mode(PRIVATE_DIRECTORY_MODE);
  builder.create(root_path).map_err(|error| {
    workspace_error(format!(
      "无法创建工作区目录 {}：{}",
      root_path.display(),
      error
    ))
  })?;
  secure_path(
    root_path,
    PrivatePathKind::Directory,
    PRIVATE_DIRECTORY_MODE,
    "工作区根目录",
  )
}

pub(super) fn create_private_workspace_directories(root_path: &Path) -> AppResult<()> {
  for directory in WORKSPACE_DIRS {
    let mut path = root_path.to_path_buf();
    for component in directory.split('/') {
      path.push(component);
      create_private_directory(&path)?;
    }
  }
  Ok(())
}

pub(super) fn create_private_database_file(database_path: &Path) -> AppResult<()> {
  let mut options = OpenOptions::new();
  options
    .read(true)
    .write(true)
    .create_new(true)
    .mode(PRIVATE_FILE_MODE);
  let file = options.open(database_path).map_err(|error| {
    workspace_error(format!(
      "无法安全创建工作区数据库 {}：{}",
      database_path.display(),
      error
    ))
  })?;
  file
    .set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))
    .map_err(|error| permission_error(database_path, "工作区数据库", error))?;
  verify_open_file(
    &file,
    database_path,
    PrivatePathKind::RegularFile,
    PRIVATE_FILE_MODE,
    "工作区数据库",
  )?;
  validate_private_database_files(database_path)
}

fn validate_database_sidecar_entries(database_path: &Path) -> AppResult<DatabaseSidecarState> {
  let mut sidecar_count = 0;
  for sidecar in database_sidecar_paths(database_path) {
    match fs::symlink_metadata(&sidecar) {
      Ok(metadata) => {
        validate_path_kind(
          &metadata,
          &sidecar,
          PrivatePathKind::RegularFile,
          "SQLite 附属文件",
        )?;
        sidecar_count += 1;
      }
      Err(error) if error.kind() == ErrorKind::NotFound => {}
      Err(error) => return Err(permission_error(&sidecar, "SQLite 附属文件", error)),
    }
  }
  if sidecar_count == 1 {
    return Err(workspace_error(
      "SQLite WAL 与 SHM 附属文件必须成对存在，已在身份校验前拒绝打开",
    ));
  }
  Ok(if sidecar_count == 0 {
    DatabaseSidecarState::Absent
  } else {
    DatabaseSidecarState::Present
  })
}

pub(super) fn secure_existing_workspace_permissions(
  root_path: &Path,
  database_path: &Path,
) -> AppResult<()> {
  secure_path(
    root_path,
    PrivatePathKind::Directory,
    PRIVATE_DIRECTORY_MODE,
    "工作区根目录",
  )?;
  for directory in WORKSPACE_DIRS {
    let mut path = root_path.to_path_buf();
    for component in directory.split('/') {
      path.push(component);
      match fs::symlink_metadata(&path) {
        Ok(_) => secure_path(
          &path,
          PrivatePathKind::Directory,
          PRIVATE_DIRECTORY_MODE,
          "工作区子目录",
        )?,
        Err(error) if error.kind() == ErrorKind::NotFound => break,
        Err(error) => return Err(permission_error(&path, "工作区子目录", error)),
      }
    }
  }
  secure_path(
    database_path,
    PrivatePathKind::RegularFile,
    PRIVATE_FILE_MODE,
    "工作区数据库",
  )?;
  for sidecar in database_sidecar_paths(database_path) {
    match fs::symlink_metadata(&sidecar) {
      Ok(_) => secure_path(
        &sidecar,
        PrivatePathKind::RegularFile,
        PRIVATE_FILE_MODE,
        "SQLite 附属文件",
      )?,
      Err(error) if error.kind() == ErrorKind::NotFound => {}
      Err(error) => return Err(permission_error(&sidecar, "SQLite 附属文件", error)),
    }
  }
  Ok(())
}

pub(super) fn validate_private_database_files(database_path: &Path) -> AppResult<()> {
  validate_private_path(
    database_path,
    PrivatePathKind::RegularFile,
    PRIVATE_FILE_MODE,
    "工作区数据库",
  )?;
  for sidecar in database_sidecar_paths(database_path) {
    match fs::symlink_metadata(&sidecar) {
      Ok(metadata) => {
        validate_path_kind(
          &metadata,
          &sidecar,
          PrivatePathKind::RegularFile,
          "SQLite 附属文件",
        )?;
        verify_mode(&metadata, &sidecar, PRIVATE_FILE_MODE, "SQLite 附属文件")?;
      }
      Err(error) if error.kind() == ErrorKind::NotFound => {}
      Err(error) => return Err(permission_error(&sidecar, "SQLite 附属文件", error)),
    }
  }
  Ok(())
}

pub(super) fn validate_private_workspace_permissions(
  root_path: &Path,
  database_path: &Path,
) -> AppResult<()> {
  validate_private_path(
    root_path,
    PrivatePathKind::Directory,
    PRIVATE_DIRECTORY_MODE,
    "工作区根目录",
  )?;
  for directory in WORKSPACE_DIRS {
    let mut path = root_path.to_path_buf();
    for component in directory.split('/') {
      path.push(component);
      validate_private_path(
        &path,
        PrivatePathKind::Directory,
        PRIVATE_DIRECTORY_MODE,
        "工作区子目录",
      )?;
    }
  }
  validate_private_database_files(database_path)
}

fn create_private_directory(path: &Path) -> AppResult<()> {
  match fs::symlink_metadata(path) {
    Ok(_) => {}
    Err(error) if error.kind() == ErrorKind::NotFound => {
      let mut builder = DirBuilder::new();
      builder.mode(PRIVATE_DIRECTORY_MODE);
      builder.create(path).map_err(|error| {
        workspace_error(format!(
          "无法创建工作区子目录 {}：{}",
          path.display(),
          error
        ))
      })?;
    }
    Err(error) => return Err(permission_error(path, "工作区子目录", error)),
  }
  secure_path(
    path,
    PrivatePathKind::Directory,
    PRIVATE_DIRECTORY_MODE,
    "工作区子目录",
  )
}

fn secure_path(
  path: &Path,
  kind: PrivatePathKind,
  expected_mode: u32,
  label: &str,
) -> AppResult<()> {
  let path_metadata =
    fs::symlink_metadata(path).map_err(|error| permission_error(path, label, error))?;
  validate_path_kind(&path_metadata, path, kind, label)?;
  ensure_mode_can_be_tightened(&path_metadata, path, expected_mode, label)?;
  let file = OpenOptions::new()
    .read(true)
    .open(path)
    .map_err(|error| permission_error(path, label, error))?;
  let opened_metadata = file
    .metadata()
    .map_err(|error| permission_error(path, label, error))?;
  validate_path_kind(&opened_metadata, path, kind, label)?;
  if !same_file(&path_metadata, &opened_metadata) {
    return Err(workspace_error(format!(
      "{label}在权限收紧前发生替换，已拒绝继续：{}",
      path.display()
    )));
  }
  file
    .set_permissions(fs::Permissions::from_mode(expected_mode))
    .map_err(|error| permission_error(path, label, error))?;
  verify_open_file(&file, path, kind, expected_mode, label)?;
  let final_metadata =
    fs::symlink_metadata(path).map_err(|error| permission_error(path, label, error))?;
  validate_path_kind(&final_metadata, path, kind, label)?;
  if !same_file(&opened_metadata, &final_metadata) {
    return Err(workspace_error(format!(
      "{label}在权限校验期间发生替换，已拒绝继续：{}",
      path.display()
    )));
  }
  verify_mode(&final_metadata, path, expected_mode, label)
}

fn ensure_mode_can_be_tightened(
  metadata: &fs::Metadata,
  path: &Path,
  expected_mode: u32,
  label: &str,
) -> AppResult<()> {
  let actual_mode = metadata.permissions().mode() & PERMISSION_BITS;
  if actual_mode & expected_mode != expected_mode {
    return Err(workspace_error(format!(
      "{label}权限 {actual_mode:o} 缺少应用所需的所有者权限，已拒绝自动放宽：{}",
      path.display()
    )));
  }
  Ok(())
}

fn validate_private_path(
  path: &Path,
  kind: PrivatePathKind,
  expected_mode: u32,
  label: &str,
) -> AppResult<()> {
  let metadata =
    fs::symlink_metadata(path).map_err(|error| permission_error(path, label, error))?;
  validate_path_kind(&metadata, path, kind, label)?;
  verify_mode(&metadata, path, expected_mode, label)
}

fn verify_open_file(
  file: &fs::File,
  path: &Path,
  kind: PrivatePathKind,
  expected_mode: u32,
  label: &str,
) -> AppResult<()> {
  let metadata = file
    .metadata()
    .map_err(|error| permission_error(path, label, error))?;
  validate_path_kind(&metadata, path, kind, label)?;
  verify_mode(&metadata, path, expected_mode, label)
}

fn validate_path_kind(
  metadata: &fs::Metadata,
  path: &Path,
  kind: PrivatePathKind,
  label: &str,
) -> AppResult<()> {
  if metadata.file_type().is_symlink() {
    return Err(workspace_error(format!(
      "{label}不能是符号链接：{}",
      path.display()
    )));
  }
  let valid = match kind {
    PrivatePathKind::Directory => metadata.is_dir(),
    PrivatePathKind::RegularFile => metadata.is_file(),
  };
  if !valid {
    return Err(workspace_error(format!(
      "{label}类型无效：{}",
      path.display()
    )));
  }
  Ok(())
}

fn verify_mode(
  metadata: &fs::Metadata,
  path: &Path,
  expected_mode: u32,
  label: &str,
) -> AppResult<()> {
  let actual_mode = metadata.permissions().mode() & PERMISSION_BITS;
  if actual_mode != expected_mode {
    return Err(workspace_error(format!(
      "{label}权限必须为 {expected_mode:o}，实际为 {actual_mode:o}；当前文件系统可能不支持安全权限：{}",
      path.display()
    )));
  }
  Ok(())
}

fn same_file(left: &fs::Metadata, right: &fs::Metadata) -> bool {
  left.dev() == right.dev() && left.ino() == right.ino()
}

fn database_sidecar_paths(database_path: &Path) -> [PathBuf; 2] {
  ["-wal", "-shm"].map(|suffix| {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
  })
}

fn permission_error(path: &Path, label: &str, error: impl ToString) -> AppError {
  workspace_error(format!(
    "无法验证或收紧{label}权限 {}：{}",
    path.display(),
    error.to_string()
  ))
}

pub(super) fn validate_workspace_root_for_creation(root_path: &Path) -> AppResult<()> {
  match fs::symlink_metadata(root_path) {
    Ok(metadata) => validate_workspace_root_metadata(root_path, &metadata),
    Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
    Err(error) => Err(workspace_error(format!(
      "无法检查工作区根目录 {}：{}",
      root_path.display(),
      error
    ))),
  }
}

pub(super) fn canonicalize_workspace_root(root_path: &Path) -> AppResult<PathBuf> {
  let metadata = fs::symlink_metadata(root_path).map_err(|error| {
    workspace_error(format!(
      "无法检查工作区根目录 {}：{}",
      root_path.display(),
      error
    ))
  })?;
  validate_workspace_root_metadata(root_path, &metadata)?;
  fs::canonicalize(root_path).map_err(|error| {
    workspace_error(format!(
      "无法解析工作区根目录 {}：{}",
      root_path.display(),
      error
    ))
  })
}

fn validate_workspace_root_metadata(root_path: &Path, metadata: &fs::Metadata) -> AppResult<()> {
  if metadata.file_type().is_symlink() {
    return Err(workspace_error(format!(
      "工作区根目录不能是符号链接：{}",
      root_path.display()
    )));
  }
  if !metadata.is_dir() {
    return Err(workspace_error(format!(
      "工作区根目录不是目录：{}",
      root_path.display()
    )));
  }
  Ok(())
}

pub(super) fn ensure_database_path_available(database_path: &Path) -> AppResult<()> {
  match fs::symlink_metadata(database_path) {
    Ok(_) => Err(AppError::validation(
      format!(
        "工作区数据库路径已存在，不能创建或覆盖：{}",
        database_path.display()
      ),
      AppErrorStage::Workspace,
    )),
    Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
    Err(error) => Err(workspace_error(format!(
      "无法检查工作区数据库 {}：{}",
      database_path.display(),
      error
    ))),
  }
}

pub(super) fn validate_workspace_database(root_path: &Path) -> AppResult<PathBuf> {
  let database_path = root_path.join(DATABASE_FILE_NAME);
  let canonical_database = canonicalize_database_file(&database_path)?;
  if canonical_database.parent() != Some(root_path) {
    return Err(workspace_error(format!(
      "工作区数据库不在所选根目录内：{}",
      database_path.display()
    )));
  }
  Ok(canonical_database)
}

pub(super) fn canonicalize_database_file(database_path: &Path) -> AppResult<PathBuf> {
  let metadata = fs::symlink_metadata(database_path).map_err(|error| {
    workspace_error(format!(
      "无法检查工作区数据库 {}：{}",
      database_path.display(),
      error
    ))
  })?;
  if metadata.file_type().is_symlink() {
    return Err(workspace_error(format!(
      "工作区数据库不能是符号链接：{}",
      database_path.display()
    )));
  }
  if !metadata.is_file() {
    return Err(workspace_error(format!(
      "工作区数据库不是普通文件：{}",
      database_path.display()
    )));
  }
  fs::canonicalize(database_path).map_err(|error| {
    workspace_error(format!(
      "无法解析工作区数据库 {}：{}",
      database_path.display(),
      error
    ))
  })
}

pub(super) fn validate_workspace_directory_entries(root_path: &Path) -> AppResult<()> {
  for directory in WORKSPACE_DIRS {
    let mut path = root_path.to_path_buf();
    for component in directory.split('/') {
      path.push(component);
      match fs::symlink_metadata(&path) {
        Ok(_) => validate_workspace_directory(&path)?,
        Err(error) if error.kind() == ErrorKind::NotFound => break,
        Err(error) => {
          return Err(workspace_error(format!(
            "无法检查工作区子目录 {}：{}",
            path.display(),
            error
          )))
        }
      }
    }
  }
  Ok(())
}

pub(super) fn validate_workspace_directory(path: &Path) -> AppResult<()> {
  let metadata = fs::symlink_metadata(path).map_err(|error| {
    workspace_error(format!(
      "无法检查工作区子目录 {}：{}",
      path.display(),
      error
    ))
  })?;
  if metadata.file_type().is_symlink() {
    return Err(workspace_error(format!(
      "工作区子目录不能是符号链接：{}",
      path.display()
    )));
  }
  if !metadata.is_dir() {
    return Err(workspace_error(format!(
      "工作区子目录路径不是目录：{}",
      path.display()
    )));
  }
  Ok(())
}

pub(super) fn validate_workspace_identity(root_path: &Path, database_path: &Path) -> AppResult<()> {
  let sidecar_state = validate_database_sidecar_entries(database_path)?;
  let initial_snapshot = database_file_snapshot(database_path)?;
  let connection = open_workspace_probe(database_path, sidecar_state)?;
  let validation_result = validate_workspace_identity_contents(&connection, root_path);
  let stability_result =
    validate_workspace_probe_stability(database_path, sidecar_state, initial_snapshot);
  stability_result?;
  validation_result
}

fn validate_workspace_identity_contents(
  connection: &Connection,
  root_path: &Path,
) -> AppResult<()> {
  for table in ["workspace", "schema_migrations"] {
    if !database_table_exists(connection, table)? {
      return Err(workspace_error(format!(
        "所选 SQLite 不是有效工作区：缺少 {table} 表"
      )));
    }
  }

  let workspace_count = connection
    .query_row("SELECT COUNT(*) FROM workspace", [], |row| {
      row.get::<_, i64>(0)
    })
    .map_err(database_error)?;
  if workspace_count != 1 {
    return Err(workspace_error(format!(
      "工作区元数据必须恰好一条，实际为 {workspace_count} 条"
    )));
  }

  let (registered_root, schema_version) = connection
    .query_row(
      "SELECT root_path, schema_version FROM workspace",
      [],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    )
    .map_err(database_error)?;
  if !(1..=CURRENT_SCHEMA_VERSION).contains(&schema_version) {
    return Err(workspace_error(format!(
      "工作区 Schema 版本 {schema_version} 不受支持，当前最高版本为 {CURRENT_SCHEMA_VERSION}"
    )));
  }

  let initial_migration_count = connection
    .query_row(
      "SELECT COUNT(*) FROM schema_migrations WHERE version = 1",
      [],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  let invalid_migration_count = connection
    .query_row(
      "SELECT COUNT(*) FROM schema_migrations WHERE version < 1 OR version > ?1",
      params![CURRENT_SCHEMA_VERSION],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  if initial_migration_count != 1 || invalid_migration_count != 0 {
    return Err(workspace_error("工作区迁移记录无效或包含未来版本"));
  }

  let registered_root = fs::canonicalize(&registered_root).map_err(|error| {
    workspace_error(format!(
      "无法解析数据库登记的工作区路径 {registered_root}：{error}"
    ))
  })?;
  if registered_root != root_path {
    return Err(workspace_error(format!(
      "数据库登记的工作区路径与所选根目录不一致：{}",
      root_path.display()
    )));
  }

  let mut statement = connection
    .prepare("PRAGMA quick_check")
    .map_err(database_error)?;
  let results = statement
    .query_map([], |row| row.get::<_, String>(0))
    .map_err(database_error)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;
  if results.as_slice() != ["ok"] {
    return Err(workspace_error(format!(
      "工作区数据库完整性检查失败：{}",
      results.join("；")
    )));
  }
  Ok(())
}

fn open_workspace_probe(
  database_path: &Path,
  sidecar_state: DatabaseSidecarState,
) -> AppResult<Connection> {
  let flags =
    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NOFOLLOW;
  match sidecar_state {
    DatabaseSidecarState::Absent => {
      Connection::open_with_flags(immutable_database_uri(database_path), flags)
        .map_err(database_error)
    }
    DatabaseSidecarState::Present => {
      Connection::open_with_flags(database_path, flags).map_err(database_error)
    }
  }
}

fn validate_workspace_probe_stability(
  database_path: &Path,
  expected_sidecars: DatabaseSidecarState,
  initial_snapshot: DatabaseFileSnapshot,
) -> AppResult<()> {
  let current_sidecars = validate_database_sidecar_entries(database_path)?;
  let current_snapshot = database_file_snapshot(database_path)?;
  if current_sidecars != expected_sidecars {
    return Err(workspace_error(
      "身份校验期间 SQLite WAL/SHM 状态发生变化，已拒绝继续",
    ));
  }
  if current_snapshot != initial_snapshot {
    return Err(workspace_error(
      "身份校验期间工作区数据库发生变化，已拒绝继续",
    ));
  }
  Ok(())
}

fn database_file_snapshot(database_path: &Path) -> AppResult<DatabaseFileSnapshot> {
  let metadata = fs::symlink_metadata(database_path)
    .map_err(|error| permission_error(database_path, "工作区数据库", error))?;
  validate_path_kind(
    &metadata,
    database_path,
    PrivatePathKind::RegularFile,
    "工作区数据库",
  )?;
  Ok(DatabaseFileSnapshot {
    device: metadata.dev(),
    inode: metadata.ino(),
    length: metadata.len(),
    modified_at: metadata.mtime(),
    modified_at_nanos: metadata.mtime_nsec(),
  })
}

fn immutable_database_uri(database_path: &Path) -> String {
  const HEX: &[u8; 16] = b"0123456789ABCDEF";
  let bytes = database_path.as_os_str().as_bytes();
  let mut uri = String::with_capacity(bytes.len() * 3 + 32);
  uri.push_str("file:");
  for &byte in bytes {
    if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
      uri.push(char::from(byte));
    } else {
      uri.push('%');
      uri.push(char::from(HEX[(byte >> 4) as usize]));
      uri.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
  }
  uri.push_str("?mode=ro&immutable=1");
  uri
}

fn database_table_exists(connection: &Connection, table: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(
        SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1
      )",
      params![table],
      |row| row.get::<_, bool>(0),
    )
    .map_err(database_error)
}
