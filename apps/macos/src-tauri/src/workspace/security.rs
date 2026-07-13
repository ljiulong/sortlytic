use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OpenFlags};

use crate::domain::{AppError, AppErrorStage, AppResult};

use super::{
  database_error, workspace_error, CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME, WORKSPACE_DIRS,
};

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
  let connection = open_workspace_probe(database_path)?;
  for table in ["workspace", "schema_migrations"] {
    if !database_table_exists(&connection, table)? {
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
      "无法解析数据库登记的工作区路径 {}：{}",
      registered_root, error
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

fn open_workspace_probe(database_path: &Path) -> AppResult<Connection> {
  Connection::open_with_flags(
    database_path,
    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NOFOLLOW,
  )
  .map_err(database_error)
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
