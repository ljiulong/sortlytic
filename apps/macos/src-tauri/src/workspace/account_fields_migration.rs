use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::api_profile_migration::create_consistent_migration_backup;
use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_VERSION: i64 = 10;
const MIGRATION_NAME: &str = "account_fields_and_sources";

const ADD_TASK_ACCOUNT_SOURCE_SQL: &str = r#"ALTER TABLE collection_task ADD COLUMN account_source TEXT
  CHECK (account_source IS NULL OR account_source IN (
    'user_search', 'content_search_authors', 'direct_account', 'item_author',
    'comment_authors', 'followers', 'followings', 'similar_accounts'
  ));"#;
const ADD_TASK_SELECTED_FIELDS_SQL: &str = r#"ALTER TABLE collection_task
  ADD COLUMN selected_fields_json TEXT NOT NULL DEFAULT '[]'
  CHECK (json_valid(selected_fields_json) AND json_type(selected_fields_json) = 'array');"#;
const ADD_ACCOUNT_FIELDS_SQL: &str = r#"ALTER TABLE collected_account
  ADD COLUMN account_fields_json TEXT NOT NULL DEFAULT '{}'
  CHECK (json_valid(account_fields_json) AND json_type(account_fields_json) = 'object');"#;
const ADD_ACCOUNT_EVIDENCE_SQL: &str = r#"ALTER TABLE collected_account
  ADD COLUMN field_evidence_json TEXT NOT NULL DEFAULT '{}'
  CHECK (json_valid(field_evidence_json) AND json_type(field_evidence_json) = 'object');"#;

const RECORD_TABLES_SQL: &str = r#"
CREATE TABLE raw_record_v10 (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  task_run_id TEXT,
  platform TEXT NOT NULL,
  data_type TEXT NOT NULL DEFAULT 'legacy',
  platform_record_id TEXT NOT NULL,
  raw_url TEXT,
  raw_file_path TEXT NOT NULL,
  raw_hash TEXT NOT NULL,
  summary_json TEXT NOT NULL DEFAULT '{}',
  collected_at TEXT NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (id, task_id, platform),
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE,
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE,
  CHECK (data_type IN (
    'keyword_search', 'user_search', 'comments', 'account_profile', 'account_posts',
    'item_detail', 'followers', 'followings', 'similar_accounts',
    'extended_demographics', 'account_country', 'legacy'
  ))
);
INSERT INTO raw_record_v10 SELECT * FROM raw_record;

CREATE TABLE normalized_record_v10 (
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
  age INTEGER CHECK (age IS NULL OR age BETWEEN 0 AND 130),
  metrics_json TEXT NOT NULL DEFAULT '{}',
  tags_json TEXT NOT NULL DEFAULT '[]',
  account_fields_json TEXT NOT NULL DEFAULT '{}'
    CHECK (json_valid(account_fields_json) AND json_type(account_fields_json) = 'object'),
  field_evidence_json TEXT NOT NULL DEFAULT '{}'
    CHECK (json_valid(field_evidence_json) AND json_type(field_evidence_json) = 'object'),
  normalized_schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (raw_record_id),
  FOREIGN KEY (raw_record_id, task_id, platform)
    REFERENCES raw_record_v10(id, task_id, platform) ON DELETE CASCADE,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);
INSERT INTO normalized_record_v10 (
  id, raw_record_id, task_id, platform, author_id, author_name, content_text,
  content_url, published_at, region, age, metrics_json, tags_json,
  account_fields_json, field_evidence_json, normalized_schema_version, created_at
)
SELECT
  id, raw_record_id, task_id, platform, author_id, author_name, content_text,
  content_url, published_at, region, age, metrics_json, tags_json,
  '{}', '{}', normalized_schema_version, created_at
FROM normalized_record;

DROP TABLE normalized_record;
DROP TABLE raw_record;
ALTER TABLE raw_record_v10 RENAME TO raw_record;
ALTER TABLE normalized_record_v10 RENAME TO normalized_record;

CREATE UNIQUE INDEX idx_raw_record_run_type_identity
ON raw_record(task_run_id, platform, data_type, platform_record_id)
WHERE task_run_id IS NOT NULL;
CREATE UNIQUE INDEX idx_normalized_record_raw_record_id ON normalized_record(raw_record_id);
CREATE INDEX idx_raw_record_task_id ON raw_record(task_id);
CREATE INDEX idx_raw_record_task_run_id ON raw_record(task_run_id);
CREATE INDEX idx_raw_record_platform ON raw_record(platform);
CREATE INDEX idx_raw_record_data_type ON raw_record(data_type);
CREATE INDEX idx_raw_record_platform_record_id ON raw_record(platform_record_id);
CREATE INDEX idx_normalized_record_task_id ON normalized_record(task_id);
"#;

const PIPELINE_TARGET_SQL: &str = r#"
CREATE TABLE collection_pipeline_target_v10 (
  id TEXT PRIMARY KEY,
  task_run_id TEXT NOT NULL,
  step_key TEXT NOT NULL CHECK (length(trim(step_key)) > 0),
  data_type TEXT NOT NULL CHECK (data_type IN (
    'keyword_search', 'user_search', 'comments', 'account_profile', 'account_posts',
    'item_detail', 'followers', 'followings', 'similar_accounts',
    'extended_demographics', 'account_country'
  )),
  target_key TEXT NOT NULL CHECK (length(trim(target_key)) > 0),
  resolved_params_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(resolved_params_json)),
  cursor_json TEXT CHECK (cursor_json IS NULL OR json_valid(cursor_json)),
  status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN (
    'pending', 'running', 'success', 'failed', 'exhausted', 'budget_stopped'
  )),
  request_count INTEGER NOT NULL DEFAULT 0 CHECK (request_count >= 0),
  output_selected INTEGER NOT NULL DEFAULT 1 CHECK (output_selected IN (0, 1)),
  failure_json TEXT CHECK (failure_json IS NULL OR json_valid(failure_json)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (task_run_id, step_key, target_key),
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE
);
INSERT INTO collection_pipeline_target_v10 SELECT * FROM collection_pipeline_target;
DROP TABLE collection_pipeline_target;
ALTER TABLE collection_pipeline_target_v10 RENAME TO collection_pipeline_target;
CREATE INDEX idx_collection_pipeline_target_status
ON collection_pipeline_target(task_run_id, status);
"#;

const MIGRATION_SQL: &[&str] = &[
  ADD_TASK_ACCOUNT_SOURCE_SQL,
  ADD_TASK_SELECTED_FIELDS_SQL,
  ADD_ACCOUNT_FIELDS_SQL,
  ADD_ACCOUNT_EVIDENCE_SQL,
  RECORD_TABLES_SQL,
  PIPELINE_TARGET_SQL,
];

pub(super) fn validate_existing_account_fields_migration(connection: &Connection) -> AppResult<()> {
  if !table_exists(connection, "schema_migrations")? {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    return validate_marker_and_schema(connection, &name, &checksum);
  }
  if declared_schema_version(connection)?.is_some_and(|version| version >= MIGRATION_VERSION) {
    return Err(workspace_error("数据库声明为 v10，但缺少账号字段迁移标记"));
  }
  Ok(())
}

pub(super) fn apply_account_fields_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    update_workspace_schema_version(connection, MIGRATION_VERSION)?;
    return ensure_foreign_key_integrity(connection);
  }
  let requires_table_rebuild =
    !record_schema_is_current(connection)? || !target_schema_is_current(connection)?;
  if requires_table_rebuild {
    match declared_schema_version(connection)? {
      Some(9) => {
        create_consistent_migration_backup(connection, 9, 10)?;
      }
      None if workspace_count(connection)? == 0 => {}
      _ => {
        return Err(workspace_error(
          "账号字段迁移需要重建本地表，但工作区未明确声明为 v9，已拒绝继续",
        ));
      }
    }
  }
  connection
    .execute_batch("PRAGMA foreign_keys = OFF;")
    .map_err(database_error)?;
  let migration_result = (|| -> AppResult<()> {
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    apply_additive_columns(&transaction)?;
    if !record_schema_is_current(&transaction)? {
      transaction
        .execute_batch(RECORD_TABLES_SQL)
        .map_err(database_error)?;
    }
    if !target_schema_is_current(&transaction)? {
      transaction
        .execute_batch(PIPELINE_TARGET_SQL)
        .map_err(database_error)?;
    }
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
    transaction.commit().map_err(database_error)
  })();
  let restore_result = connection
    .execute_batch("PRAGMA foreign_keys = ON;")
    .map_err(database_error);
  migration_result?;
  restore_result?;
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
      "数据库迁移 v10 校验失败，账号字段结构、标记或 checksum 不一致",
    ));
  }
  Ok(())
}

fn workspace_count(connection: &Connection) -> AppResult<i64> {
  connection
    .query_row("SELECT COUNT(*) FROM workspace", [], |row| row.get(0))
    .map_err(database_error)
}

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  let task_columns = columns(connection, "collection_task")?;
  let normalized_columns = columns(connection, "normalized_record")?;
  let account_columns = columns(connection, "collected_account")?;
  let raw_sql = object_sql(connection, "raw_record")?.unwrap_or_default();
  let target_sql = object_sql(connection, "collection_pipeline_target")?.unwrap_or_default();
  Ok(
    ["account_source", "selected_fields_json"]
      .iter()
      .all(|column| task_columns.iter().any(|value| value == column))
      && ["account_fields_json", "field_evidence_json"]
        .iter()
        .all(|column| normalized_columns.iter().any(|value| value == column))
      && ["account_fields_json", "field_evidence_json"]
        .iter()
        .all(|column| account_columns.iter().any(|value| value == column))
      && [
        "'user_search'",
        "'followers'",
        "'followings'",
        "'similar_accounts'",
        "'extended_demographics'",
        "'account_country'",
      ]
      .iter()
      .all(|data_type| raw_sql.contains(data_type) && target_sql.contains(data_type)),
  )
}

fn apply_additive_columns(connection: &Connection) -> AppResult<()> {
  for (table, column, sql) in [
    (
      "collection_task",
      "account_source",
      ADD_TASK_ACCOUNT_SOURCE_SQL,
    ),
    (
      "collection_task",
      "selected_fields_json",
      ADD_TASK_SELECTED_FIELDS_SQL,
    ),
    (
      "collected_account",
      "account_fields_json",
      ADD_ACCOUNT_FIELDS_SQL,
    ),
    (
      "collected_account",
      "field_evidence_json",
      ADD_ACCOUNT_EVIDENCE_SQL,
    ),
  ] {
    if !columns(connection, table)?
      .iter()
      .any(|value| value == column)
    {
      connection.execute_batch(sql).map_err(database_error)?;
    }
  }
  Ok(())
}

fn record_schema_is_current(connection: &Connection) -> AppResult<bool> {
  let normalized = columns(connection, "normalized_record")?;
  let raw_sql = object_sql(connection, "raw_record")?.unwrap_or_default();
  Ok(
    ["account_fields_json", "field_evidence_json"]
      .iter()
      .all(|column| normalized.iter().any(|value| value == column))
      && raw_sql.contains("'user_search'")
      && raw_sql.contains("'account_country'"),
  )
}

fn target_schema_is_current(connection: &Connection) -> AppResult<bool> {
  let target_sql = object_sql(connection, "collection_pipeline_target")?.unwrap_or_default();
  Ok(target_sql.contains("'user_search'") && target_sql.contains("'account_country'"))
}

fn migration_checksum() -> String {
  let mut hasher = Sha256::new();
  for sql in MIGRATION_SQL {
    hasher.update(sql.as_bytes());
  }
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

fn object_sql(connection: &Connection, name: &str) -> AppResult<Option<String>> {
  connection
    .query_row(
      "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
      [name],
      |row| row.get(0),
    )
    .optional()
    .map_err(database_error)
}

#[cfg(test)]
mod tests {
  use std::fs;
  #[cfg(unix)]
  use std::os::unix::fs::PermissionsExt;

  use super::*;
  use crate::workspace::schema::{schema_checksum, SCHEMA_SQL};
  use uuid::Uuid;

  #[test]
  fn v9_upgrade_preserves_records_and_accepts_v4_account_types() {
    let (mut connection, root) = v9_connection();
    install_v9_fixture(&connection);

    apply_account_fields_migration(&mut connection).expect("v10 migration should succeed");
    let backup_path = fs::read_dir(root.join("backups"))
      .unwrap()
      .filter_map(Result::ok)
      .map(|entry| entry.path())
      .find(|path| {
        path
          .file_name()
          .and_then(|name| name.to_str())
          .is_some_and(|name| name.starts_with("app-v9-before-v10-") && name.ends_with(".sqlite"))
      })
      .expect("v9 migration backup should exist");
    assert!(backup_path.with_extension("manifest.json").is_file());
    let backup = Connection::open(&backup_path).unwrap();
    assert_eq!(
      backup
        .query_row(
          "SELECT COUNT(*) FROM raw_record WHERE id = 'raw-v9'",
          [],
          |row| { row.get::<_, i64>(0) }
        )
        .unwrap(),
      1
    );
    assert_eq!(
      backup
        .query_row(
          "SELECT COUNT(*) FROM schema_migrations WHERE version = 10",
          [],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      0
    );
    drop(backup);
    apply_account_fields_migration(&mut connection).expect("v10 migration should be idempotent");

    assert!(schema_is_current(&connection).unwrap());
    assert_eq!(
      connection
        .query_row("SELECT schema_version FROM workspace", [], |row| row
          .get::<_, i64>(0))
        .unwrap(),
      10
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT account_source, selected_fields_json FROM collection_task WHERE id = 'task-v9'",
          [],
          |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap(),
      (None, "[]".to_string())
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT author_name, account_fields_json, field_evidence_json
           FROM normalized_record WHERE id = 'normalized-v9'",
          [],
          |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?
          )),
        )
        .unwrap(),
      ("旧账号".to_string(), "{}".to_string(), "{}".to_string())
    );
    connection
      .execute(
        "INSERT INTO raw_record (
          id, task_id, task_run_id, platform, data_type, platform_record_id,
          raw_file_path, raw_hash, collected_at, created_at
        ) VALUES ('raw-v10', 'task-v9', 'run-v9', 'tiktok', 'user_search',
          'user-v10', 'raw/tikhub/v10.json', 'hash-v10', ?1, ?1)",
        ["2026-07-20T00:00:00Z"],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO collection_pipeline_target (
          id, task_run_id, step_key, data_type, target_key, created_at, updated_at
        ) VALUES ('target-v10', 'run-v9', 'discover', 'followers', 'seed', ?1, ?1)",
        ["2026-07-20T00:00:00Z"],
      )
      .unwrap();
    assert_eq!(
      connection
        .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        .unwrap(),
      "ok"
    );
    ensure_foreign_key_integrity(&connection).unwrap();
    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn marker_or_partial_schema_tampering_fails_closed() {
    let (mut connection, root) = v9_connection();
    apply_account_fields_migration(&mut connection).unwrap();
    connection
      .execute(
        "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 10",
        [],
      )
      .unwrap();
    assert!(validate_existing_account_fields_migration(&connection).is_err());
    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  fn v9_connection() -> (Connection, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("account-fields-v9-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    #[cfg(unix)]
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let mut connection = Connection::open_in_memory().unwrap();
    connection
      .execute_batch("PRAGMA foreign_keys = ON;")
      .unwrap();
    connection.execute_batch(SCHEMA_SQL).unwrap();
    let now = "2026-07-19T00:00:00Z";
    connection
      .execute(
        "INSERT INTO workspace (
          id, name, root_path, created_at, updated_at, schema_version, last_opened_at
        ) VALUES ('workspace-v9', 'v9 fixture', ?1, ?2, ?2, 1, ?2)",
        params![root.to_string_lossy(), now],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO schema_migrations (version, name, applied_at, checksum)
         VALUES (1, 'initial_schema', ?1, ?2)",
        params![now, schema_checksum()],
      )
      .unwrap();
    super::super::apply_record_observation_migration(&mut connection).unwrap();
    super::super::apply_tikhub_connector_migration(&mut connection).unwrap();
    super::super::apply_run_checkpoint_migration(&mut connection).unwrap();
    super::super::apply_active_run_migration(&mut connection).unwrap();
    super::super::apply_collection_runtime_migration(&mut connection).unwrap();
    super::super::apply_collection_pipeline_migration(&mut connection).unwrap();
    super::super::apply_api_profile_migration(&mut connection).unwrap();
    super::super::apply_plan_review_migration(&mut connection).unwrap();
    assert_eq!(
      connection
        .query_row("SELECT schema_version FROM workspace", [], |row| row
          .get::<_, i64>(0))
        .unwrap(),
      9
    );
    (connection, root)
  }

  fn install_v9_fixture(connection: &Connection) {
    let now = "2026-07-19T00:00:00Z";
    connection
      .execute(
        "INSERT INTO collection_task (
          id, name, source_type, status, platforms_json, data_types_json, created_at, updated_at
        ) VALUES ('task-v9', '历史任务', 'form', 'success', '[\"tiktok\"]',
          '[\"account_posts\"]', ?1, ?1)",
        [now],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO collection_plan (
          id, task_id, source, schema_version, plan_json, validation_status,
          confirmed_by_user, created_at, updated_at
        ) VALUES ('plan-v9', 'task-v9', 'form_generated', 3, '{}', 'valid', 1, ?1, ?1)",
        [now],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO task_run (
          id, task_id, status, started_at, ended_at, retryable, plan_id, attempt_number
        ) VALUES ('run-v9', 'task-v9', 'success', ?1, ?1, 0, 'plan-v9', 1)",
        [now],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO raw_record (
          id, task_id, task_run_id, platform, data_type, platform_record_id,
          raw_file_path, raw_hash, collected_at, created_at
        ) VALUES ('raw-v9', 'task-v9', 'run-v9', 'tiktok', 'account_posts',
          'user-v9', 'raw/tikhub/v9.json', 'hash-v9', ?1, ?1)",
        [now],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO normalized_record (
          id, raw_record_id, task_id, platform, author_id, author_name, age,
          metrics_json, tags_json, normalized_schema_version, created_at
        ) VALUES ('normalized-v9', 'raw-v9', 'task-v9', 'tiktok', 'user-v9',
          '旧账号', 0, '{\"followers\":0}', '[]', 1, ?1)",
        [now],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO collection_pipeline_target (
          id, task_run_id, step_key, data_type, target_key, created_at, updated_at
        ) VALUES ('target-v9', 'run-v9', 'account_posts', 'account_posts', 'user-v9', ?1, ?1)",
        [now],
      )
      .unwrap();
    connection
      .execute(
        "INSERT INTO collected_account (
          id, task_run_id, platform, identity_key, username, data_source,
          collected_at, created_at, updated_at
        ) VALUES ('account-v9', 'run-v9', 'tiktok', 'id:user-v9', '旧账号',
          'TikHub API', ?1, ?1, ?1)",
        [now],
      )
      .unwrap();
  }
}
