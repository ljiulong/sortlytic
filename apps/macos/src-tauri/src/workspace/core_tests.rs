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
fn opening_a_read_connection_does_not_compete_for_an_existing_write_lock() {
  let root_path = unique_temp_workspace("concurrent-read");
  let summary = create_workspace("并发读取测试", &root_path).expect("workspace should be created");
  let writer = open_workspace_database(&summary.database_path).expect("writer should open");
  writer
    .execute_batch("BEGIN IMMEDIATE;")
    .expect("writer should hold the write transaction");

  let reader = open_workspace_database(&summary.database_path)
    .expect("opening a read connection must not request a write lock");
  let workspace_count = reader
    .query_row("SELECT COUNT(*) FROM workspace", [], |row| {
      row.get::<_, i64>(0)
    })
    .expect("WAL readers should remain available during a write transaction");
  let busy_timeout = reader
    .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
    .expect("busy timeout should be readable");

  assert_eq!(workspace_count, 1);
  assert_eq!(busy_timeout, 5_000);

  writer.execute_batch("ROLLBACK;").ok();
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
