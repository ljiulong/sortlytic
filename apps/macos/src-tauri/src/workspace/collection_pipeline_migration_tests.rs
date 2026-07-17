use std::fs;

use rusqlite::params;
use uuid::Uuid;

use super::*;

#[test]
fn fresh_workspace_creates_v7_pipeline_account_and_pricing_contracts() {
  let root = std::env::temp_dir().join(format!("sortlytic-v7-{}", Uuid::new_v4()));
  let summary = create_workspace("v7 采集流水线", &root).expect("工作区应创建");
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("数据库应打开");

  assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
  assert_eq!(
    migration_marker(&connection, 7).0,
    "collection_pipeline_accounts"
  );
  for table in [
    "collection_pipeline_target",
    "collected_account",
    "collection_failure_evidence",
    "pricing_quote_snapshot",
  ] {
    assert_eq!(object_count(&connection, "table", table), 1, "{table}");
  }
  assert!(columns(&connection, "normalized_record").contains(&"age".to_string()));

  insert_record_fixture(&connection);
  assert_eq!(
    connection
      .execute(
        "INSERT INTO normalized_record (
           id, raw_record_id, task_id, platform, age, normalized_schema_version, created_at
         ) VALUES ('norm-0', 'raw-account-post', 'task-v7', 'tiktok', 0, 3, '2026-07-16T00:00:00Z')",
        [],
      )
      .expect("年龄 0 应属于合法闭区间"),
    1
  );
  connection
    .execute(
      "UPDATE normalized_record SET age = 130 WHERE id = 'norm-0'",
      [],
    )
    .expect("年龄 130 应属于合法闭区间");
  connection
    .execute(
      "UPDATE normalized_record SET age = 131 WHERE id = 'norm-0'",
      [],
    )
    .expect_err("超过 130 的年龄必须被数据库拒绝");

  assert_eq!(foreign_key_violation_count(&connection), 0);
  drop(connection);
  fs::remove_dir_all(root).ok();
}

fn insert_record_fixture(connection: &rusqlite::Connection) {
  connection
    .execute(
      "INSERT INTO collection_task (
         id, name, source_type, status, created_at, updated_at
       ) VALUES ('task-v7', '任务', 'form', 'draft', '2026-07-16T00:00:00Z', '2026-07-16T00:00:00Z')",
      [],
    )
    .expect("任务应插入");
  connection
    .execute(
      "INSERT INTO raw_record (
         id, task_id, platform, data_type, platform_record_id, raw_file_path,
         raw_hash, collected_at, created_at
       ) VALUES (
         'raw-account-post', 'task-v7', 'tiktok', 'account_posts', 'post-1',
         'raw/tikhub/post-1.json', 'hash', '2026-07-16T00:00:00Z', '2026-07-16T00:00:00Z'
       )",
      [],
    )
    .expect("account_posts 原始记录应被 v7 接受");
}

fn object_count(connection: &rusqlite::Connection, kind: &str, name: &str) -> i64 {
  connection
    .query_row(
      "SELECT COUNT(*) FROM sqlite_master WHERE type = ?1 AND name = ?2",
      params![kind, name],
      |row| row.get(0),
    )
    .expect("sqlite_master 应可查询")
}

fn columns(connection: &rusqlite::Connection, table: &str) -> Vec<String> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .expect("table_info 应准备成功");
  statement
    .query_map([], |row| row.get(1))
    .expect("列应可读取")
    .collect::<Result<Vec<_>, _>>()
    .expect("列应解析")
}

fn migration_marker(connection: &rusqlite::Connection, version: i64) -> (String, String) {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = ?1",
      [version],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("迁移标记应存在")
}

fn foreign_key_violation_count(connection: &rusqlite::Connection) -> i64 {
  let mut statement = connection
    .prepare("PRAGMA foreign_key_check")
    .expect("foreign_key_check 应准备成功");
  statement
    .query_map([], |_| Ok(()))
    .expect("外键检查应执行")
    .count() as i64
}
