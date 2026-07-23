use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_VERSION: i64 = 14;
const MIGRATION_NAME: &str = "worker_fence_generation";
const ADD_GENERATION_SQL: &str = r#"
ALTER TABLE task_worker_lease
ADD COLUMN generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0);
"#;

pub(super) fn validate_existing_worker_fence_migration(connection: &Connection) -> AppResult<()> {
  if !table_exists(connection, "schema_migrations")? {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    return validate_marker_and_schema(connection, &name, &checksum);
  }
  if declared_schema_version(connection)?.is_some_and(|version| version >= MIGRATION_VERSION) {
    return Err(workspace_error(
      "数据库声明为 v14，但缺少任务执行器栅栏代次迁移标记",
    ));
  }
  Ok(())
}

pub(super) fn apply_worker_fence_migration(connection: &mut Connection) -> AppResult<()> {
  apply_worker_fence_migration_with_before_transaction(connection, || {})
}

fn apply_worker_fence_migration_with_before_transaction(
  connection: &mut Connection,
  before_transaction: impl FnOnce(),
) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    update_workspace_schema_version(connection, MIGRATION_VERSION)?;
    return ensure_foreign_key_integrity(connection);
  }

  before_transaction();
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if let Some((name, checksum)) = marker(&transaction)? {
    validate_marker_and_schema(&transaction, &name, &checksum)?;
    update_workspace_schema_version(&transaction, MIGRATION_VERSION)?;
    transaction.commit().map_err(database_error)?;
    return ensure_foreign_key_integrity(connection);
  }
  if column_exists(&transaction, "task_worker_lease", "generation")? {
    if !schema_is_current(&transaction)? {
      return Err(workspace_error(
        "数据库迁移 v14 前已存在不完整的任务执行器栅栏结构",
      ));
    }
  } else {
    transaction
      .execute_batch(ADD_GENERATION_SQL)
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
      "数据库迁移 v14 校验失败，任务执行器栅栏代次结构、标记或 checksum 不一致",
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
      "generation",
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
  Ok(sql.contains("generation integer not null default 0 check (generation >= 0)"))
}

fn migration_checksum() -> String {
  format!("{:x}", Sha256::digest(ADD_GENERATION_SQL.as_bytes()))
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

fn column_exists(connection: &Connection, table: &str, column: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
      params![table, column],
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
  use std::sync::{Arc, Barrier};
  use std::thread;

  use uuid::Uuid;

  use super::*;
  use crate::workspace::{
    create_workspace, open_workspace, open_workspace_database, CURRENT_SCHEMA_VERSION,
    DATABASE_FILE_NAME,
  };

  #[test]
  fn v13_workspace_upgrades_with_an_initial_worker_fence_generation() {
    let root = std::env::temp_dir().join(format!("worker-fence-v14-{}", Uuid::new_v4()));
    create_workspace("执行器栅栏升级", &root).expect("workspace should create");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute_batch(
        "DELETE FROM schema_migrations WHERE version = 14;
         UPDATE workspace SET schema_version = 13;
         ALTER TABLE task_worker_lease DROP COLUMN generation;",
      )
      .unwrap();
    drop(connection);

    let summary = open_workspace(&root).expect("v13 workspace should upgrade");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let generation: i64 = connection
      .query_row(
        "SELECT generation FROM task_worker_lease WHERE id = 'task_worker'",
        [],
        |row| row.get(0),
      )
      .unwrap_or_default();

    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(generation, 0);
    assert!(schema_is_current(&connection).unwrap());
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn concurrent_v13_upgrades_share_one_worker_fence_generation_migration() {
    let root = std::env::temp_dir().join(format!("worker-fence-v14-race-{}", Uuid::new_v4()));
    create_workspace("执行器栅栏并发升级", &root).expect("workspace should create");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute_batch(
        "DELETE FROM schema_migrations WHERE version = 14;
         UPDATE workspace SET schema_version = 13;
         ALTER TABLE task_worker_lease DROP COLUMN generation;",
      )
      .unwrap();
    drop(connection);

    let barrier = Arc::new(Barrier::new(2));
    let handles = (0..2)
      .map(|_| {
        let root = root.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
          let mut connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
          apply_worker_fence_migration_with_before_transaction(&mut connection, || {
            barrier.wait();
          })
        })
      })
      .collect::<Vec<_>>();
    let results = handles
      .into_iter()
      .map(|handle| handle.join().unwrap())
      .collect::<Vec<_>>();

    assert!(results.iter().all(Result::is_ok), "{results:?}");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let marker_count: i64 = connection
      .query_row(
        "SELECT COUNT(*) FROM schema_migrations WHERE version = 14",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert_eq!(marker_count, 1);
    assert!(schema_is_current(&connection).unwrap());
    fs::remove_dir_all(root).ok();
  }
}
