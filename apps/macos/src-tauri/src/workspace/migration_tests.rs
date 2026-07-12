use std::fs;
use std::path::PathBuf;

use rusqlite::params;
use uuid::Uuid;

use super::*;

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

fn unique_temp_workspace(label: &str) -> PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
