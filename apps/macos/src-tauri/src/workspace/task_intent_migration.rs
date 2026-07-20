use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_VERSION: i64 = 11;
const MIGRATION_NAME: &str = "natural_language_attempt_state";
const ADD_PARSE_PHASE_SQL: &str = "ALTER TABLE task_intent ADD COLUMN parse_phase TEXT;";
const ADD_ERROR_CODE_SQL: &str = "ALTER TABLE task_intent ADD COLUMN error_code TEXT;";
const ADD_ERROR_MESSAGE_SQL: &str = "ALTER TABLE task_intent ADD COLUMN error_message TEXT;";
const ADD_RETRYABLE_SQL: &str = "ALTER TABLE task_intent ADD COLUMN retryable INTEGER CHECK (retryable IS NULL OR retryable IN (0, 1));";
const ADD_SAFE_DETAILS_SQL: &str = "ALTER TABLE task_intent ADD COLUMN error_safe_details_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(error_safe_details_json) AND json_type(error_safe_details_json) = 'object');";
const ADD_UPDATED_AT_SQL: &str =
  "ALTER TABLE task_intent ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';";
const BACKFILL_SQL: &str = r#"
UPDATE task_intent
SET updated_at = created_at
WHERE updated_at = '';
UPDATE task_intent
SET error_code = COALESCE(
      error_code,
      (SELECT run.error_code FROM ai_run AS run WHERE run.id = task_intent.ai_run_id)
    ),
    error_message = COALESCE(
      error_message,
      (SELECT run.error_message FROM ai_run AS run WHERE run.id = task_intent.ai_run_id)
    )
WHERE ai_run_id IS NOT NULL;
"#;
const INDEX_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_task_intent_latest_attempt
ON task_intent(task_id, updated_at DESC, created_at DESC, id DESC);
"#;

const COLUMN_MIGRATIONS: &[(&str, &str)] = &[
  ("parse_phase", ADD_PARSE_PHASE_SQL),
  ("error_code", ADD_ERROR_CODE_SQL),
  ("error_message", ADD_ERROR_MESSAGE_SQL),
  ("retryable", ADD_RETRYABLE_SQL),
  ("error_safe_details_json", ADD_SAFE_DETAILS_SQL),
  ("updated_at", ADD_UPDATED_AT_SQL),
];

pub(super) fn validate_existing_task_intent_migration(connection: &Connection) -> AppResult<()> {
  if !table_exists(connection, "schema_migrations")? {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    return validate_marker_and_schema(connection, &name, &checksum);
  }
  if declared_schema_version(connection)?.is_some_and(|version| version >= MIGRATION_VERSION) {
    return Err(workspace_error(
      "数据库声明为 v11，但缺少自然语言解析状态迁移标记",
    ));
  }
  Ok(())
}

pub(super) fn apply_task_intent_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    update_workspace_schema_version(connection, MIGRATION_VERSION)?;
    return ensure_foreign_key_integrity(connection);
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let existing_columns = columns(&transaction, "task_intent")?;
  for (column, sql) in COLUMN_MIGRATIONS {
    if !existing_columns.iter().any(|existing| existing == column) {
      transaction.execute_batch(sql).map_err(database_error)?;
    }
  }
  transaction
    .execute_batch(BACKFILL_SQL)
    .map_err(database_error)?;
  transaction
    .execute_batch(INDEX_SQL)
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
      "数据库迁移 v11 校验失败，自然语言解析状态结构、标记或 checksum 不一致",
    ));
  }
  Ok(())
}

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  let columns = columns(connection, "task_intent")?;
  Ok(
    COLUMN_MIGRATIONS
      .iter()
      .all(|(column, _)| columns.iter().any(|existing| existing == column))
      && index_exists(connection, "idx_task_intent_latest_attempt")?,
  )
}

fn migration_checksum() -> String {
  let mut hasher = Sha256::new();
  for (_, sql) in COLUMN_MIGRATIONS {
    hasher.update(sql.as_bytes());
  }
  hasher.update(BACKFILL_SQL.as_bytes());
  hasher.update(INDEX_SQL.as_bytes());
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

fn columns(connection: &Connection, table: &str) -> AppResult<Vec<String>> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .map_err(database_error)?;
  let columns = statement
    .query_map([], |row| row.get(1))
    .map_err(database_error)?
    .collect::<Result<Vec<_>, _>>()
    .map_err(database_error)?;
  Ok(columns)
}

fn table_exists(connection: &Connection, table: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
      [table],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn index_exists(connection: &Connection, index: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
      [index],
      |row| row.get(0),
    )
    .map_err(database_error)
}

#[cfg(test)]
mod tests {
  use std::fs;

  use uuid::Uuid;

  use super::*;
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

  #[test]
  fn v10_upgrade_preserves_failed_intent_and_adds_latest_attempt_state() {
    let root = std::env::temp_dir().join(format!("task-intent-v11-{}", Uuid::new_v4()));
    create_workspace("自然语言迁移", &root).expect("workspace should create");
    let mut connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    connection
      .execute("DELETE FROM schema_migrations WHERE version = 11", [])
      .unwrap();
    connection
      .execute("UPDATE workspace SET schema_version = 10", [])
      .unwrap();
    connection
      .execute("DROP INDEX idx_task_intent_latest_attempt", [])
      .unwrap();
    connection
      .execute_batch(
        "DROP TABLE task_intent;
      CREATE TABLE task_intent (
        id TEXT PRIMARY KEY,
        task_id TEXT NOT NULL,
        intent_text TEXT NOT NULL,
        language TEXT,
        parse_status TEXT NOT NULL,
        ai_run_id TEXT,
        created_at TEXT NOT NULL,
        FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
      );",
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO collection_task (
        id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
      ) VALUES ('task-v10', '旧解析失败', 'natural_language', 'draft', '[]', '[]', ?1, ?1)",
        ["2026-07-20T00:00:00Z"],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO task_intent (
        id, task_id, intent_text, language, parse_status, ai_run_id, created_at
      ) VALUES ('intent-v10', 'task-v10', '查找英国账号', 'zh-CN', 'failed', NULL, ?1)",
        ["2026-07-20T00:00:01Z"],
      )
      .unwrap();

    apply_task_intent_migration(&mut connection).expect("v11 migration should succeed");
    apply_task_intent_migration(&mut connection).expect("v11 migration should be idempotent");

    let intent: (String, String, Option<String>, String) = connection
      .query_row(
        "SELECT intent_text, parse_status, error_code, updated_at
         FROM task_intent WHERE id = 'intent-v10'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
      )
      .unwrap();
    assert_eq!(
      intent,
      (
        "查找英国账号".to_string(),
        "failed".to_string(),
        None,
        "2026-07-20T00:00:01Z".to_string(),
      )
    );
    assert!(schema_is_current(&connection).unwrap());
    assert_eq!(
      connection
        .query_row("SELECT schema_version FROM workspace", [], |row| row
          .get::<_, i64>(0))
        .unwrap(),
      11
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM schema_migrations WHERE version = 11",
          [],
          |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
      1
    );

    connection
      .execute(
        "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 11",
        [],
      )
      .unwrap();
    assert!(validate_existing_task_intent_migration(&connection).is_err());

    drop(connection);
    fs::remove_dir_all(root).ok();
  }
}
