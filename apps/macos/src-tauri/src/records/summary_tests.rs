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
    ("run-old", "success", "2026-07-17T00:00:00Z"),
    (
      "run-latest-terminal",
      "partial_success",
      "2026-07-16T00:00:00Z",
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

#[test]
fn lists_paginated_results_from_the_latest_successful_run_only() {
  let root = std::env::temp_dir().join(format!("task-results-{}", Uuid::new_v4()));
  create_workspace("任务结果测试", &root).expect("workspace should create");
  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  let now = "2026-07-19T00:00:00Z";

  for task_id in ["task-results", "other-task"] {
    connection
      .execute(
        "INSERT INTO collection_task (
          id, name, source_type, status, platforms_json, data_types_json,
          selected_fields_json, created_at, updated_at
        ) VALUES (?1, ?1, 'form', 'partial_success', '[\"tiktok\"]',
          '[\"account\"]', '[\"bio\",\"followers_count\",\"country_region\"]', ?2, ?2)",
        params![task_id, now],
      )
      .expect("task should insert");
  }
  for (run_id, task_id, status, started_at) in [
    ("run-old", "task-results", "success", "2026-07-18T00:00:00Z"),
    (
      "run-latest",
      "task-results",
      "partial_success",
      "2026-07-17T00:00:00Z",
    ),
    ("run-other", "other-task", "success", "2026-07-19T00:00:00Z"),
  ] {
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![run_id, task_id, status, started_at],
      )
      .expect("task run should insert");
  }
  connection
    .execute(
      "INSERT INTO collection_plan (
        id, task_id, source, schema_version, plan_json, validation_status,
        confirmed_by_user, created_at, updated_at
      ) VALUES ('plan-latest', 'task-results', 'form_generated', 3,
        '{\"age_range\":{\"min\":18,\"max\":35},\"gender_filter\":null}',
        'valid', 1, ?1, ?1)",
      params![now],
    )
    .expect("collection plan should insert");
  connection
    .execute(
      "UPDATE task_run SET plan_id = 'plan-latest' WHERE id = 'run-latest'",
      [],
    )
    .expect("latest run should reference its plan");
  for (id, run_id, username, output_included, created_at) in [
    ("old", "run-old", "旧运行", 1, "2026-07-17T01:00:00Z"),
    (
      "latest-a",
      "run-latest",
      "账号甲",
      1,
      "2026-07-18T01:00:00Z",
    ),
    (
      "latest-b",
      "run-latest",
      "账号乙",
      1,
      "2026-07-18T02:00:00Z",
    ),
    (
      "latest-c",
      "run-latest",
      "账号丙",
      1,
      "2026-07-18T03:00:00Z",
    ),
    (
      "filtered",
      "run-latest",
      "不应展示",
      0,
      "2026-07-18T04:00:00Z",
    ),
    ("other", "run-other", "其他任务", 1, "2026-07-19T01:00:00Z"),
  ] {
    connection
      .execute(
        "INSERT INTO collected_account (
          id, task_run_id, platform, identity_key, username, account, platform_user_id,
          profile_text, country_region, gender, age, followers_count, posts_count,
          profile_url, data_source, collected_at, account_fields_json, field_evidence_json,
          output_included, created_at, updated_at
        ) VALUES (?1, ?2, 'tiktok', ?1, ?3, ?1, ?1, '公开简介', 'US', 'female',
          30, 100, 5, 'https://example.com/profile', 'TikHub API', ?5,
          '{\"bio\":\"公开简介\",\"followers_count\":100,\"country_region\":\"US\",\"friends_count\":0}',
          '{\"bio\":{\"endpoint_key\":\"tiktok.account_profile\",\"raw_path\":\"user.signature\",\"collected_at\":\"2026-07-18T00:00:00Z\"}}',
          ?4, ?5, ?5)",
        params![id, run_id, username, output_included, created_at],
      )
      .expect("collected account should insert");
  }
  drop(connection);

  let page = list_task_results(&root, "task-results", 2, 1).expect("results should list");

  assert_eq!(page.task_id, "task-results");
  assert_eq!(page.task_run_id, "run-latest");
  assert_eq!(page.run_status, "partial_success");
  assert_eq!(page.total_count, 3);
  assert_eq!(page.offset, 1);
  assert_eq!(page.limit, 2);
  assert!(page.age_filter_configured);
  assert!(!page.gender_filter_configured);
  assert_eq!(
    page.selected_fields,
    vec!["bio", "followers_count", "country_region"]
  );
  assert_eq!(
    page
      .items
      .iter()
      .map(|item| item.username.as_deref())
      .collect::<Vec<_>>(),
    vec![Some("账号乙"), Some("账号丙")]
  );
  assert!(page.items.iter().all(|item| item.platform == "tiktok"));
  assert_eq!(page.items[0].account_fields_json["friends_count"], 0);
  assert_eq!(
    page.items[0].field_evidence_json["bio"]["endpoint_key"],
    "tiktok.account_profile"
  );

  fs::remove_dir_all(root).ok();
}
