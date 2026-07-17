use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use active_run_migration::{apply_active_run_migration, validate_existing_active_run_migration};
use api_profile_migration::{apply_api_profile_migration, validate_existing_api_profile_migration};
use collection_pipeline_migration::{
  apply_collection_pipeline_migration, validate_existing_collection_pipeline_migration,
};
use collection_runtime_migration::{
  apply_collection_runtime_migration, validate_existing_collection_runtime_migration,
};
use run_checkpoint_migration::{
  apply_run_checkpoint_migration, validate_existing_run_checkpoint_migration,
};
use schema::{
  record_observation_migration_checksum, schema_checksum, tikhub_connector_migration_checksum,
  RECORD_OBSERVATION_INDEX_SQL, RECORD_OBSERVATION_MIGRATION_SQL, SCHEMA_SQL,
  TIKHUB_CONNECTOR_MIGRATION_SQL,
};
use security::{
  canonicalize_database_file, canonicalize_workspace_root, create_private_database_file,
  create_private_workspace_directories, create_private_workspace_root,
  ensure_database_path_available, secure_existing_workspace_permissions,
  validate_private_database_files, validate_private_workspace_permissions,
  validate_workspace_database, validate_workspace_directory_entries, validate_workspace_identity,
  validate_workspace_root_for_creation,
};

mod active_run_migration;
mod api_profile_migration;
mod collection_pipeline_migration;
mod collection_runtime_migration;
mod run_checkpoint_migration;
mod schema;
mod security;

pub const CURRENT_SCHEMA_VERSION: i64 = 8;
pub const DATABASE_FILE_NAME: &str = "app.sqlite";

const WORKSPACE_DIRS: &[&str] = &[
  "secrets",
  "raw/tikhub",
  "exports/excel",
  "exports/pdf",
  "reports",
  "prompts/snapshots",
  "logs",
  "temp",
  "backups",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceSummary {
  pub id: String,
  pub name: String,
  pub root_path: PathBuf,
  pub database_path: PathBuf,
  pub schema_version: i64,
  pub created_at: String,
  pub updated_at: String,
  pub last_opened_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceHealthCheck {
  pub workspace_id: String,
  pub database_quick_check: String,
  pub foreign_keys_enabled: bool,
  pub journal_mode: String,
  pub missing_directories: Vec<String>,
  pub database_writable: bool,
}

pub fn create_workspace(name: &str, root_path: impl AsRef<Path>) -> AppResult<WorkspaceSummary> {
  let name = normalize_workspace_name(name)?;
  let root_path = normalize_workspace_path(root_path)?;
  validate_workspace_root_for_creation(&root_path)?;
  ensure_database_path_available(&root_path.join(DATABASE_FILE_NAME))?;
  validate_workspace_directory_entries(&root_path)?;

  create_private_workspace_root(&root_path)?;

  let root_path = canonicalize_workspace_root(&root_path)?;
  let database_path = root_path.join(DATABASE_FILE_NAME);
  ensure_database_path_available(&database_path)?;
  validate_workspace_directory_entries(&root_path)?;
  create_private_workspace_directories(&root_path)?;
  create_private_database_file(&database_path)?;

  let mut connection = open_workspace_database(&database_path)?;
  apply_schema(&mut connection)?;

  let now = Utc::now().to_rfc3339();
  let workspace_id = Uuid::new_v4().to_string();

  connection
    .execute(
      "INSERT INTO workspace (
        id, name, root_path, created_at, updated_at, schema_version, last_opened_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
      params![
        workspace_id,
        name,
        root_path.to_string_lossy(),
        now,
        now,
        CURRENT_SCHEMA_VERSION,
        now
      ],
    )
    .map_err(database_error)?;

  connection
    .execute(
      "INSERT INTO audit_log (id, entity_type, entity_id, action, safe_details_json, created_at)
       VALUES (?1, 'workspace', ?2, 'create_workspace', ?3, ?4)",
      params![
        Uuid::new_v4().to_string(),
        workspace_id,
        serde_json::json!({ "name": name }).to_string(),
        now
      ],
    )
    .map_err(database_error)?;

  let summary = get_workspace_summary(&connection, &root_path, &database_path)?;
  validate_private_workspace_permissions(&root_path, &database_path)?;
  drop(connection);
  crate::api_profiles::initialize_api_profile_registry(&root_path)?;
  Ok(summary)
}

pub fn ensure_workspace(name: &str, root_path: impl AsRef<Path>) -> AppResult<WorkspaceSummary> {
  let root_path = normalize_workspace_path(root_path)?;
  let database_path = root_path.join(DATABASE_FILE_NAME);

  match fs::symlink_metadata(&database_path) {
    Ok(_) => open_workspace(root_path),
    Err(error) if error.kind() == ErrorKind::NotFound => create_workspace(name, root_path),
    Err(error) => Err(workspace_error(format!(
      "无法检查工作区数据库 {}：{}",
      database_path.display(),
      error
    ))),
  }
}

pub fn open_workspace(root_path: impl AsRef<Path>) -> AppResult<WorkspaceSummary> {
  let root_path = normalize_workspace_path(root_path)?;
  let root_path = canonicalize_workspace_root(&root_path)?;
  let database_path = validate_workspace_database(&root_path)?;
  validate_workspace_directory_entries(&root_path)?;
  validate_workspace_identity(&root_path, &database_path)?;
  secure_existing_workspace_permissions(&root_path, &database_path)?;

  let mut connection = open_workspace_database(&database_path)?;
  apply_schema(&mut connection)?;
  let mut summary = get_workspace_summary(&connection, &root_path, &database_path)?;
  create_private_workspace_directories(&root_path)?;
  let now = Utc::now().to_rfc3339();

  connection
    .execute(
      "UPDATE workspace SET last_opened_at = ?1, updated_at = ?1 WHERE id = ?2",
      params![now, summary.id],
    )
    .map_err(database_error)?;

  summary.last_opened_at = now.clone();
  summary.updated_at = now;
  validate_private_workspace_permissions(&root_path, &database_path)?;
  Ok(summary)
}

pub fn run_workspace_health_check(root_path: impl AsRef<Path>) -> AppResult<WorkspaceHealthCheck> {
  let root_path = normalize_workspace_path(root_path)?;
  let root_path = canonicalize_workspace_root(&root_path)?;
  let database_path = validate_workspace_database(&root_path)?;
  validate_workspace_directory_entries(&root_path)?;
  validate_workspace_identity(&root_path, &database_path)?;
  secure_existing_workspace_permissions(&root_path, &database_path)?;
  let connection = open_workspace_database(&database_path)?;
  let summary = get_workspace_summary(&connection, &root_path, &database_path)?;

  let database_quick_check = connection
    .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
    .map_err(database_error)?;
  let foreign_keys_enabled = pragma_i64(&connection, "PRAGMA foreign_keys")? == 1;
  let journal_mode = pragma_string(&connection, "PRAGMA journal_mode")?;
  let missing_directories = WORKSPACE_DIRS
    .iter()
    .filter(|directory| !root_path.join(directory).is_dir())
    .map(|directory| (*directory).to_string())
    .collect::<Vec<_>>();
  let database_writable = connection
    .execute(
      "INSERT INTO audit_log (id, entity_type, entity_id, action, safe_details_json, created_at)
       VALUES (?1, 'workspace', ?2, 'health_check', '{}', ?3)",
      params![
        Uuid::new_v4().to_string(),
        summary.id,
        Utc::now().to_rfc3339()
      ],
    )
    .map(|rows| rows == 1)
    .map_err(database_error)?;

  let health = WorkspaceHealthCheck {
    workspace_id: summary.id,
    database_quick_check,
    foreign_keys_enabled,
    journal_mode,
    missing_directories,
    database_writable,
  };
  validate_private_database_files(&database_path)?;
  Ok(health)
}

pub fn open_workspace_database(database_path: impl AsRef<Path>) -> AppResult<Connection> {
  let database_path = canonicalize_database_file(database_path.as_ref())?;
  validate_private_database_files(&database_path)?;
  let connection = Connection::open_with_flags(
    &database_path,
    OpenFlags::SQLITE_OPEN_READ_WRITE
      | OpenFlags::SQLITE_OPEN_URI
      | OpenFlags::SQLITE_OPEN_NOFOLLOW,
  )
  .map_err(database_error)?;
  apply_connection_pragmas(&connection)?;
  connection
    .execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
    .map_err(database_error)?;
  validate_private_database_files(&database_path)?;
  Ok(connection)
}

fn normalize_workspace_name(name: &str) -> AppResult<String> {
  let name = name.trim();

  if name.is_empty() {
    return Err(AppError::validation(
      "工作区名称不能为空",
      AppErrorStage::Workspace,
    ));
  }

  Ok(name.to_string())
}

fn normalize_workspace_path(root_path: impl AsRef<Path>) -> AppResult<PathBuf> {
  let root_path = root_path.as_ref();

  if root_path.as_os_str().is_empty() {
    return Err(AppError::validation(
      "工作区路径不能为空",
      AppErrorStage::Workspace,
    ));
  }

  Ok(root_path.to_path_buf())
}

fn apply_connection_pragmas(connection: &Connection) -> AppResult<()> {
  connection
    .execute_batch(
      "
      PRAGMA foreign_keys = ON;
      PRAGMA journal_mode = WAL;
      PRAGMA wal_autocheckpoint = 1000;
      PRAGMA synchronous = NORMAL;
      PRAGMA temp_store = MEMORY;
      ",
    )
    .map_err(database_error)
}

fn apply_schema(connection: &mut Connection) -> AppResult<()> {
  validate_existing_tikhub_connector_migration(connection)?;
  validate_existing_run_checkpoint_migration(connection)?;
  validate_existing_active_run_migration(connection)?;
  validate_existing_collection_runtime_migration(connection)?;
  validate_existing_collection_pipeline_migration(connection)?;
  validate_existing_api_profile_migration(connection)?;
  connection
    .execute_batch(SCHEMA_SQL)
    .map_err(database_error)?;

  connection
    .execute(
      "INSERT OR IGNORE INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (?1, 'initial_schema', ?2, ?3)",
      params![1, Utc::now().to_rfc3339(), schema_checksum()],
    )
    .map_err(database_error)?;

  apply_record_observation_migration(connection)?;
  apply_tikhub_connector_migration(connection)?;
  apply_run_checkpoint_migration(connection)?;
  apply_active_run_migration(connection)?;
  apply_collection_runtime_migration(connection)?;
  apply_collection_pipeline_migration(connection)?;
  apply_api_profile_migration(connection)
}

fn apply_record_observation_migration(connection: &mut Connection) -> AppResult<()> {
  let expected_checksum = record_observation_migration_checksum();
  let applied_checksum = connection
    .query_row(
      "SELECT checksum FROM schema_migrations WHERE version = 2",
      [],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?;
  let schema_is_current = table_has_column(connection, "raw_record", "task_run_id")?
    && table_has_column(connection, "raw_record", "data_type")?;

  if let Some(applied_checksum) = applied_checksum {
    if applied_checksum != expected_checksum || !schema_is_current {
      return Err(workspace_error(
        "数据库迁移 v2 校验失败，记录结构与迁移标记不一致",
      ));
    }
    connection
      .execute_batch(RECORD_OBSERVATION_INDEX_SQL)
      .map_err(database_error)?;
    update_workspace_schema_version(connection, 2)?;
    return ensure_foreign_key_integrity(connection);
  }

  if schema_is_current {
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    transaction
      .execute_batch(RECORD_OBSERVATION_INDEX_SQL)
      .map_err(database_error)?;
    record_v2_migration(&transaction, &expected_checksum)?;
    transaction.commit().map_err(database_error)?;
    return ensure_foreign_key_integrity(connection);
  }

  connection
    .execute_batch("PRAGMA foreign_keys = OFF;")
    .map_err(database_error)?;
  let migration_result = (|| -> AppResult<()> {
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    transaction
      .execute_batch(RECORD_OBSERVATION_MIGRATION_SQL)
      .map_err(database_error)?;
    transaction
      .execute_batch(RECORD_OBSERVATION_INDEX_SQL)
      .map_err(database_error)?;
    record_v2_migration(&transaction, &expected_checksum)?;
    transaction.commit().map_err(database_error)
  })();
  let restore_result = connection
    .execute_batch("PRAGMA foreign_keys = ON;")
    .map_err(database_error);

  migration_result?;
  restore_result?;
  ensure_foreign_key_integrity(connection)
}

fn record_v2_migration(connection: &Connection, checksum: &str) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (2, 'record_observations', ?1, ?2)",
      params![Utc::now().to_rfc3339(), checksum],
    )
    .map_err(database_error)?;
  update_workspace_schema_version(connection, 2)
}

fn apply_tikhub_connector_migration(connection: &mut Connection) -> AppResult<()> {
  let expected_checksum = tikhub_connector_migration_checksum();
  let applied_marker = tikhub_connector_migration_marker(connection)?;

  if let Some((name, checksum)) = applied_marker {
    validate_tikhub_connector_marker_and_schema(connection, &name, &checksum)?;
    update_workspace_schema_version(connection, 3)?;
    return ensure_foreign_key_integrity(connection);
  }

  let schema_is_current = tikhub_connector_schema_is_current(connection)?;
  if !schema_is_current {
    return Err(workspace_error(
      "数据库迁移 v3 结构校验失败，连接器表结构无效",
    ));
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  transaction
    .execute_batch(TIKHUB_CONNECTOR_MIGRATION_SQL)
    .map_err(database_error)?;
  record_v3_migration(&transaction, &expected_checksum)?;
  transaction.commit().map_err(database_error)?;

  if !tikhub_connector_schema_is_current(connection)? {
    return Err(workspace_error("数据库迁移 v3 后连接器表结构校验失败"));
  }
  ensure_foreign_key_integrity(connection)
}

fn validate_existing_tikhub_connector_migration(connection: &Connection) -> AppResult<()> {
  if table_columns(connection, "schema_migrations")?.is_empty() {
    return Ok(());
  }
  if let Some((name, checksum)) = tikhub_connector_migration_marker(connection)? {
    validate_tikhub_connector_marker_and_schema(connection, &name, &checksum)?;
  }
  Ok(())
}

fn tikhub_connector_migration_marker(
  connection: &Connection,
) -> AppResult<Option<(String, String)>> {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = 3",
      [],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(database_error)
}

fn validate_tikhub_connector_marker_and_schema(
  connection: &Connection,
  name: &str,
  checksum: &str,
) -> AppResult<()> {
  if name != "tikhub_connector" || checksum != tikhub_connector_migration_checksum() {
    return Err(workspace_error(
      "数据库迁移 v3 校验失败，连接器迁移标记或 checksum 不一致",
    ));
  }
  if !tikhub_connector_schema_is_current(connection)? {
    return Err(workspace_error(
      "数据库迁移 v3 结构校验失败，连接器表与迁移标记不一致",
    ));
  }
  Ok(())
}

fn record_v3_migration(connection: &Connection, checksum: &str) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (3, 'tikhub_connector', ?1, ?2)",
      params![Utc::now().to_rfc3339(), checksum],
    )
    .map_err(database_error)?;
  update_workspace_schema_version(connection, 3)
}

fn tikhub_connector_schema_is_current(connection: &Connection) -> AppResult<bool> {
  let columns = table_columns(connection, "tikhub_connector")?;
  let expected_columns = [
    "id",
    "workspace_id",
    "secret_ref_id",
    "base_url",
    "enabled",
    "config_version",
    "last_tested_at",
    "last_test_status",
    "created_at",
    "updated_at",
  ];
  if !columns.iter().map(String::as_str).eq(expected_columns) {
    return Ok(false);
  }

  let table_sql = connection
    .query_row(
      "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'tikhub_connector'",
      [],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?;
  let Some(table_sql) = table_sql else {
    return Ok(false);
  };
  let normalized = table_sql
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase();
  Ok(
    [
      "id text primary key not null default 'default' check (id = 'default')",
      "workspace_id text not null unique",
      "base_url text not null",
      "enabled integer not null default 1 check (enabled in (0, 1))",
      "config_version integer not null default 1 check (config_version > 0)",
      "created_at text not null",
      "updated_at text not null",
      "foreign key (workspace_id) references workspace(id) on delete cascade",
      "foreign key (secret_ref_id) references secret_ref(id) on delete set null",
    ]
    .iter()
    .all(|fragment| normalized.contains(fragment)),
  )
}

fn update_workspace_schema_version(connection: &Connection, version: i64) -> AppResult<()> {
  connection
    .execute(
      "UPDATE workspace SET schema_version = ?1 WHERE schema_version < ?1",
      params![version],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> AppResult<bool> {
  Ok(
    table_columns(connection, table)?
      .iter()
      .any(|value| value == column),
  )
}

fn table_columns(connection: &Connection, table: &str) -> AppResult<Vec<String>> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .map_err(database_error)?;
  let rows = statement
    .query_map([], |row| row.get::<_, String>(1))
    .map_err(database_error)?;
  let columns = rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;
  Ok(columns)
}

fn ensure_foreign_key_integrity(connection: &Connection) -> AppResult<()> {
  let mut statement = connection
    .prepare("PRAGMA foreign_key_check")
    .map_err(database_error)?;
  let mut rows = statement.query([]).map_err(database_error)?;
  if rows.next().map_err(database_error)?.is_some() {
    Err(workspace_error("数据库迁移后外键完整性检查失败"))
  } else {
    Ok(())
  }
}

fn get_workspace_summary(
  connection: &Connection,
  root_path: &Path,
  database_path: &Path,
) -> AppResult<WorkspaceSummary> {
  connection
    .query_row(
      "SELECT id, name, created_at, updated_at, schema_version, last_opened_at
       FROM workspace
       ORDER BY created_at
       LIMIT 1",
      [],
      |row| {
        Ok(WorkspaceSummary {
          id: row.get(0)?,
          name: row.get(1)?,
          root_path: root_path.to_path_buf(),
          database_path: database_path.to_path_buf(),
          created_at: row.get(2)?,
          updated_at: row.get(3)?,
          schema_version: row.get(4)?,
          last_opened_at: row.get(5)?,
        })
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| workspace_error("工作区元数据不存在"))
}

fn pragma_i64(connection: &Connection, statement: &str) -> AppResult<i64> {
  connection
    .query_row(statement, [], |row| row.get::<_, i64>(0))
    .map_err(database_error)
}

fn pragma_string(connection: &Connection, statement: &str) -> AppResult<String> {
  connection
    .query_row(statement, [], |row| row.get::<_, String>(0))
    .map_err(database_error)
}

fn workspace_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::WorkspaceError,
    message,
    AppErrorStage::Workspace,
    false,
  )
}

fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn create_workspace_initializes_directories_database_and_pragmas() {
    let root_path = unique_temp_workspace("create");

    let summary = create_workspace("测试工作区", &root_path).expect("workspace should be created");
    let health = run_workspace_health_check(&root_path).expect("health check should pass");

    assert_eq!(summary.name, "测试工作区");
    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(summary.database_path.is_file());
    assert_eq!(health.database_quick_check, "ok");
    assert!(health.foreign_keys_enabled);
    assert_eq!(health.journal_mode, "wal");
    assert!(health.missing_directories.is_empty());
    assert!(health.database_writable);

    for directory in WORKSPACE_DIRS {
      assert!(
        root_path.join(directory).is_dir(),
        "{directory} should exist"
      );
    }

    fs::remove_dir_all(root_path).ok();
  }

  #[test]
  fn create_workspace_rejects_existing_workspace() {
    let root_path = unique_temp_workspace("existing");

    create_workspace("测试工作区", &root_path).expect("first create should pass");
    let error = create_workspace("测试工作区", &root_path).expect_err("second create should fail");

    assert_eq!(error.code, AppErrorCode::ValidationError);
    fs::remove_dir_all(root_path).ok();
  }

  #[test]
  fn ensure_workspace_creates_once_and_reopens_afterwards() {
    let root_path = unique_temp_workspace("ensure");

    let created = ensure_workspace("默认工作区", &root_path).expect("first ensure should create");
    let reopened = ensure_workspace("默认工作区", &root_path).expect("second ensure should open");

    assert_eq!(created.id, reopened.id);
    assert_eq!(created.name, "默认工作区");
    assert!(reopened.database_path.is_file());

    fs::remove_dir_all(root_path).ok();
  }

  #[test]
  fn schema_contains_core_tables_and_indexes() {
    let root_path = unique_temp_workspace("schema");
    create_workspace("结构测试", &root_path).expect("workspace should be created");
    let connection =
      open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");

    for table in [
      "workspace",
      "secret_ref",
      "model_provider",
      "prompt_version",
      "collection_task",
      "collection_plan",
      "task_run",
      "raw_record",
      "normalized_record",
      "runtime_snapshot",
      "ai_run",
      "field_provenance",
      "report",
      "export_job",
      "webhook_job",
      "audit_log",
    ] {
      assert_eq!(
        object_count(&connection, "table", table),
        1,
        "{table} exists"
      );
    }

    for index in [
      "idx_collection_task_status",
      "idx_task_run_task_id",
      "idx_raw_record_task_id",
      "idx_ai_run_task_id",
      "idx_export_job_report_id",
    ] {
      assert_eq!(
        object_count(&connection, "index", index),
        1,
        "{index} exists"
      );
    }

    fs::remove_dir_all(root_path).ok();
  }

  fn object_count(connection: &Connection, object_type: &str, name: &str) -> i64 {
    connection
      .query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
        params![object_type, name],
        |row| row.get(0),
      )
      .expect("sqlite_master query should pass")
  }

  fn unique_temp_workspace(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
  }
}

#[cfg(test)]
#[path = "workspace/migration_tests.rs"]
mod migration_tests;

#[cfg(test)]
#[path = "workspace/collection_runtime_migration_tests.rs"]
mod collection_runtime_migration_tests;

#[cfg(test)]
#[path = "workspace/collection_pipeline_migration_tests.rs"]
mod collection_pipeline_migration_tests;

#[cfg(all(test, unix))]
#[path = "workspace/permission_tests.rs"]
mod permission_tests;
