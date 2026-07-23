use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_VERSION: i64 = 12;
const MIGRATION_NAME: &str = "task_run_monotonic_sequence";
const ADD_RUN_SEQUENCE_SQL: &str = "ALTER TABLE task_run ADD COLUMN run_sequence INTEGER CHECK (run_sequence IS NULL OR run_sequence > 0);";
const BACKFILL_SQL: &str = r#"
UPDATE task_run AS run
SET run_sequence = (
  SELECT COUNT(*)
  FROM task_run AS candidate
  WHERE candidate.task_id = run.task_id
    AND (
      candidate.started_at < run.started_at
      OR (candidate.started_at = run.started_at AND candidate.id <= run.id)
    )
);
"#;
const INDEX_SQL: &str = r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_task_run_task_sequence
ON task_run(task_id, run_sequence);
"#;
const ASSIGN_TRIGGER_SQL: &str = r#"
CREATE TRIGGER IF NOT EXISTS trg_task_run_assign_sequence
AFTER INSERT ON task_run
FOR EACH ROW
WHEN NEW.run_sequence IS NULL
BEGIN
  UPDATE task_run
  SET run_sequence = (
    SELECT COALESCE(MAX(candidate.run_sequence), 0) + 1
    FROM task_run AS candidate
    WHERE candidate.task_id = NEW.task_id AND candidate.id <> NEW.id
  )
  WHERE id = NEW.id;
END;
"#;
const GUARD_TRIGGER_SQL: &str = r#"
CREATE TRIGGER IF NOT EXISTS trg_task_run_sequence_not_null
BEFORE UPDATE OF run_sequence ON task_run
FOR EACH ROW
WHEN OLD.run_sequence IS NOT NULL AND NEW.run_sequence IS NULL
BEGIN
  SELECT RAISE(ABORT, 'task_run.run_sequence cannot be cleared');
END;
"#;

pub(super) fn validate_existing_task_run_sequence_migration(
  connection: &Connection,
) -> AppResult<()> {
  if !table_exists(connection, "schema_migrations")? {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    return validate_marker_and_schema(connection, &name, &checksum);
  }
  if declared_schema_version(connection)?.is_some_and(|version| version >= MIGRATION_VERSION) {
    return Err(workspace_error(
      "数据库声明为 v12，但缺少任务运行单调序号迁移标记",
    ));
  }
  Ok(())
}

pub(super) fn apply_task_run_sequence_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    update_workspace_schema_version(connection, MIGRATION_VERSION)?;
    return ensure_foreign_key_integrity(connection);
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if !column_exists(&transaction, "task_run", "run_sequence")? {
    transaction
      .execute_batch(ADD_RUN_SEQUENCE_SQL)
      .map_err(database_error)?;
  }
  transaction
    .execute_batch(BACKFILL_SQL)
    .map_err(database_error)?;
  transaction
    .execute_batch(INDEX_SQL)
    .map_err(database_error)?;
  transaction
    .execute_batch(ASSIGN_TRIGGER_SQL)
    .map_err(database_error)?;
  transaction
    .execute_batch(GUARD_TRIGGER_SQL)
    .map_err(database_error)?;
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
      "数据库迁移 v12 校验失败，任务运行序号结构、标记或 checksum 不一致",
    ));
  }
  Ok(())
}

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  if !column_exists(connection, "task_run", "run_sequence")? {
    return Ok(false);
  }
  for (kind, name, expected_sql) in [
    ("index", "idx_task_run_task_sequence", INDEX_SQL),
    (
      "trigger",
      "trg_task_run_assign_sequence",
      ASSIGN_TRIGGER_SQL,
    ),
    (
      "trigger",
      "trg_task_run_sequence_not_null",
      GUARD_TRIGGER_SQL,
    ),
  ] {
    if object_sql(connection, kind, name)?.as_deref()
      != Some(normalized_schema_sql(expected_sql).as_str())
    {
      return Ok(false);
    }
  }
  let invalid_rows = connection
    .query_row(
      "SELECT COUNT(*) FROM task_run WHERE run_sequence IS NULL OR run_sequence <= 0",
      [],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  Ok(invalid_rows == 0)
}

fn migration_checksum() -> String {
  let mut hasher = Sha256::new();
  for sql in [
    ADD_RUN_SEQUENCE_SQL,
    BACKFILL_SQL,
    INDEX_SQL,
    ASSIGN_TRIGGER_SQL,
    GUARD_TRIGGER_SQL,
  ] {
    hasher.update(sql.as_bytes());
  }
  format!("{:x}", hasher.finalize())
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

fn column_exists(connection: &Connection, table: &str, column: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
      params![table, column],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn table_exists(connection: &Connection, table: &str) -> AppResult<bool> {
  object_exists(connection, "table", table)
}

fn object_exists(connection: &Connection, object_type: &str, name: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2)",
      params![object_type, name],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn object_sql(connection: &Connection, object_type: &str, name: &str) -> AppResult<Option<String>> {
  connection
    .query_row(
      "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
      params![object_type, name],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|sql| sql.map(|value| normalized_schema_sql(&value)))
    .map_err(database_error)
}

fn normalized_schema_sql(sql: &str) -> String {
  sql
    .trim()
    .trim_end_matches(';')
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
    .replacen(
      "create unique index if not exists ",
      "create unique index ",
      1,
    )
    .replacen("create trigger if not exists ", "create trigger ", 1)
}

#[cfg(test)]
mod tests {
  use std::fs;

  use rusqlite::params;
  use uuid::Uuid;

  use super::*;
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

  #[test]
  fn new_workspace_assigns_task_run_sequences_by_insertion_order() {
    let root = std::env::temp_dir().join(format!("task-run-sequence-v12-{}", Uuid::new_v4()));
    create_workspace("运行序号迁移", &root).expect("workspace should create");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    connection
      .execute(
        "INSERT INTO collection_task (
           id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
         ) VALUES ('task-sequence', '运行序号', 'form', 'failed', '[]', '[]', ?1, ?1)",
        ["2026-07-21T00:00:00Z"],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES (?1, 'task-sequence', 'failed', ?2)",
        params!["run-first", "2026-07-21T01:00:00Z"],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES (?1, 'task-sequence', 'failed', ?2)",
        params!["run-second", "2026-07-20T01:00:00Z"],
      )
      .unwrap();

    let sequences = connection
      .prepare("SELECT id, run_sequence FROM task_run ORDER BY run_sequence")
      .unwrap()
      .query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
      })
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();

    assert_eq!(
      sequences,
      vec![("run-first".to_string(), 1), ("run-second".to_string(), 2)]
    );
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn v11_upgrade_backfills_a_stable_sequence_and_records_the_marker() {
    let root = std::env::temp_dir().join(format!("task-run-sequence-upgrade-{}", Uuid::new_v4()));
    create_workspace("运行序号升级", &root).expect("workspace should create");
    let mut connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    connection
      .execute(
        "INSERT INTO collection_task (
           id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
         ) VALUES ('task-upgrade', '运行序号升级', 'form', 'failed', '[]', '[]', ?1, ?1)",
        ["2026-07-21T00:00:00Z"],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES ('run-later', 'task-upgrade', 'failed', '2026-07-21T01:00:00Z')",
        [],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES ('run-earlier', 'task-upgrade', 'failed', '2026-07-20T01:00:00Z')",
        [],
      )
      .unwrap();
    connection
      .execute_batch(
        "DROP TRIGGER trg_task_run_sequence_not_null;
         DROP TRIGGER trg_task_run_assign_sequence;
         DROP INDEX idx_task_run_task_sequence;
         DELETE FROM schema_migrations WHERE version = 12;
         UPDATE workspace SET schema_version = 11;
         ALTER TABLE task_run DROP COLUMN run_sequence;",
      )
      .unwrap();

    apply_task_run_sequence_migration(&mut connection).expect("v12 migration should succeed");

    let sequences = connection
      .prepare("SELECT id, run_sequence FROM task_run ORDER BY run_sequence")
      .unwrap()
      .query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
      })
      .unwrap()
      .collect::<Result<Vec<_>, _>>()
      .unwrap();
    assert_eq!(
      sequences,
      vec![("run-earlier".to_string(), 1), ("run-later".to_string(), 2)]
    );
    assert_eq!(
      connection
        .query_row("SELECT schema_version FROM workspace", [], |row| row
          .get::<_, i64>(0))
        .unwrap(),
      12
    );
    assert_eq!(marker(&connection).unwrap().unwrap().0, MIGRATION_NAME);
    assert!(schema_is_current(&connection).unwrap());
    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn schema_validation_rejects_a_same_name_noop_sequence_trigger() {
    let root = std::env::temp_dir().join(format!("task-run-sequence-tamper-{}", Uuid::new_v4()));
    create_workspace("运行序号结构校验", &root).expect("workspace should create");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    connection
      .execute_batch(
        "DROP TRIGGER trg_task_run_assign_sequence;
         CREATE TRIGGER trg_task_run_assign_sequence
         AFTER INSERT ON task_run
         BEGIN
           SELECT 1;
         END;",
      )
      .expect("same-name no-op trigger should install for the regression");

    assert!(
      !schema_is_current(&connection).unwrap(),
      "matching object names must not conceal a broken sequence trigger contract"
    );
    assert!(validate_existing_task_run_sequence_migration(&connection).is_err());
    drop(connection);
    fs::remove_dir_all(root).ok();
  }
}
