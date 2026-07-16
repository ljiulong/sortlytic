use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_NAME: &str = "collection_pipeline_accounts";

const RECORD_TABLES_SQL: &str = r#"
CREATE TABLE raw_record_v7 (
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
    'keyword_search', 'comments', 'account_profile', 'account_posts', 'item_detail', 'legacy'
  ))
);

INSERT INTO raw_record_v7 SELECT * FROM raw_record;

CREATE TABLE normalized_record_v7 (
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
  normalized_schema_version INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  UNIQUE (raw_record_id),
  FOREIGN KEY (raw_record_id, task_id, platform)
    REFERENCES raw_record_v7(id, task_id, platform) ON DELETE CASCADE,
  FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
);

INSERT INTO normalized_record_v7 (
  id, raw_record_id, task_id, platform, author_id, author_name, content_text,
  content_url, published_at, region, age, metrics_json, tags_json,
  normalized_schema_version, created_at
)
SELECT
  id, raw_record_id, task_id, platform, author_id, author_name, content_text,
  content_url, published_at, region, NULL, metrics_json, tags_json,
  normalized_schema_version, created_at
FROM normalized_record;

DROP TABLE normalized_record;
DROP TABLE raw_record;
ALTER TABLE raw_record_v7 RENAME TO raw_record;
ALTER TABLE normalized_record_v7 RENAME TO normalized_record;

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

const PIPELINE_TABLES_SQL: &str = r#"
CREATE TABLE collection_pipeline_target (
  id TEXT PRIMARY KEY,
  task_run_id TEXT NOT NULL,
  step_key TEXT NOT NULL CHECK (length(trim(step_key)) > 0),
  data_type TEXT NOT NULL CHECK (data_type IN (
    'keyword_search', 'comments', 'account_profile', 'account_posts', 'item_detail'
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
CREATE INDEX idx_collection_pipeline_target_status
ON collection_pipeline_target(task_run_id, status);

CREATE TABLE collected_account (
  id TEXT PRIMARY KEY,
  task_run_id TEXT NOT NULL,
  platform TEXT NOT NULL CHECK (platform IN ('tiktok', 'douyin', 'xiaohongshu')),
  identity_key TEXT NOT NULL CHECK (length(trim(identity_key)) > 0),
  username TEXT,
  account TEXT,
  platform_user_id TEXT,
  profile_text TEXT,
  country_region TEXT,
  region_source TEXT,
  region_confidence TEXT,
  gender TEXT,
  age INTEGER CHECK (age IS NULL OR age BETWEEN 0 AND 130),
  followers_count INTEGER CHECK (followers_count IS NULL OR followers_count >= 0),
  posts_count INTEGER CHECK (posts_count IS NULL OR posts_count >= 0),
  last_posted_at TEXT,
  profile_url TEXT,
  data_source TEXT NOT NULL DEFAULT 'TikHub API',
  collected_at TEXT NOT NULL,
  notes TEXT,
  merged_record_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(merged_record_json)),
  source_priority_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(source_priority_json)),
  output_included INTEGER NOT NULL DEFAULT 0 CHECK (output_included IN (0, 1)),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (task_run_id, platform, identity_key),
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE
);
CREATE INDEX idx_collected_account_output
ON collected_account(task_run_id, output_included, created_at);

CREATE TABLE collection_failure_evidence (
  id TEXT PRIMARY KEY,
  task_run_id TEXT NOT NULL,
  target_id TEXT,
  step_key TEXT NOT NULL,
  endpoint_key TEXT NOT NULL,
  target_key TEXT,
  error_code TEXT NOT NULL,
  error_message TEXT NOT NULL,
  retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
  evidence_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(evidence_json)),
  created_at TEXT NOT NULL,
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE,
  FOREIGN KEY (target_id) REFERENCES collection_pipeline_target(id) ON DELETE SET NULL
);
CREATE INDEX idx_collection_failure_evidence_run
ON collection_failure_evidence(task_run_id, step_key);

CREATE TABLE pricing_quote_snapshot (
  id TEXT PRIMARY KEY,
  task_run_id TEXT NOT NULL,
  endpoint_key TEXT NOT NULL,
  currency TEXT NOT NULL DEFAULT 'USD' CHECK (currency = 'USD'),
  quoted_cost_micros INTEGER NOT NULL CHECK (quoted_cost_micros >= 0),
  accumulated_quote_micros INTEGER NOT NULL CHECK (accumulated_quote_micros >= 0),
  balance_micros INTEGER NOT NULL CHECK (balance_micros >= 0),
  free_credit_micros INTEGER NOT NULL CHECK (free_credit_micros >= 0),
  available_micros INTEGER NOT NULL CHECK (
    available_micros = balance_micros + free_credit_micros
  ),
  quote_json TEXT NOT NULL CHECK (json_valid(quote_json)),
  quoted_at TEXT NOT NULL,
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE
);
CREATE INDEX idx_pricing_quote_snapshot_run
ON pricing_quote_snapshot(task_run_id, quoted_at);
"#;

const MIGRATION_SQL: &[&str] = &[RECORD_TABLES_SQL, PIPELINE_TABLES_SQL];

pub(super) fn validate_existing_collection_pipeline_migration(
  connection: &Connection,
) -> AppResult<()> {
  if columns(connection, "schema_migrations")?.is_empty() {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
  } else if declared_schema_version(connection)?.is_some_and(|version| version >= 7) {
    return Err(workspace_error(
      "数据库迁移 v7 校验失败，工作区版本已升级但采集流水线标记缺失",
    ));
  }
  Ok(())
}

pub(super) fn apply_collection_pipeline_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    update_workspace_schema_version(connection, 7)?;
    return ensure_foreign_key_integrity(connection);
  }
  if migration_artifacts_present(connection)? {
    return Err(workspace_error(
      "数据库迁移 v7 发现未标记或不完整的采集流水线结构，已拒绝自动修复",
    ));
  }

  connection
    .execute_batch("PRAGMA foreign_keys = OFF;")
    .map_err(database_error)?;
  let migration_result = (|| -> AppResult<()> {
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    for sql in MIGRATION_SQL {
      transaction.execute_batch(sql).map_err(database_error)?;
    }
    transaction
      .execute(
        "INSERT INTO schema_migrations (version, name, applied_at, checksum)
         VALUES (7, ?1, ?2, ?3)",
        params![
          MIGRATION_NAME,
          Utc::now().to_rfc3339(),
          migration_checksum()
        ],
      )
      .map_err(database_error)?;
    update_workspace_schema_version(&transaction, 7)?;
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
      "数据库迁移 v7 校验失败，采集流水线结构、标记或 checksum 不一致",
    ));
  }
  Ok(())
}

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  let required_tables = [
    "collection_pipeline_target",
    "collected_account",
    "collection_failure_evidence",
    "pricing_quote_snapshot",
  ];
  if required_tables
    .iter()
    .any(|table| !object_exists(connection, "table", table).unwrap_or(false))
  {
    return Ok(false);
  }
  let normalized_columns = columns(connection, "normalized_record")?;
  let raw_sql = object_sql(connection, "table", "raw_record")?.unwrap_or_default();
  Ok(normalized_columns.iter().any(|column| column == "age") && raw_sql.contains("'account_posts'"))
}

fn migration_artifacts_present(connection: &Connection) -> AppResult<bool> {
  if columns(connection, "normalized_record")?
    .iter()
    .any(|column| column == "age")
  {
    return Ok(true);
  }
  for table in [
    "collection_pipeline_target",
    "collected_account",
    "collection_failure_evidence",
    "pricing_quote_snapshot",
  ] {
    if object_exists(connection, "table", table)? {
      return Ok(true);
    }
  }
  Ok(false)
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
      "SELECT name, checksum FROM schema_migrations WHERE version = 7",
      [],
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

fn object_exists(connection: &Connection, kind: &str, name: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = ?1 AND name = ?2)",
      params![kind, name],
      |row| row.get(0),
    )
    .map_err(database_error)
}

fn object_sql(connection: &Connection, kind: &str, name: &str) -> AppResult<Option<String>> {
  connection
    .query_row(
      "SELECT sql FROM sqlite_master WHERE type = ?1 AND name = ?2",
      params![kind, name],
      |row| row.get(0),
    )
    .optional()
    .map_err(database_error)
}
