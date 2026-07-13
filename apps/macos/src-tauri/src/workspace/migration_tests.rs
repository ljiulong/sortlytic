use std::fs;
#[cfg(unix)]
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use uuid::Uuid;

use super::*;

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

  assert_eq!(summary.schema_version, 2);
  assert_eq!(migrated_raw.0.as_deref(), Some(run_id.as_str()));
  assert_eq!(migrated_raw.1, "keyword_search");
  assert_eq!(object_count(&migrated, "table", "raw_record"), 1);
  assert_eq!(object_count(&migrated, "table", "normalized_record"), 1);
  assert_eq!(foreign_key_violation_count(&migrated), 0);

  fs::remove_dir_all(root_path).ok();
}

fn insert_task_and_runs(connection: &Connection, task_id: &str, run_ids: &[&str]) {
  let now = "2026-07-12T08:00:00+00:00";
  connection
    .execute(
      "INSERT INTO collection_task (
        id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
      ) VALUES (?1, '记录结构测试', 'form', 'running', '[\"tiktok\"]',
        '[\"keyword_search\",\"item_detail\"]', ?2, ?2)",
      params![task_id, now],
    )
    .expect("task should insert");
  for run_id in run_ids {
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES (?1, ?2, 'running', ?3)",
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
