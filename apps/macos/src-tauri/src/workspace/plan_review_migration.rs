use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_VERSION: i64 = 9;
const MIGRATION_NAME: &str = "plan_review_state_repair";

const CLEAR_INVALID_CONFIRMATIONS_SQL: &str = r#"UPDATE collection_plan
SET confirmed_by_user = 0, updated_at = ?1
WHERE validation_status = 'needs_review' AND confirmed_by_user <> 0"#;

const REPAIR_WAITING_TASKS_SQL: &str = r#"UPDATE collection_task AS task
SET status = 'draft', confirmed_at = NULL, updated_at = ?1
WHERE task.status = 'waiting_confirmation'
  AND EXISTS (
    SELECT 1
    FROM collection_plan AS latest
    WHERE latest.id = (
      SELECT candidate.id
      FROM collection_plan AS candidate
      WHERE candidate.task_id = task.id
      ORDER BY candidate.created_at DESC, candidate.id DESC
      LIMIT 1
    )
      AND latest.validation_status = 'needs_review'
  )
  AND NOT EXISTS (
    SELECT 1
    FROM collection_plan AS valid
    WHERE valid.task_id = task.id
      AND valid.validation_status = 'valid'
      AND valid.confirmed_by_user = 1
  )"#;

pub(super) fn validate_existing_plan_review_migration(connection: &Connection) -> AppResult<()> {
  if !table_exists(connection, "schema_migrations")? {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    return validate_marker(&name, &checksum);
  }
  if declared_schema_version(connection)?.is_some_and(|version| version >= MIGRATION_VERSION) {
    return Err(workspace_error(
      "数据库声明为 v9，但缺少计划复核状态修复迁移标记",
    ));
  }
  Ok(())
}

pub(super) fn apply_plan_review_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker(&name, &checksum)?;
    update_workspace_schema_version(connection, MIGRATION_VERSION)?;
    return ensure_foreign_key_integrity(connection);
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let now = Utc::now().to_rfc3339();
  let cleared_confirmations = transaction
    .execute(CLEAR_INVALID_CONFIRMATIONS_SQL, [&now])
    .map_err(database_error)?;
  let repaired_tasks = transaction
    .execute(REPAIR_WAITING_TASKS_SQL, [&now])
    .map_err(database_error)?;
  let workspace_id = transaction
    .query_row(
      "SELECT id FROM workspace ORDER BY created_at LIMIT 1",
      [],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?;
  if let Some(workspace_id) = workspace_id {
    transaction
      .execute(
        "INSERT INTO audit_log (
          id, entity_type, entity_id, action, safe_details_json, created_at
        ) VALUES (?1, 'workspace', ?2, 'migrate_plan_review_state_repair', ?3, ?4)",
        params![
          Uuid::new_v4().to_string(),
          workspace_id,
          serde_json::json!({
            "migration_version": MIGRATION_VERSION,
            "cleared_invalid_confirmations": cleared_confirmations,
            "repaired_waiting_tasks": repaired_tasks,
          })
          .to_string(),
          now,
        ],
      )
      .map_err(database_error)?;
  }
  record_migration(&transaction)?;
  transaction.commit().map_err(database_error)?;
  ensure_foreign_key_integrity(connection)
}

fn record_migration(connection: &Connection) -> AppResult<()> {
  connection
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
  update_workspace_schema_version(connection, MIGRATION_VERSION)
}

fn validate_marker(name: &str, checksum: &str) -> AppResult<()> {
  if name != MIGRATION_NAME || checksum != migration_checksum() {
    return Err(workspace_error(
      "数据库迁移 v9 校验失败，计划复核状态修复标记或 checksum 不一致",
    ));
  }
  Ok(())
}

fn migration_checksum() -> String {
  let mut hasher = Sha256::new();
  hasher.update(CLEAR_INVALID_CONFIRMATIONS_SQL.as_bytes());
  hasher.update(REPAIR_WAITING_TASKS_SQL.as_bytes());
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

fn table_exists(connection: &Connection, table: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
      [table],
      |row| row.get(0),
    )
    .map_err(database_error)
}

#[cfg(test)]
mod tests {
  use std::fs;

  use rusqlite::params;
  use uuid::Uuid;

  use super::*;
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

  #[test]
  fn repairs_waiting_task_whose_latest_plan_needs_review() {
    let root = std::env::temp_dir().join(format!("plan-review-v9-{}", Uuid::new_v4()));
    create_workspace("计划审查迁移", &root).expect("workspace should create");
    let mut connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let now = "2026-07-18T00:00:00Z";
    connection
      .execute("DELETE FROM schema_migrations WHERE version = 9", [])
      .expect("v9 marker should clear");
    connection
      .execute("UPDATE workspace SET schema_version = 8", [])
      .expect("workspace should downgrade to v8");
    connection
      .execute(
        "INSERT INTO collection_task (
          id, name, source_type, status, platforms_json, data_types_json,
          created_at, updated_at, confirmed_at
        ) VALUES ('task-review', '待复核计划', 'form', 'waiting_confirmation',
          '[\"tiktok\"]', '[\"keyword_search\"]', ?1, ?1, ?1)",
        [now],
      )
      .expect("task should insert");
    connection
      .execute(
        "INSERT INTO collection_plan (
          id, task_id, source, schema_version, plan_json, validation_status,
          validation_errors_json, confirmed_by_user, created_at, updated_at
        ) VALUES ('plan-review', 'task-review', 'form_generated', 3, '{}',
          'needs_review', '[\"范围已失效\"]', 1, ?1, ?1)",
        params![now],
      )
      .expect("plan should insert");

    apply_plan_review_migration(&mut connection).expect("v9 migration should succeed");

    let task_state: (String, Option<String>) = connection
      .query_row(
        "SELECT status, confirmed_at FROM collection_task WHERE id = 'task-review'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
      )
      .expect("task should load");
    let confirmed: i64 = connection
      .query_row(
        "SELECT confirmed_by_user FROM collection_plan WHERE id = 'plan-review'",
        [],
        |row| row.get(0),
      )
      .expect("plan should load");

    assert_eq!(task_state, ("draft".to_string(), None));
    assert_eq!(confirmed, 0);
    assert_eq!(
      connection
        .query_row("SELECT schema_version FROM workspace", [], |row| row
          .get::<_, i64>(0))
        .expect("schema version should load"),
      9
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT name FROM schema_migrations WHERE version = 9",
          [],
          |row| row.get::<_, String>(0),
        )
        .expect("migration marker should load"),
      "plan_review_state_repair"
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM audit_log
           WHERE action = 'migrate_plan_review_state_repair'",
          [],
          |row| row.get::<_, i64>(0),
        )
        .expect("migration audit should load"),
      1
    );
    connection
      .execute(
        "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 9",
        [],
      )
      .expect("migration marker should tamper");
    assert!(validate_existing_plan_review_migration(&connection).is_err());

    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn preserves_waiting_task_when_another_valid_plan_is_confirmed() {
    let root = std::env::temp_dir().join(format!("plan-review-valid-{}", Uuid::new_v4()));
    create_workspace("有效计划保护", &root).expect("workspace should create");
    let mut connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    connection
      .execute("DELETE FROM schema_migrations WHERE version = 9", [])
      .unwrap();
    connection
      .execute("UPDATE workspace SET schema_version = 8", [])
      .unwrap();
    connection
      .execute(
        "INSERT INTO collection_task (
          id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
        ) VALUES ('task-valid', '保留确认态', 'form', 'waiting_confirmation', '[]', '[]', ?1, ?1)",
        ["2026-07-17T00:00:00Z"],
      )
      .unwrap();
    for (id, status, confirmed, created_at) in [
      ("plan-valid", "valid", 1, "2026-07-17T00:00:00Z"),
      ("plan-review", "needs_review", 1, "2026-07-18T00:00:00Z"),
    ] {
      connection
        .execute(
          "INSERT INTO collection_plan (
            id, task_id, source, schema_version, plan_json, validation_status,
            confirmed_by_user, created_at, updated_at
          ) VALUES (?1, 'task-valid', 'form_generated', 3, '{}', ?2, ?3, ?4, ?4)",
          params![id, status, confirmed, created_at],
        )
        .unwrap();
    }

    apply_plan_review_migration(&mut connection).expect("v9 migration should succeed");

    assert_eq!(
      connection
        .query_row(
          "SELECT status FROM collection_task WHERE id = 'task-valid'",
          [],
          |row| row.get::<_, String>(0),
        )
        .unwrap(),
      "waiting_confirmation"
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT confirmed_by_user FROM collection_plan WHERE id = 'plan-review'",
          [],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      0
    );

    drop(connection);
    fs::remove_dir_all(root).ok();
  }
}
