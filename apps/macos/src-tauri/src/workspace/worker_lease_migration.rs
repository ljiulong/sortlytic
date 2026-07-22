use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_VERSION: i64 = 13;
const MIGRATION_NAME: &str = "authoritative_worker_lease";
const WORKER_LEASE_SQL: &str = r#"
CREATE TABLE task_worker_lease (
  id TEXT PRIMARY KEY NOT NULL DEFAULT 'task_worker' CHECK (id = 'task_worker'),
  owner_id TEXT NOT NULL CHECK (length(owner_id) BETWEEN 1 AND 128),
  lease_expires_at INTEGER NOT NULL CHECK (lease_expires_at >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
"#;

pub(super) fn validate_existing_worker_lease_migration(connection: &Connection) -> AppResult<()> {
  if !table_exists(connection, "schema_migrations")? {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    return validate_marker_and_schema(connection, &name, &checksum);
  }
  if declared_schema_version(connection)?.is_some_and(|version| version >= MIGRATION_VERSION) {
    return Err(workspace_error(
      "数据库声明为 v13，但缺少任务执行器权威租约迁移标记",
    ));
  }
  Ok(())
}

pub(super) fn apply_worker_lease_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    update_workspace_schema_version(connection, MIGRATION_VERSION)?;
    return ensure_foreign_key_integrity(connection);
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if table_exists(&transaction, "task_worker_lease")? {
    if !schema_is_current(&transaction)? {
      return Err(workspace_error(
        "数据库迁移 v13 前已存在不完整的任务执行器租约结构",
      ));
    }
  } else {
    transaction
      .execute_batch(WORKER_LEASE_SQL)
      .map_err(database_error)?;
  }
  transaction
    .execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (?1, ?2, ?3, ?4)",
      params![
        MIGRATION_VERSION,
        MIGRATION_NAME,
        Utc::now().to_rfc3339(),
        migration_checksum(),
      ],
    )
    .map_err(database_error)?;
  update_workspace_schema_version(&transaction, MIGRATION_VERSION)?;
  transaction.commit().map_err(database_error)?;

  validate_marker_and_schema(connection, MIGRATION_NAME, &migration_checksum())?;
  ensure_foreign_key_integrity(connection)
}

fn validate_marker_and_schema(
  connection: &Connection,
  name: &str,
  checksum: &str,
) -> AppResult<()> {
  if name != MIGRATION_NAME || checksum != migration_checksum() || !schema_is_current(connection)? {
    return Err(workspace_error(
      "数据库迁移 v13 校验失败，任务执行器租约结构、标记或 checksum 不一致",
    ));
  }
  Ok(())
}

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  let columns = table_columns(connection, "task_worker_lease")?;
  if columns
    != [
      "id",
      "owner_id",
      "lease_expires_at",
      "created_at",
      "updated_at",
    ]
  {
    return Ok(false);
  }
  let sql = connection
    .query_row(
      "SELECT lower(sql) FROM sqlite_schema WHERE type = 'table' AND name = 'task_worker_lease'",
      [],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?
    .unwrap_or_default()
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
  Ok(
    [
      "id text primary key not null default 'task_worker' check (id = 'task_worker')",
      "owner_id text not null check (length(owner_id) between 1 and 128)",
      "lease_expires_at integer not null check (lease_expires_at >= 0)",
      "created_at text not null",
      "updated_at text not null",
    ]
    .iter()
    .all(|fragment| sql.contains(fragment)),
  )
}

fn migration_checksum() -> String {
  format!("{:x}", Sha256::digest(WORKER_LEASE_SQL.as_bytes()))
}

fn marker(connection: &Connection) -> AppResult<Option<(String, String)>> {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = ?1",
      [MIGRATION_VERSION],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(database_error)
}

fn declared_schema_version(connection: &Connection) -> AppResult<Option<i64>> {
  connection
    .query_row("SELECT MAX(schema_version) FROM workspace", [], |row| {
      row.get(0)
    })
    .map_err(database_error)
}

fn table_exists(connection: &Connection, table: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
      [table],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn table_columns(connection: &Connection, table: &str) -> AppResult<Vec<String>> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .map_err(database_error)?;
  let columns = statement
    .query_map([], |row| row.get(1))
    .map_err(database_error)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;
  Ok(columns)
}

#[cfg(test)]
mod tests {
  use std::fs;

  use uuid::Uuid;

  use super::*;
  use crate::workspace::{
    create_workspace, open_workspace, open_workspace_database, CURRENT_SCHEMA_VERSION,
    DATABASE_FILE_NAME,
  };

  #[test]
  fn new_workspace_creates_authoritative_worker_lease_schema() {
    let root = temp_root("fresh");
    let summary = create_workspace("执行器租约", &root).expect("workspace should create");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();

    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    assert!(schema_is_current(&connection).unwrap());
    assert_eq!(marker(&connection).unwrap().unwrap().0, MIGRATION_NAME);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn v12_workspace_upgrades_without_touching_existing_tasks() {
    let root = temp_root("upgrade");
    create_workspace("执行器租约升级", &root).expect("workspace should create");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute(
        "INSERT INTO collection_task (
           id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
         ) VALUES ('preserved-task', '保留任务', 'form', 'draft', '[]', '[]', ?1, ?1)",
        ["2026-07-21T00:00:00Z"],
      )
      .unwrap();
    connection
      .execute_batch(
        "DROP TABLE task_worker_lease;
         DELETE FROM schema_migrations WHERE version = 13;
         UPDATE workspace SET schema_version = 12;",
      )
      .unwrap();
    drop(connection);

    let summary = open_workspace(&root).expect("v12 workspace should upgrade");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let task_count = connection
      .query_row(
        "SELECT COUNT(*) FROM collection_task WHERE id = 'preserved-task'",
        [],
        |row| row.get::<_, i64>(0),
      )
      .unwrap();

    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(task_count, 1);
    assert!(schema_is_current(&connection).unwrap());
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn registered_worker_lease_schema_rejects_unregistered_columns() {
    let root = temp_root("tamper");
    create_workspace("执行器租约篡改", &root).expect("workspace should create");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute(
        "ALTER TABLE task_worker_lease ADD COLUMN unexpected TEXT",
        [],
      )
      .unwrap();
    drop(connection);

    let error = open_workspace(&root).expect_err("tampered schema must be rejected");

    assert!(error.message.contains("迁移 v13") && error.message.contains("校验失败"));
    fs::remove_dir_all(root).ok();
  }

  fn temp_root(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("worker-lease-v13-{label}-{}", Uuid::new_v4()))
  }
}
