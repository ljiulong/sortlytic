use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use uuid::Uuid;

use super::*;

const TIKHUB_CONNECTOR_MIGRATION_CHECKSUM: &str =
  "c5cf7126f50164158b02c8e72c23d5acae16f05a9d76e7c6e546fab1eb7df069";

#[cfg(unix)]
#[test]
fn opening_rejects_symlinked_database_without_modifying_its_target() {
  let root_path = unique_temp_workspace("symlinked-database");
  let target_root = unique_temp_workspace("symlinked-database-target");
  fs::create_dir_all(&root_path).expect("workspace root should exist");
  fs::create_dir_all(&target_root).expect("target root should exist");
  let target_database = target_root.join("foreign.sqlite");
  let connection = Connection::open(&target_database).expect("foreign database should open");
  connection
    .execute("CREATE TABLE sentinel (value TEXT NOT NULL)", [])
    .expect("sentinel table should be created");
  drop(connection);
  symlink(&target_database, root_path.join(DATABASE_FILE_NAME))
    .expect("database symlink should be created");

  let error = open_workspace(&root_path).expect_err("database symlink must be rejected");
  let target = Connection::open(&target_database).expect("foreign database should reopen");

  assert!(error.message.contains("符号链接"));
  assert_eq!(object_count(&target, "table", "sentinel"), 1);
  assert_eq!(object_count(&target, "table", "workspace"), 0);
  assert_eq!(object_count(&target, "table", "schema_migrations"), 0);
  assert_workspace_directories_absent(&root_path);
  fs::remove_dir_all(root_path).ok();
  fs::remove_dir_all(target_root).ok();
}

#[cfg(unix)]
#[test]
fn opening_rejects_symlinked_root_even_when_it_targets_a_valid_workspace() {
  let real_root = unique_temp_workspace("real-root");
  let linked_root = unique_temp_workspace("linked-root");
  create_workspace("真实工作区", &real_root).expect("workspace should be created");
  symlink(&real_root, &linked_root).expect("root symlink should be created");

  let error = open_workspace(&linked_root).expect_err("root symlink must be rejected");

  assert!(error.message.contains("根目录") && error.message.contains("符号链接"));
  fs::remove_file(linked_root).ok();
  fs::remove_dir_all(real_root).ok();
}

#[test]
fn opening_rejects_mismatched_registered_root_before_creating_directories() {
  let registered_root = unique_temp_workspace("registered-root");
  let selected_root = unique_temp_workspace("selected-root");
  create_workspace("登记工作区", &registered_root).expect("workspace should be created");
  fs::create_dir_all(&selected_root).expect("selected root should exist");
  fs::copy(
    registered_root.join(DATABASE_FILE_NAME),
    selected_root.join(DATABASE_FILE_NAME),
  )
  .expect("workspace database should be copied");

  let error = open_workspace(&selected_root).expect_err("registered root mismatch must fail");

  assert!(error.message.contains("登记") && error.message.contains("路径"));
  assert_workspace_directories_absent(&selected_root);
  fs::remove_dir_all(registered_root).ok();
  fs::remove_dir_all(selected_root).ok();
}

#[test]
fn opening_non_workspace_sqlite_does_not_migrate_or_create_directories() {
  let root_path = unique_temp_workspace("foreign-sqlite-open");
  fs::create_dir_all(&root_path).expect("root should exist");
  let database_path = root_path.join(DATABASE_FILE_NAME);
  let connection = Connection::open(&database_path).expect("foreign database should open");
  connection
    .execute("CREATE TABLE sentinel (value TEXT NOT NULL)", [])
    .expect("sentinel table should be created");
  drop(connection);

  open_workspace(&root_path).expect_err("foreign SQLite must not be opened as a workspace");
  let connection = Connection::open(&database_path).expect("foreign database should reopen");

  assert_eq!(object_count(&connection, "table", "sentinel"), 1);
  assert_eq!(object_count(&connection, "table", "workspace"), 0);
  assert_eq!(object_count(&connection, "table", "schema_migrations"), 0);
  assert_workspace_directories_absent(&root_path);
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn creating_workspace_rejects_existing_sqlite_without_modifying_it() {
  let root_path = unique_temp_workspace("foreign-sqlite-create");
  fs::create_dir_all(&root_path).expect("root should exist");
  let database_path = root_path.join(DATABASE_FILE_NAME);
  let connection = Connection::open(&database_path).expect("foreign database should open");
  connection
    .execute("CREATE TABLE sentinel (value TEXT NOT NULL)", [])
    .expect("sentinel table should be created");
  drop(connection);

  create_workspace("不得覆盖", &root_path).expect_err("existing SQLite must be rejected");
  let connection = Connection::open(&database_path).expect("foreign database should reopen");

  assert_eq!(object_count(&connection, "table", "sentinel"), 1);
  assert_eq!(object_count(&connection, "table", "workspace"), 0);
  assert_eq!(object_count(&connection, "table", "schema_migrations"), 0);
  assert_workspace_directories_absent(&root_path);
  fs::remove_dir_all(root_path).ok();
}

#[cfg(unix)]
#[test]
fn opening_rejects_symlinked_workspace_child_before_updating_database() {
  let root_path = unique_temp_workspace("symlinked-child");
  let outside = unique_temp_workspace("symlinked-child-target");
  let created = create_workspace("子目录边界", &root_path).expect("workspace should be created");
  fs::create_dir_all(&outside).expect("outside directory should exist");
  fs::remove_dir_all(root_path.join("temp")).expect("temp directory should be removed");
  symlink(&outside, root_path.join("temp")).expect("temp symlink should be created");

  let error = open_workspace(&root_path).expect_err("child symlink must be rejected");
  let connection =
    Connection::open(root_path.join(DATABASE_FILE_NAME)).expect("workspace database should reopen");
  let last_opened_at = connection
    .query_row("SELECT last_opened_at FROM workspace", [], |row| {
      row.get::<_, String>(0)
    })
    .expect("last opened timestamp should load");

  assert!(error.message.contains("temp") && error.message.contains("符号链接"));
  assert_eq!(last_opened_at, created.last_opened_at);
  fs::remove_file(root_path.join("temp")).ok();
  fs::remove_dir_all(root_path).ok();
  fs::remove_dir_all(outside).ok();
}

#[cfg(unix)]
#[test]
fn opening_does_not_update_timestamp_when_directory_creation_fails() {
  let root_path = unique_temp_workspace("directory-create-failure");
  create_workspace("目录创建失败", &root_path).expect("workspace should be created");
  let database_path = root_path.join(DATABASE_FILE_NAME);
  let raw_path = root_path.join("raw");
  fs::remove_dir_all(raw_path.join("tikhub")).expect("nested directory should be removed");
  let connection = Connection::open(&database_path).expect("workspace database should open");
  let original_last_opened_at = "2000-01-01T00:00:00+00:00";
  connection
    .execute(
      "UPDATE workspace SET last_opened_at = ?1, updated_at = ?1",
      params![original_last_opened_at],
    )
    .expect("workspace timestamp should be fixed for the test");
  drop(connection);

  let original_permissions = fs::metadata(&raw_path)
    .expect("raw directory metadata should load")
    .permissions();
  let mut read_only_permissions = original_permissions.clone();
  read_only_permissions.set_mode(0o500);
  fs::set_permissions(&raw_path, read_only_permissions)
    .expect("raw directory should become read-only");
  let result = open_workspace(&root_path);
  fs::set_permissions(&raw_path, original_permissions)
    .expect("raw directory permissions should be restored");
  result.expect_err("missing nested directory must fail to be created");

  let connection = Connection::open(&database_path).expect("workspace database should reopen");
  let last_opened_at = connection
    .query_row("SELECT last_opened_at FROM workspace", [], |row| {
      row.get::<_, String>(0)
    })
    .expect("last opened timestamp should load");

  assert_eq!(last_opened_at, original_last_opened_at);
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn new_workspace_creates_tikhub_connector_schema_and_marker() {
  let root_path = unique_temp_workspace("tikhub-connector-schema");
  let workspace = create_workspace("连接器结构", &root_path).expect("workspace should be created");
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");

  assert_eq!(object_count(&connection, "table", "tikhub_connector"), 1);
  let columns = table_columns(&connection, "tikhub_connector");
  assert_eq!(
    columns,
    [
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
    ]
  );
  assert!(!columns.iter().any(|column| {
    let column = column.to_ascii_lowercase();
    column.contains("token") || column.contains("api_key")
  }));

  let table_sql = connection
    .query_row(
      "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = 'tikhub_connector'",
      [],
      |row| row.get::<_, String>(0),
    )
    .expect("connector table SQL should load");
  assert!(table_sql.contains("CHECK (id = 'default')"));
  assert!(table_sql.contains("workspace_id TEXT NOT NULL UNIQUE"));
  assert!(table_sql.contains("CHECK (enabled IN (0, 1))"));
  assert!(table_sql.contains("CHECK (config_version > 0)"));
  assert!(table_sql.contains("REFERENCES workspace(id)"));
  assert!(table_sql.contains("REFERENCES secret_ref(id)"));

  let (migration_name, checksum) = migration_marker(&connection, 3);
  assert_eq!(migration_name, "tikhub_connector");
  assert_eq!(checksum, TIKHUB_CONNECTOR_MIGRATION_CHECKSUM);
  assert_eq!(workspace.schema_version, CURRENT_SCHEMA_VERSION);
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn tikhub_connector_enforces_singleton_foreign_keys_and_value_constraints() {
  let root_path = unique_temp_workspace("tikhub-connector-constraints");
  let workspace = create_workspace("连接器约束", &root_path).expect("workspace should be created");
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");
  let now = "2026-07-13T08:00:00+00:00";

  connection
    .execute(
      "INSERT INTO tikhub_connector (
        workspace_id, base_url, created_at, updated_at
      ) VALUES (?1, 'https://api.tikhub.io', ?2, ?2)",
      params![workspace.id, now],
    )
    .expect("valid singleton connector should insert");
  let (id, enabled, config_version) = connection
    .query_row(
      "SELECT id, enabled, config_version FROM tikhub_connector",
      [],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, i64>(2)?,
        ))
      },
    )
    .expect("connector defaults should load");
  assert_eq!((id.as_str(), enabled, config_version), ("default", 1, 1));

  assert!(connection
    .execute(
      "INSERT INTO tikhub_connector (
        workspace_id, base_url, created_at, updated_at
      ) VALUES (?1, 'https://api.tikhub.io', ?2, ?2)",
      params![workspace.id, now],
    )
    .is_err());
  assert!(connection
    .execute("UPDATE tikhub_connector SET id = 'secondary'", [])
    .is_err());
  assert!(connection
    .execute("UPDATE tikhub_connector SET enabled = 2", [])
    .is_err());
  assert!(connection
    .execute("UPDATE tikhub_connector SET config_version = 0", [])
    .is_err());
  assert!(connection
    .execute(
      "UPDATE tikhub_connector SET workspace_id = 'missing-workspace'",
      [],
    )
    .is_err());
  assert!(connection
    .execute(
      "UPDATE tikhub_connector SET secret_ref_id = 'missing-secret'",
      [],
    )
    .is_err());

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn opening_v2_workspace_migrates_connector_schema_without_data_loss() {
  let root_path = unique_temp_workspace("tikhub-connector-v2-migration");
  create_workspace("连接器迁移", &root_path).expect("workspace should be created");
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");
  connection
    .execute(
      "INSERT INTO audit_log (
        id, entity_type, action, safe_details_json, created_at
      ) VALUES ('connector-migration-sentinel', 'test', 'preserve', '{}', ?1)",
      params!["2026-07-13T08:00:00+00:00"],
    )
    .expect("sentinel should insert");
  remove_collection_runtime_v6_fixture(&connection);
  connection
    .execute_batch(
      "DROP TABLE IF EXISTS tikhub_connector;
       DELETE FROM schema_migrations WHERE version >= 3;
       UPDATE workspace SET schema_version = 2;",
    )
    .expect("workspace should be downgraded to v2");
  drop(connection);

  let summary = open_workspace(&root_path).expect("v2 workspace should migrate");
  let migrated =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should reopen");

  assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
  assert_eq!(object_count(&migrated, "table", "tikhub_connector"), 1);
  assert_eq!(
    migrated
      .query_row(
        "SELECT COUNT(*) FROM audit_log WHERE id = 'connector-migration-sentinel'",
        [],
        |row| row.get::<_, i64>(0),
      )
      .expect("sentinel count should load"),
    1
  );
  let (migration_name, checksum) = migration_marker(&migrated, 3);
  assert_eq!(migration_name, "tikhub_connector");
  assert_eq!(checksum, TIKHUB_CONNECTOR_MIGRATION_CHECKSUM);
  assert_eq!(foreign_key_violation_count(&migrated), 0);
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn opening_rejects_invalid_tikhub_connector_marker_or_schema() {
  let checksum_root = unique_temp_workspace("tikhub-connector-bad-checksum");
  create_workspace("连接器校验", &checksum_root).expect("workspace should be created");
  let connection = Connection::open(checksum_root.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  connection
    .execute(
      "INSERT OR REPLACE INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (3, 'tikhub_connector', ?1, 'tampered')",
      params!["2026-07-13T08:00:00+00:00"],
    )
    .expect("v3 marker should be corrupted");
  connection
    .execute("UPDATE workspace SET schema_version = 3", [])
    .expect("workspace version should be set to v3");
  drop(connection);

  let checksum_error =
    open_workspace(&checksum_root).expect_err("invalid v3 checksum must be rejected");
  assert!(checksum_error.message.contains("v3") && checksum_error.message.contains("校验"));
  fs::remove_dir_all(checksum_root).ok();

  let schema_root = unique_temp_workspace("tikhub-connector-bad-schema");
  create_workspace("连接器结构校验", &schema_root).expect("workspace should be created");
  let connection =
    Connection::open(schema_root.join(DATABASE_FILE_NAME)).expect("workspace database should open");
  connection
    .execute_batch(
      "DROP TABLE IF EXISTS tikhub_connector;
       CREATE TABLE tikhub_connector (id TEXT PRIMARY KEY);",
    )
    .expect("connector table should be replaced with an invalid shape");
  drop(connection);

  let schema_error =
    open_workspace(&schema_root).expect_err("invalid marked v3 schema must be rejected");
  assert!(schema_error.message.contains("v3") && schema_error.message.contains("结构"));
  fs::remove_dir_all(schema_root).ok();

  let missing_root = unique_temp_workspace("tikhub-connector-missing-schema");
  create_workspace("连接器缺表校验", &missing_root).expect("workspace should be created");
  let connection = Connection::open(missing_root.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  connection
    .execute("DROP TABLE tikhub_connector", [])
    .expect("marked connector table should be removed for the test");
  drop(connection);

  let missing_error =
    open_workspace(&missing_root).expect_err("missing marked v3 schema must be rejected");
  assert!(missing_error.message.contains("v3") && missing_error.message.contains("结构"));
  let connection =
    Connection::open(missing_root.join(DATABASE_FILE_NAME)).expect("database should reopen");
  assert_eq!(object_count(&connection, "table", "tikhub_connector"), 0);
  fs::remove_dir_all(missing_root).ok();
}

#[test]
fn opening_rejects_damaged_v5_marker_or_active_run_index() {
  let checksum_root = unique_temp_workspace("v5-bad-checksum");
  create_workspace("v5 checksum 校验", &checksum_root).expect("workspace should be created");
  let connection = Connection::open(checksum_root.join(DATABASE_FILE_NAME))
    .expect("workspace database should open");
  connection
    .execute(
      "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 5",
      [],
    )
    .expect("v5 checksum should be corrupted");
  drop(connection);
  let checksum_error =
    open_workspace(&checksum_root).expect_err("invalid v5 checksum must be rejected");
  assert!(checksum_error.message.contains("v5") && checksum_error.message.contains("校验"));
  fs::remove_dir_all(checksum_root).ok();

  let marker_root = unique_temp_workspace("v5-missing-marker");
  create_workspace("v5 标记校验", &marker_root).expect("workspace should be created");
  let connection =
    Connection::open(marker_root.join(DATABASE_FILE_NAME)).expect("workspace database should open");
  connection
    .execute("DELETE FROM schema_migrations WHERE version = 5", [])
    .expect("v5 marker should be removed");
  drop(connection);
  let marker_error = open_workspace(&marker_root).expect_err("missing v5 marker must be rejected");
  assert!(marker_error.message.contains("v5") && marker_error.message.contains("标记"));
  fs::remove_dir_all(marker_root).ok();

  let index_root = unique_temp_workspace("v5-missing-index");
  create_workspace("v5 索引校验", &index_root).expect("workspace should be created");
  let connection =
    Connection::open(index_root.join(DATABASE_FILE_NAME)).expect("workspace database should open");
  connection
    .execute("DROP INDEX idx_task_run_single_active", [])
    .expect("v5 index should be removed");
  drop(connection);
  let index_error = open_workspace(&index_root).expect_err("missing v5 index must be rejected");
  assert!(index_error.message.contains("v5") && index_error.message.contains("结构"));
  let connection = Connection::open(index_root.join(DATABASE_FILE_NAME))
    .expect("database should reopen without repair");
  assert_eq!(
    object_count(&connection, "index", "idx_task_run_single_active"),
    0
  );
  fs::remove_dir_all(index_root).ok();
}

#[test]
fn fresh_workspace_enforces_one_active_run_per_task() {
  let root_path = unique_temp_workspace("v5-active-run-index");
  let workspace = create_workspace("活动运行唯一性", &root_path).expect("workspace should create");
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");
  let now = "2026-07-13T08:00:00+00:00";
  for task_id in ["task-active", "task-other"] {
    connection
      .execute(
        "INSERT INTO collection_task (
           id, name, source_type, status, created_at, updated_at
         ) VALUES (?1, '唯一性测试', 'form', 'queued', ?2, ?2)",
        params![task_id, now],
      )
      .expect("task should insert");
  }
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at)
       VALUES ('run-active', 'task-active', 'queued', ?1)",
      params![now],
    )
    .expect("first active run should insert");

  assert_eq!(workspace.schema_version, CURRENT_SCHEMA_VERSION);
  let (migration_name, checksum) = migration_marker(&connection, 5);
  assert_eq!(migration_name, "single_active_task_run");
  assert_eq!(
    checksum,
    "5f1a30aa477486ddabf59efac0ce858ad188b3e0e8d1bd054820756a0d101849"
  );
  assert!(connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at)
       VALUES ('run-second', 'task-active', 'running', ?1)",
      params![now],
    )
    .is_err());
  connection
    .execute(
      "UPDATE task_run SET status = 'running' WHERE id = 'run-active'",
      [],
    )
    .expect("active run should transition in place");
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, ended_at)
       VALUES ('run-history', 'task-active', 'failed', ?1, ?1)",
      params![now],
    )
    .expect("terminal history should insert");
  assert!(connection
    .execute(
      "UPDATE task_run SET status = 'queued' WHERE id = 'run-history'",
      [],
    )
    .is_err());
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at)
       VALUES ('run-other', 'task-other', 'queued', ?1)",
      params![now],
    )
    .expect("another task may have its own active run");

  drop(connection);
  fs::remove_dir_all(root_path).ok();
}

mod active_run_v5 {
  use super::*;

  const T0: &str = "2026-07-13T08:00:00+00:00";
  const T1: &str = "2026-07-13T08:01:00+00:00";
  const T2: &str = "2026-07-13T08:02:00+00:00";
  const T3: &str = "2026-07-13T08:03:00+00:00";

  #[test]
  fn v4_conflicts_fail_closed_and_preserve_uncertain_request_evidence() {
    let root = unique_temp_workspace("v5-conflict-migration");
    create_workspace("冲突迁移", &root).expect("workspace should be created");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("workspace database should open");
    downgrade_to_v4(&connection);

    insert_task(&connection, "task-conflict", "success", true);
    connection
      .execute(
        "UPDATE collection_task SET completed_at = ?1, cancelled_at = ?2
         WHERE id = 'task-conflict'",
        params![T1, T2],
      )
      .expect("historical terminal timestamps should be forged for the test");
    insert_plan(&connection, "plan-conflict", "task-conflict", true);
    insert_api_step(&connection, "api-step-conflict", "plan-conflict");
    insert_bound_run(
      &connection,
      "run-requesting",
      "task-conflict",
      "plan-conflict",
      1,
      "running",
      Some(T1),
    );
    insert_bound_run(
      &connection,
      "run-queued",
      "task-conflict",
      "plan-conflict",
      2,
      "queued",
      None,
    );
    insert_run_step(
      &connection,
      "run-step-requesting",
      "run-requesting",
      "api-step-conflict",
      "running",
    );
    insert_run_step(
      &connection,
      "run-step-queued",
      "run-queued",
      "api-step-conflict",
      "pending",
    );
    insert_requesting_checkpoint(&connection, "checkpoint-requesting", "run-step-requesting");

    insert_task(&connection, "task-single", "running", true);
    insert_unbound_run(&connection, "run-single", "task-single", "running")
      .expect("single active run should insert before migration");

    insert_task(&connection, "task-cancelled", "cancelled", true);
    connection
      .execute(
        "UPDATE collection_task SET cancelled_at = ?1 WHERE id = 'task-cancelled'",
        params![T2],
      )
      .expect("cancelled task evidence should persist");
    insert_unbound_run(&connection, "run-cancelled-a", "task-cancelled", "queued")
      .expect("first cancelled-task run should insert before migration");
    insert_unbound_run(&connection, "run-cancelled-b", "task-cancelled", "running")
      .expect("second cancelled-task run should insert before migration");

    let evidence_before = checkpoint_evidence(&connection, "checkpoint-requesting");
    drop(connection);

    let summary = open_workspace(&root).expect("v4 workspace should migrate to v5");
    let migrated = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("migrated database should reopen");

    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(active_conflict_count(&migrated), 0);
    assert_eq!(
      checkpoint_evidence(&migrated, "checkpoint-requesting"),
      evidence_before
    );
    let checkpoint_state = migrated
      .query_row(
        "SELECT status, retryable, last_error_code
         FROM collection_page_checkpoint WHERE id = 'checkpoint-requesting'",
        [],
        |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
          ))
        },
      )
      .expect("checkpoint state should load");
    assert_eq!(
      checkpoint_state,
      (
        "uncertain".to_string(),
        0,
        Some("UNCERTAIN_REQUEST_AFTER_CRASH".to_string())
      )
    );

    assert_eq!(
      run_state(&migrated, "run-requesting"),
      (
        "failed".to_string(),
        Some("请求状态不确定".to_string()),
        Some("UNCERTAIN_REQUEST_AFTER_CRASH".to_string()),
        0,
        Some(T1.to_string())
      )
    );
    assert_eq!(
      run_state(&migrated, "run-queued"),
      (
        "failed".to_string(),
        Some("活动运行冲突".to_string()),
        Some("ACTIVE_RUN_CONFLICT_MIGRATION".to_string()),
        0,
        None
      )
    );
    assert_eq!(
      run_step_state(&migrated, "run-step-requesting"),
      (
        "failed".to_string(),
        Some("uncertain_request".to_string()),
        true
      )
    );
    assert_eq!(
      run_step_state(&migrated, "run-step-queued"),
      (
        "failed".to_string(),
        Some("terminal_error".to_string()),
        true
      )
    );

    let task_state = migrated
      .query_row(
        "SELECT status, confirmed_at, completed_at, cancelled_at
         FROM collection_task WHERE id = 'task-conflict'",
        [],
        |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
          ))
        },
      )
      .expect("task state should load");
    assert_eq!(
      task_state,
      ("waiting_confirmation".to_string(), None, None, None)
    );

    let plan_state = migrated
      .query_row(
        "SELECT validation_status, confirmed_by_user
         FROM collection_plan WHERE id = 'plan-conflict'",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
      )
      .expect("plan state should load");
    assert_eq!(plan_state, ("valid".to_string(), 1));

    assert_eq!(
      migrated
        .query_row(
          "SELECT status FROM task_run WHERE id = 'run-single'",
          [],
          |row| row.get::<_, String>(0),
        )
        .expect("unrelated run state should load"),
      "running"
    );

    assert_eq!(
      migrated
        .query_row(
          "SELECT status, confirmed_at, cancelled_at
           FROM collection_task WHERE id = 'task-cancelled'",
          [],
          |row| {
            Ok((
              row.get::<_, String>(0)?,
              row.get::<_, Option<String>>(1)?,
              row.get::<_, Option<String>>(2)?,
            ))
          },
        )
        .expect("cancelled task state should load"),
      (
        "cancelled".to_string(),
        Some(T0.to_string()),
        Some(T2.to_string())
      )
    );
    assert_eq!(
      migrated
        .query_row(
          "SELECT COUNT(*) FROM task_run
           WHERE id IN ('run-cancelled-a', 'run-cancelled-b')
             AND status = 'failed' AND retryable = 0 AND ended_at IS NOT NULL",
          [],
          |row| row.get::<_, i64>(0),
        )
        .expect("cancelled task runs should load"),
      2
    );

    let audit = migrated
      .query_row(
        "SELECT json_extract(safe_details_json, '$.original_run_status'),
                json_extract(safe_details_json, '$.original_current_stage'),
                json_extract(safe_details_json, '$.original_error_code'),
                json_extract(safe_details_json, '$.original_ended_at'),
                json_extract(safe_details_json, '$.original_retryable'),
                json_extract(safe_details_json, '$.original_claimed_at'),
                json_extract(safe_details_json, '$.original_task_status'),
                json_extract(safe_details_json, '$.original_confirmed_at'),
                json_extract(safe_details_json, '$.original_completed_at'),
                json_extract(safe_details_json, '$.original_cancelled_at'),
                json_extract(safe_details_json, '$.original_task_updated_at'),
                json_extract(safe_details_json, '$.active_run_count')
         FROM task_log
         WHERE task_run_id = 'run-requesting' AND stage = '活动运行冲突迁移'",
        [],
        |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, i64>(11)?,
          ))
        },
      )
      .expect("migration audit should load");
    assert_eq!(
      audit,
      (
        "running".to_string(),
        Some("执行采集".to_string()),
        None,
        None,
        1,
        Some(T1.to_string()),
        "success".to_string(),
        Some(T0.to_string()),
        Some(T1.to_string()),
        Some(T2.to_string()),
        Some(T0.to_string()),
        2
      )
    );
    let original_error_message_was_null = migrated
      .query_row(
        "SELECT json_extract(
           safe_details_json, '$.original_error_message_was_null'
         ) FROM task_log
         WHERE task_run_id = 'run-requesting' AND stage = '活动运行冲突迁移'",
        [],
        |row| row.get::<_, Option<i64>>(0),
      )
      .expect("run error-message null marker should load");
    assert_eq!(original_error_message_was_null, Some(1));

    let step_audit = migrated
      .query_row(
        "SELECT json_extract(safe_details_json, '$.original_status'),
                json_extract(safe_details_json, '$.original_stop_reason'),
                json_extract(safe_details_json, '$.original_completed_at'),
                json_extract(safe_details_json, '$.original_updated_at')
         FROM task_log
         WHERE task_run_id = 'run-requesting' AND stage = '活动步骤冲突迁移'",
        [],
        |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
          ))
        },
      )
      .expect("run step audit should load");
    assert_eq!(
      step_audit,
      ("running".to_string(), None, None, T1.to_string())
    );

    let checkpoint_audit = migrated
      .query_row(
        "SELECT json_extract(safe_details_json, '$.original_status'),
                json_extract(safe_details_json, '$.original_retryable'),
                json_extract(safe_details_json, '$.original_updated_at'),
                json_extract(safe_details_json, '$.original_last_error_code_was_null'),
                json_extract(safe_details_json, '$.original_last_error_message_was_null')
         FROM task_log
         WHERE task_run_id = 'run-requesting' AND stage = '请求检查点冲突迁移'",
        [],
        |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<i64>>(3)?,
            row.get::<_, Option<i64>>(4)?,
          ))
        },
      )
      .expect("checkpoint audit should load");
    assert_eq!(
      checkpoint_audit,
      (
        "requesting".to_string(),
        1,
        T3.to_string(),
        Some(1),
        Some(1)
      )
    );

    assert_eq!(foreign_key_violation_count(&migrated), 0);
    let log_count_before = migration_log_count(&migrated, "task-conflict");
    assert_eq!(log_count_before, 5);
    drop(migrated);

    open_workspace(&root).expect("v5 migration should be idempotent");
    let reopened = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("workspace database should reopen");
    assert_eq!(
      migration_log_count(&reopened, "task-conflict"),
      log_count_before
    );
    assert!(insert_unbound_run(&reopened, "run-new-a", "task-conflict", "queued").is_ok());
    assert!(insert_unbound_run(&reopened, "run-new-b", "task-conflict", "running").is_err());

    drop(reopened);
    fs::remove_dir_all(root).ok();
  }

  fn downgrade_to_v4(connection: &Connection) {
    remove_collection_runtime_v6_fixture(connection);
    connection
      .execute_batch(
        "DROP INDEX idx_task_run_single_active;
         DELETE FROM schema_migrations WHERE version = 5;
         UPDATE workspace SET schema_version = 4;",
      )
      .expect("workspace should downgrade to v4 for migration testing");
  }

  fn insert_task(connection: &Connection, id: &str, status: &str, confirmed: bool) {
    connection
      .execute(
        "INSERT INTO collection_task (
           id, name, source_type, status, created_at, updated_at, confirmed_at
         ) VALUES (?1, '迁移测试任务', 'form', ?2, ?3, ?3, ?4)",
        params![id, status, T0, confirmed.then_some(T0)],
      )
      .expect("task should insert");
  }

  fn insert_plan(connection: &Connection, id: &str, task_id: &str, confirmed: bool) {
    connection
      .execute(
        "INSERT INTO collection_plan (
           id, task_id, source, schema_version, plan_json, validation_status,
           confirmed_by_user, created_at, updated_at
         ) VALUES (?1, ?2, 'form_generated', 2, '{}', 'valid', ?3, ?4, ?4)",
        params![id, task_id, i64::from(confirmed), T0],
      )
      .expect("plan should insert");
  }

  fn insert_api_step(connection: &Connection, id: &str, plan_id: &str) {
    connection
      .execute(
        "INSERT INTO api_call_step (
           id, plan_id, step_order, platform, data_type, endpoint_key, status,
           created_at, updated_at
         ) VALUES (?1, ?2, 0, 'tiktok', 'comments', 'tiktok.comments', 'planned', ?3, ?3)",
        params![id, plan_id, T0],
      )
      .expect("API step should insert");
  }

  fn insert_bound_run(
    connection: &Connection,
    id: &str,
    task_id: &str,
    plan_id: &str,
    attempt: i64,
    status: &str,
    claimed_at: Option<&str>,
  ) {
    connection
      .execute(
        "INSERT INTO task_run (
           id, task_id, plan_id, attempt_number, status, started_at,
           current_stage, claimed_at, retryable
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
        params![
          id,
          task_id,
          plan_id,
          attempt,
          status,
          T0,
          if status == "running" {
            "执行采集"
          } else {
            "等待执行"
          },
          claimed_at
        ],
      )
      .expect("bound run should insert");
  }

  fn insert_unbound_run(
    connection: &Connection,
    id: &str,
    task_id: &str,
    status: &str,
  ) -> rusqlite::Result<usize> {
    connection.execute(
      "INSERT INTO task_run (id, task_id, status, started_at)
       VALUES (?1, ?2, ?3, ?4)",
      params![id, task_id, status, T0],
    )
  }

  fn insert_run_step(
    connection: &Connection,
    id: &str,
    run_id: &str,
    api_step_id: &str,
    status: &str,
  ) {
    connection
      .execute(
        "INSERT INTO task_run_step (
           id, task_run_id, api_call_step_id, status, started_at, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?5)",
        params![id, run_id, api_step_id, status, T1],
      )
      .expect("run step should insert");
  }

  fn insert_requesting_checkpoint(connection: &Connection, id: &str, run_step_id: &str) {
    connection
      .execute(
        "INSERT INTO collection_page_checkpoint (
           id, task_run_step_id, page_index, idempotency_key, input_cursor_json,
           status, request_attempt_count, retry_count, fallback_count, final_endpoint_key,
           provider_response_json, provider_response_hash, provider_response_size,
           has_more, next_cursor_json, record_count_received, record_count_persisted,
           cost_actual_json, retryable, requested_at, response_received_at, committed_at,
           created_at, updated_at
         ) VALUES (
           ?1, ?2, 0, 'request-key-v5', '{\"cursor\":1}',
           'requesting', 3, 2, 1, 'tiktok.comments.fallback',
           '{\"data\":[1]}', 'evidence-hash', 12,
           1, '{\"cursor\":2}', 1, 1,
           '{\"currency\":\"USD\",\"amount_micros\":100}', 1, ?3, ?4, ?5, ?3, ?5
         )",
        params![id, run_step_id, T1, T2, T3],
      )
      .expect("requesting checkpoint should insert");
  }

  fn checkpoint_evidence(connection: &Connection, id: &str) -> String {
    connection
      .query_row(
        "SELECT json_array(
           input_cursor_json, request_attempt_count, retry_count, fallback_count,
           final_endpoint_key, provider_response_json, provider_response_hash,
           provider_response_size, has_more, next_cursor_json, record_count_received,
           record_count_persisted, cost_actual_json, requested_at,
           response_received_at, committed_at
         ) FROM collection_page_checkpoint WHERE id = ?1",
        params![id],
        |row| row.get(0),
      )
      .expect("checkpoint evidence should load")
  }

  fn run_state(
    connection: &Connection,
    id: &str,
  ) -> (String, Option<String>, Option<String>, i64, Option<String>) {
    connection
      .query_row(
        "SELECT status, current_stage, error_code, retryable, claimed_at
         FROM task_run WHERE id = ?1",
        params![id],
        |row| {
          Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
          ))
        },
      )
      .expect("run state should load")
  }

  fn run_step_state(connection: &Connection, id: &str) -> (String, Option<String>, bool) {
    connection
      .query_row(
        "SELECT status, stop_reason, completed_at IS NOT NULL
         FROM task_run_step WHERE id = ?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0)),
      )
      .expect("run step state should load")
  }

  fn migration_log_count(connection: &Connection, task_id: &str) -> i64 {
    connection
      .query_row(
        "SELECT COUNT(*)
         FROM task_log AS log
         JOIN task_run AS run ON run.id = log.task_run_id
         WHERE run.task_id = ?1 AND log.stage IN (
           '活动运行冲突迁移', '活动步骤冲突迁移', '请求检查点冲突迁移'
         )",
        params![task_id],
        |row| row.get(0),
      )
      .expect("migration log count should load")
  }

  fn active_conflict_count(connection: &Connection) -> i64 {
    connection
      .query_row(
        "SELECT COUNT(*) FROM (
           SELECT task_id FROM task_run
           WHERE status IN ('queued', 'running')
           GROUP BY task_id HAVING COUNT(*) > 1
         )",
        [],
        |row| row.get(0),
      )
      .expect("active run conflict count should load")
  }
}

#[test]
fn record_schema_scopes_observations_to_run_and_data_type() {
  let root_path = unique_temp_workspace("record-observation-schema");
  create_workspace("记录观察结构", &root_path).expect("workspace should be created");
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");
  let task_id = Uuid::new_v4().to_string();
  let first_run_id = Uuid::new_v4().to_string();
  let second_run_id = Uuid::new_v4().to_string();
  insert_task_and_runs(&connection, &task_id, &[&first_run_id, &second_run_id]);

  insert_raw_observation(
    &connection,
    "raw-search-run-1",
    &task_id,
    &first_run_id,
    "keyword_search",
    "shared-record",
  )
  .expect("first observation should insert");
  insert_raw_observation(
    &connection,
    "raw-detail-run-1",
    &task_id,
    &first_run_id,
    "item_detail",
    "shared-record",
  )
  .expect("a different data type must be a distinct observation");
  insert_raw_observation(
    &connection,
    "raw-search-run-2",
    &task_id,
    &second_run_id,
    "keyword_search",
    "shared-record",
  )
  .expect("a different task run must be a distinct observation");
  assert!(insert_raw_observation(
    &connection,
    "raw-search-run-1-duplicate",
    &task_id,
    &first_run_id,
    "keyword_search",
    "shared-record",
  )
  .is_err());

  insert_normalized_observation(
    &connection,
    "normalized-search-run-1",
    "raw-search-run-1",
    &task_id,
    "tiktok",
  )
  .expect("matching normalized observation should insert");
  assert!(insert_normalized_observation(
    &connection,
    "normalized-search-run-1-duplicate",
    "raw-search-run-1",
    &task_id,
    "tiktok",
  )
  .is_err());
  assert!(insert_normalized_observation(
    &connection,
    "normalized-mismatched-platform",
    "raw-detail-run-1",
    &task_id,
    "douyin",
  )
  .is_err());

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn opening_v1_workspace_migrates_record_observations_without_data_loss() {
  let root_path = unique_temp_workspace("record-observation-migration");
  create_workspace("迁移测试", &root_path).expect("workspace should be created");
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");
  let task_id = Uuid::new_v4().to_string();
  let run_id = Uuid::new_v4().to_string();
  insert_task_and_runs(&connection, &task_id, &[&run_id]);
  replace_record_tables_with_v1_schema(&connection);
  connection
    .execute(
      "INSERT INTO raw_record (
        id, task_id, platform, platform_record_id, raw_file_path, raw_hash,
        summary_json, collected_at, created_at
      ) VALUES ('legacy-raw', ?1, 'tiktok', 'legacy-record',
        'raw/tikhub/legacy.json', 'legacy-hash', ?2, ?3, ?3)",
      params![
        task_id,
        serde_json::json!({
          "data_type": "keyword_search",
          "task_run_id": run_id
        })
        .to_string(),
        "2026-07-12T08:00:00+00:00"
      ],
    )
    .expect("legacy raw row should insert");
  insert_normalized_observation(
    &connection,
    "legacy-normalized",
    "legacy-raw",
    &task_id,
    "tiktok",
  )
  .expect("legacy normalized row should insert");
  remove_collection_runtime_v6_fixture(&connection);
  connection
    .execute("DELETE FROM schema_migrations WHERE version > 1", [])
    .expect("future migration markers should clear");
  connection
    .execute("UPDATE workspace SET schema_version = 1", [])
    .expect("workspace version should reset");
  drop(connection);

  let summary = open_workspace(&root_path).expect("v1 workspace should migrate");
  let migrated =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should reopen");
  let migrated_raw = migrated
    .query_row(
      "SELECT task_run_id, data_type FROM raw_record WHERE id = 'legacy-raw'",
      [],
      |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
    )
    .expect("legacy raw row should survive migration");

  assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
  assert_eq!(migrated_raw.0.as_deref(), Some(run_id.as_str()));
  assert_eq!(migrated_raw.1, "keyword_search");
  assert_eq!(object_count(&migrated, "table", "raw_record"), 1);
  assert_eq!(object_count(&migrated, "table", "normalized_record"), 1);
  assert_eq!(object_count(&migrated, "table", "tikhub_connector"), 1);
  assert_eq!(migration_marker(&migrated, 3).0, "tikhub_connector");
  assert_eq!(foreign_key_violation_count(&migrated), 0);

  fs::remove_dir_all(root_path).ok();
}

fn insert_task_and_runs(connection: &Connection, task_id: &str, run_ids: &[&str]) {
  let now = "2026-07-12T08:00:00+00:00";
  connection
    .execute(
      "INSERT INTO collection_task (
        id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
      ) VALUES (?1, '记录结构测试', 'form', 'success', '[\"tiktok\"]',
        '[\"keyword_search\",\"item_detail\"]', ?2, ?2)",
      params![task_id, now],
    )
    .expect("task should insert");
  for run_id in run_ids {
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at, ended_at)
         VALUES (?1, ?2, 'success', ?3, ?3)",
        params![run_id, task_id, now],
      )
      .expect("task run should insert");
  }
}

fn insert_raw_observation(
  connection: &Connection,
  id: &str,
  task_id: &str,
  task_run_id: &str,
  data_type: &str,
  platform_record_id: &str,
) -> rusqlite::Result<usize> {
  connection.execute(
    "INSERT INTO raw_record (
      id, task_id, task_run_id, platform, data_type, platform_record_id,
      raw_file_path, raw_hash, collected_at, created_at
    ) VALUES (?1, ?2, ?3, 'tiktok', ?4, ?5, ?6, ?7, ?8, ?8)",
    params![
      id,
      task_id,
      task_run_id,
      data_type,
      platform_record_id,
      format!("raw/tikhub/{id}.json"),
      format!("hash-{id}"),
      "2026-07-12T08:00:00+00:00"
    ],
  )
}

fn insert_normalized_observation(
  connection: &Connection,
  id: &str,
  raw_record_id: &str,
  task_id: &str,
  platform: &str,
) -> rusqlite::Result<usize> {
  connection.execute(
    "INSERT INTO normalized_record (
      id, raw_record_id, task_id, platform, normalized_schema_version, created_at
    ) VALUES (?1, ?2, ?3, ?4, 1, ?5)",
    params![
      id,
      raw_record_id,
      task_id,
      platform,
      "2026-07-12T08:00:00+00:00"
    ],
  )
}

fn replace_record_tables_with_v1_schema(connection: &Connection) {
  connection
    .execute_batch(
      "PRAGMA foreign_keys = OFF;
       DROP TABLE normalized_record;
       DROP TABLE raw_record;
       CREATE TABLE raw_record (
         id TEXT PRIMARY KEY,
         task_id TEXT NOT NULL,
         platform TEXT NOT NULL,
         platform_record_id TEXT NOT NULL,
         raw_url TEXT,
         raw_file_path TEXT NOT NULL,
         raw_hash TEXT NOT NULL,
         summary_json TEXT NOT NULL DEFAULT '{}',
         collected_at TEXT NOT NULL,
         created_at TEXT NOT NULL,
         UNIQUE (platform, platform_record_id, task_id),
         FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
       );
       CREATE TABLE normalized_record (
         id TEXT PRIMARY KEY,
         raw_record_id TEXT NOT NULL,
         task_id TEXT NOT NULL,
         platform TEXT NOT NULL,
         author_id TEXT,
         author_name TEXT,
         content_text TEXT,
         content_url TEXT,
         published_at TEXT,
         region TEXT,
         metrics_json TEXT NOT NULL DEFAULT '{}',
         tags_json TEXT NOT NULL DEFAULT '[]',
         normalized_schema_version INTEGER NOT NULL,
         created_at TEXT NOT NULL,
         FOREIGN KEY (raw_record_id) REFERENCES raw_record(id) ON DELETE CASCADE,
         FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
       );
       PRAGMA foreign_keys = ON;",
    )
    .expect("v1 record tables should be restored");
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

fn table_columns(connection: &Connection, table: &str) -> Vec<String> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .expect("table info should prepare");
  statement
    .query_map([], |row| row.get::<_, String>(1))
    .expect("table info should run")
    .collect::<rusqlite::Result<Vec<_>>>()
    .expect("table columns should load")
}

fn remove_collection_runtime_v6_fixture(connection: &Connection) {
  if table_columns(connection, "secret_ref")
    .iter()
    .any(|column| column == "credential_revision")
  {
    connection
      .execute_batch(
        "DROP TRIGGER IF EXISTS trg_collection_runtime_snapshot_immutable_delete;
         DROP TRIGGER IF EXISTS trg_collection_runtime_snapshot_immutable_update;
         DROP TRIGGER IF EXISTS trg_collection_runtime_snapshot_insert;
         DROP TRIGGER IF EXISTS trg_secret_ref_credential_invalidates_connector;
         DROP TRIGGER IF EXISTS trg_secret_ref_credential_revision;
         DROP TRIGGER IF EXISTS trg_secret_ref_credential_revision_overflow;
         DROP INDEX IF EXISTS idx_collection_runtime_snapshot_task_run_id;
         DROP TABLE IF EXISTS collection_runtime_snapshot;
         ALTER TABLE secret_ref DROP COLUMN credential_revision;
         DELETE FROM schema_migrations WHERE version = 6;",
      )
      .expect("v6 runtime fixture should be removed");
  }
}

fn migration_marker(connection: &Connection, version: i64) -> (String, String) {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = ?1",
      params![version],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("migration marker should exist")
}

fn foreign_key_violation_count(connection: &Connection) -> usize {
  let mut statement = connection
    .prepare("PRAGMA foreign_key_check")
    .expect("foreign key check should prepare");
  let rows = statement
    .query_map([], |_| Ok(()))
    .expect("foreign key check should run");
  rows.count()
}

fn assert_workspace_directories_absent(root_path: &Path) {
  for directory in WORKSPACE_DIRS {
    assert!(
      fs::symlink_metadata(root_path.join(directory)).is_err(),
      "{directory} must not be created before workspace validation"
    );
  }
}

fn unique_temp_workspace(label: &str) -> PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
