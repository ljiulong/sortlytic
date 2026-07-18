use std::fs;

use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

#[test]
fn lists_included_account_counts_from_each_tasks_latest_terminal_run() {
  let root = std::env::temp_dir().join(format!("record-counts-{}", Uuid::new_v4()));
  create_workspace("记录统计测试", &root).expect("workspace should create");
  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  let now = "2026-07-18T00:00:00Z";

  for (task_id, created_at) in [
    ("task-with-records", now),
    ("task-empty", "2026-07-17T00:00:00Z"),
  ] {
    connection
      .execute(
        "INSERT INTO collection_task (
          id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
        ) VALUES (?1, ?1, 'form', 'success', '[\"tiktok\"]', '[\"keyword_search\"]', ?2, ?2)",
        params![task_id, created_at],
      )
      .expect("task should insert");
  }

  for index in 1..=2 {
    let raw_id = format!("raw-{index}");
    connection
      .execute(
        "INSERT INTO raw_record (
          id, task_id, platform, data_type, platform_record_id, raw_file_path,
          raw_hash, collected_at, created_at
        ) VALUES (?1, 'task-with-records', 'tiktok', 'keyword_search', ?2, ?3, ?4, ?5, ?5)",
        params![
          raw_id,
          format!("platform-{index}"),
          format!("raw/tikhub/raw-{index}.json"),
          format!("hash-{index}"),
          now
        ],
      )
      .expect("raw record should insert");
    connection
      .execute(
        "INSERT INTO normalized_record (
          id, raw_record_id, task_id, platform, normalized_schema_version, created_at
        ) VALUES (?1, ?2, 'task-with-records', 'tiktok', 1, ?3)",
        params![format!("normalized-{index}"), raw_id, now],
      )
      .expect("normalized record should insert");
  }
  for (run_id, status, started_at) in [
    ("run-old", "success", "2026-07-16T00:00:00Z"),
    (
      "run-latest-terminal",
      "partial_success",
      "2026-07-17T00:00:00Z",
    ),
    ("run-current", "running", "2026-07-18T00:00:00Z"),
  ] {
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES (?1, 'task-with-records', ?2, ?3)",
        params![run_id, status, started_at],
      )
      .expect("task run should insert");
  }
  for (id, run_id, output_included) in [
    ("old-included-1", "run-old", 1),
    ("old-included-2", "run-old", 1),
    ("latest-included", "run-latest-terminal", 1),
    ("latest-filtered", "run-latest-terminal", 0),
    ("current-included", "run-current", 1),
  ] {
    connection
      .execute(
        "INSERT INTO collected_account (
          id, task_run_id, platform, identity_key, collected_at, output_included,
          created_at, updated_at
        ) VALUES (?1, ?2, 'tiktok', ?1, ?3, ?4, ?3, ?3)",
        params![id, run_id, now, output_included],
      )
      .expect("collected account should insert");
  }
  drop(connection);

  let counts = list_task_record_counts(&root).expect("record counts should list");

  assert_eq!(
    counts,
    vec![
      TaskRecordCountView {
        task_id: "task-with-records".to_string(),
        record_count: 1,
      },
      TaskRecordCountView {
        task_id: "task-empty".to_string(),
        record_count: 0,
      },
    ]
  );

  fs::remove_dir_all(root).ok();
}
