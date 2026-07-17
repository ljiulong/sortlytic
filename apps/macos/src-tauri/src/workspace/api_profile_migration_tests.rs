use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use uuid::Uuid;

use super::*;
use crate::workspace::{
  create_workspace, open_workspace_database, CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME,
};

#[test]
fn v8_backup_precedes_cleanup_and_only_fresh_queue_snapshots_are_removed() {
  let root = v7_workspace_with_snapshots();
  let mut connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();

  apply_api_profile_migration(&mut connection).expect("v8 migration should succeed");

  assert_eq!(
    snapshot_ids(&connection),
    vec!["completed", "recovery", "running"]
  );
  let marker: (String, String) = connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = 8",
      [],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap();
  assert_eq!(marker.0, MIGRATION_NAME);
  assert_eq!(marker.1, migration_checksum());
  assert_eq!(
    connection
      .query_row("SELECT schema_version FROM workspace", [], |row| row
        .get::<_, i64>(0))
      .unwrap(),
    8
  );

  let backup_path = only_v8_backup(&root);
  let mode = fs::symlink_metadata(&backup_path)
    .unwrap()
    .permissions()
    .mode()
    & 0o7777;
  assert_eq!(mode, 0o600);
  let backup = Connection::open(&backup_path).unwrap();
  assert_eq!(
    backup
      .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
      .unwrap(),
    "ok"
  );
  assert_eq!(
    snapshot_ids(&backup),
    vec!["completed", "fresh", "recovery", "running"]
  );
  assert_eq!(
    backup
      .query_row(
        "SELECT COUNT(*) FROM schema_migrations WHERE version = 8",
        [],
        |row| row.get::<_, i64>(0),
      )
      .unwrap(),
    0
  );
  drop(backup);
  drop(connection);
  fs::remove_dir_all(root).ok();
}

#[test]
fn fresh_workspace_records_v8_without_creating_a_rollback_backup() {
  let root = std::env::temp_dir().join(format!("fresh-v8-{}", Uuid::new_v4()));
  let summary = create_workspace("新 v8 工作区", &root).unwrap();
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();

  assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
  assert_eq!(
    connection
      .query_row(
        "SELECT name FROM schema_migrations WHERE version = 8",
        [],
        |row| row.get::<_, String>(0),
      )
      .unwrap(),
    MIGRATION_NAME
  );
  assert!(v8_backups(&root).is_empty());
  drop(connection);
  fs::remove_dir_all(root).ok();
}

#[test]
fn damaged_v8_marker_is_rejected() {
  let root = std::env::temp_dir().join(format!("damaged-v8-{}", Uuid::new_v4()));
  create_workspace("损坏 v8 标记", &root).unwrap();
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  connection
    .execute(
      "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 8",
      [],
    )
    .unwrap();

  assert!(validate_existing_api_profile_migration(&connection).is_err());
  drop(connection);
  fs::remove_dir_all(root).ok();
}

fn v7_workspace_with_snapshots() -> PathBuf {
  let root = std::env::temp_dir().join(format!("migrate-v8-{}", Uuid::new_v4()));
  create_workspace("v8 迁移测试", &root).unwrap();
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  connection
    .execute("DELETE FROM schema_migrations WHERE version = 8", [])
    .unwrap();
  connection
    .execute("UPDATE workspace SET schema_version = 7", [])
    .unwrap();
  insert_connector(&connection);
  for id in ["fresh", "recovery", "running", "completed"] {
    insert_snapshot_fixture(&connection, id);
  }
  connection
    .execute(
      "UPDATE task_run SET current_stage = '恢复待发送' WHERE id = 'run-recovery'",
      [],
    )
    .unwrap();
  connection
    .execute(
      "UPDATE task_run SET status = 'running', claimed_at = ?1 WHERE id = 'run-running'",
      params!["2026-07-16T10:01:00+00:00"],
    )
    .unwrap();
  connection
    .execute(
      "UPDATE task_run SET status = 'success', claimed_at = ?1, ended_at = ?2
       WHERE id = 'run-completed'",
      params!["2026-07-16T10:01:00+00:00", "2026-07-16T10:02:00+00:00"],
    )
    .unwrap();
  drop(connection);
  root
}

fn insert_connector(connection: &Connection) {
  let workspace_id: String = connection
    .query_row("SELECT id FROM workspace", [], |row| row.get(0))
    .unwrap();
  connection
    .execute(
      "INSERT INTO secret_ref (
         id, provider_type, provider_id, secret_store_key, masked_hint,
         created_at, updated_at, credential_revision
       ) VALUES ('v8-secret', 'tikhub', 'profile-v8', 'json-ref', '[REDACTED]', ?1, ?1, 1)",
      params!["2026-07-16T10:00:00+00:00"],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO tikhub_connector (
         id, workspace_id, secret_ref_id, base_url, enabled, config_version,
         last_tested_at, last_test_status, created_at, updated_at
       ) VALUES ('default', ?1, 'v8-secret', 'https://api.tikhub.io', 1, 1,
                 ?2, 'success', ?2, ?2)",
      params![workspace_id, "2026-07-16T10:00:00+00:00"],
    )
    .unwrap();
}

fn insert_snapshot_fixture(connection: &Connection, id: &str) {
  let task_id = format!("task-{id}");
  let plan_id = format!("plan-{id}");
  let api_step_id = format!("api-step-{id}");
  let run_id = format!("run-{id}");
  let run_step_id = format!("run-step-{id}");
  let now = "2026-07-16T10:00:00+00:00";
  connection
    .execute(
      "INSERT INTO collection_task (
         id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
       ) VALUES (?1, ?2, 'form', 'queued', '[\"tiktok\"]', '[\"item_detail\"]', ?3, ?3)",
      params![task_id, format!("任务-{id}"), now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO collection_plan (
         id, task_id, source, schema_version, plan_json, validation_status,
         confirmed_by_user, created_at, updated_at
       ) VALUES (?1, ?2, 'form', 2, '{}', 'valid', 1, ?3, ?3)",
      params![plan_id, task_id, now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO api_call_step (
         id, plan_id, step_order, platform, data_type, endpoint_key, status,
         created_at, updated_at
       ) VALUES (?1, ?2, 1, 'tiktok', 'item_detail', 'item_detail', 'pending', ?3, ?3)",
      params![api_step_id, plan_id, now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO task_run (
         id, task_id, status, started_at, current_stage, plan_id, attempt_number
       ) VALUES (?1, ?2, 'queued', ?3, '等待执行', ?4, 1)",
      params![run_id, task_id, now, plan_id],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO task_run_step (
         id, task_run_id, api_call_step_id, status, created_at, updated_at
       ) VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
      params![run_step_id, run_id, api_step_id, now],
    )
    .unwrap();
  let workspace_id: String = connection
    .query_row("SELECT id FROM workspace", [], |row| row.get(0))
    .unwrap();
  connection
    .execute(
      "INSERT INTO collection_runtime_snapshot (
         id, task_run_id, workspace_id, runtime_contract_version, plan_id,
         plan_schema_version, plan_json, connector_type, connector_id,
         connector_config_version, base_url, secret_ref_id, secret_revision,
         secret_provider_type, secret_provider_id, connector_tested_at,
         connector_test_status, created_at
       ) VALUES (?1, ?2, ?3, 1, ?4, 2, '{}', 'tikhub', 'default', 1,
                 'https://api.tikhub.io', 'v8-secret', 1, 'tikhub', 'profile-v8',
                 ?5, 'success', ?5)",
      params![format!("snapshot-{id}"), run_id, workspace_id, plan_id, now],
    )
    .unwrap();
}

fn snapshot_ids(connection: &Connection) -> Vec<String> {
  let mut statement = connection
    .prepare(
      "SELECT substr(id, length('snapshot-') + 1)
       FROM collection_runtime_snapshot ORDER BY id",
    )
    .unwrap();
  statement
    .query_map([], |row| row.get(0))
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
}

fn only_v8_backup(root: &Path) -> PathBuf {
  let backups = v8_backups(root);
  assert_eq!(backups.len(), 1);
  backups.into_iter().next().unwrap()
}

fn v8_backups(root: &Path) -> Vec<PathBuf> {
  fs::read_dir(root.join("backups"))
    .unwrap()
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .filter(|path| {
      path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("app-v7-before-v8-"))
    })
    .collect()
}
