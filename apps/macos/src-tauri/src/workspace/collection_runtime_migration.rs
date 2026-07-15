use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_NAME: &str = "collection_runtime_snapshot";
const SNAPSHOT_TABLE: &str = "collection_runtime_snapshot";
const SNAPSHOT_INDEX: &str = "idx_collection_runtime_snapshot_task_run_id";

const SECRET_REF_TABLE_V6_SQL: &str = r#"CREATE TABLE secret_ref (
  id TEXT PRIMARY KEY,
  provider_type TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  alias TEXT,
  secret_store_key TEXT NOT NULL,
  masked_hint TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  last_tested_at TEXT,
  last_test_status TEXT,
  credential_revision INTEGER NOT NULL DEFAULT 1 CHECK (credential_revision > 0)
)"#;

const CREDENTIAL_REVISION_SQL: &str = r#"ALTER TABLE secret_ref
ADD COLUMN credential_revision INTEGER NOT NULL DEFAULT 1
  CHECK (credential_revision > 0);

UPDATE tikhub_connector
SET config_version = config_version + 1,
    last_tested_at = NULL,
    last_test_status = NULL,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now');"#;

const SNAPSHOT_TABLE_SQL: &str = r#"CREATE TABLE collection_runtime_snapshot (
  id TEXT PRIMARY KEY CHECK (length(trim(id)) > 0),
  task_run_id TEXT NOT NULL,
  workspace_id TEXT NOT NULL,
  runtime_contract_version INTEGER NOT NULL DEFAULT 1
    CHECK (runtime_contract_version = 1),
  plan_id TEXT NOT NULL,
  plan_schema_version INTEGER NOT NULL CHECK (plan_schema_version >= 2),
  plan_json TEXT NOT NULL CHECK (json_valid(plan_json)),
  connector_type TEXT NOT NULL CHECK (connector_type = 'tikhub'),
  connector_id TEXT NOT NULL,
  connector_config_version INTEGER NOT NULL CHECK (connector_config_version > 0),
  base_url TEXT NOT NULL CHECK (
    base_url IN ('https://api.tikhub.io', 'https://api.tikhub.dev')
  ),
  secret_ref_id TEXT NOT NULL CHECK (length(trim(secret_ref_id)) > 0),
  secret_revision INTEGER NOT NULL CHECK (secret_revision > 0),
  secret_provider_type TEXT NOT NULL CHECK (secret_provider_type = 'tikhub'),
  secret_provider_id TEXT NOT NULL CHECK (length(trim(secret_provider_id)) > 0),
  connector_tested_at TEXT NOT NULL CHECK (length(trim(connector_tested_at)) > 0),
  connector_test_status TEXT NOT NULL CHECK (connector_test_status = 'success'),
  created_at TEXT NOT NULL CHECK (length(trim(created_at)) > 0),
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE
);"#;

const SNAPSHOT_INDEX_SQL: &str = r#"CREATE UNIQUE INDEX idx_collection_runtime_snapshot_task_run_id
ON collection_runtime_snapshot(task_run_id);"#;

const CREDENTIAL_OVERFLOW_TRIGGER_SQL: &str = r#"CREATE TRIGGER trg_secret_ref_credential_revision_overflow
BEFORE UPDATE OF secret_store_key, masked_hint ON secret_ref
WHEN OLD.credential_revision >= 9223372036854775807 OR EXISTS (
  SELECT 1 FROM tikhub_connector
  WHERE secret_ref_id = OLD.id AND config_version >= 9223372036854775807
)
BEGIN
  SELECT RAISE(ABORT, 'secret credential revision overflow');
END;"#;

const CREDENTIAL_REVISION_TRIGGER_SQL: &str = r#"CREATE TRIGGER trg_secret_ref_credential_revision
AFTER UPDATE OF secret_store_key, masked_hint ON secret_ref
BEGIN
  UPDATE secret_ref
  SET credential_revision = OLD.credential_revision + 1
  WHERE id = OLD.id;
END;"#;

const CREDENTIAL_INVALIDATION_TRIGGER_SQL: &str = r#"CREATE TRIGGER trg_secret_ref_credential_invalidates_connector
AFTER UPDATE OF secret_store_key, masked_hint ON secret_ref
BEGIN
  UPDATE tikhub_connector
  SET config_version = config_version + 1,
      last_tested_at = NULL,
      last_test_status = NULL,
      updated_at = NEW.updated_at
  WHERE secret_ref_id = OLD.id;
END;"#;

const SNAPSHOT_INSERT_TRIGGER_SQL: &str = r#"CREATE TRIGGER trg_collection_runtime_snapshot_insert
BEFORE INSERT ON collection_runtime_snapshot
WHEN NOT EXISTS (
  SELECT 1
  FROM task_run AS run
  JOIN collection_plan AS plan
    ON plan.id = run.plan_id AND plan.task_id = run.task_id
  JOIN workspace AS workspace ON workspace.id = NEW.workspace_id
  JOIN tikhub_connector AS connector
    ON connector.id = NEW.connector_id AND connector.workspace_id = workspace.id
  JOIN secret_ref AS secret ON secret.id = NEW.secret_ref_id
  WHERE run.id = NEW.task_run_id
    AND run.status = 'queued'
    AND run.claimed_at IS NULL
    AND run.plan_id = NEW.plan_id
    AND plan.schema_version = NEW.plan_schema_version
    AND plan.plan_json = NEW.plan_json
    AND plan.validation_status = 'valid'
    AND plan.confirmed_by_user = 1
    AND NEW.connector_type = 'tikhub'
    AND connector.enabled = 1
    AND connector.config_version = NEW.connector_config_version
    AND connector.base_url = NEW.base_url
    AND connector.secret_ref_id = NEW.secret_ref_id
    AND connector.last_tested_at = NEW.connector_tested_at
    AND connector.last_test_status = NEW.connector_test_status
    AND secret.provider_type = NEW.secret_provider_type
    AND secret.provider_id = NEW.secret_provider_id
    AND secret.credential_revision = NEW.secret_revision
    AND EXISTS (
      SELECT 1 FROM task_run_step WHERE task_run_id = run.id
    )
    AND NOT EXISTS (
      SELECT 1 FROM task_run_step
      WHERE task_run_id = run.id AND (
        status <> 'pending' OR stop_reason IS NOT NULL
        OR started_at IS NOT NULL OR completed_at IS NOT NULL
      )
    )
    AND NOT EXISTS (
      SELECT 1
      FROM collection_page_checkpoint AS checkpoint
      JOIN task_run_step AS run_step ON run_step.id = checkpoint.task_run_step_id
      WHERE run_step.task_run_id = run.id
    )
)
BEGIN
  SELECT RAISE(ABORT, 'collection runtime snapshot source mismatch');
END;"#;

const SNAPSHOT_UPDATE_TRIGGER_SQL: &str = r#"CREATE TRIGGER trg_collection_runtime_snapshot_immutable_update
BEFORE UPDATE ON collection_runtime_snapshot
BEGIN
  SELECT RAISE(ABORT, 'collection runtime snapshot is immutable');
END;"#;

const SNAPSHOT_DELETE_TRIGGER_SQL: &str = r#"CREATE TRIGGER trg_collection_runtime_snapshot_immutable_delete
BEFORE DELETE ON collection_runtime_snapshot
WHEN EXISTS (SELECT 1 FROM task_run WHERE id = OLD.task_run_id)
BEGIN
  SELECT RAISE(ABORT, 'collection runtime snapshot cannot be deleted directly');
END;"#;

const MIGRATION_SQL: &[&str] = &[
  CREDENTIAL_REVISION_SQL,
  SNAPSHOT_TABLE_SQL,
  SNAPSHOT_INDEX_SQL,
  CREDENTIAL_OVERFLOW_TRIGGER_SQL,
  CREDENTIAL_REVISION_TRIGGER_SQL,
  CREDENTIAL_INVALIDATION_TRIGGER_SQL,
  SNAPSHOT_INSERT_TRIGGER_SQL,
  SNAPSHOT_UPDATE_TRIGGER_SQL,
  SNAPSHOT_DELETE_TRIGGER_SQL,
];

pub(super) fn validate_existing_collection_runtime_migration(
  connection: &Connection,
) -> AppResult<()> {
  if columns(connection, "schema_migrations")?.is_empty() {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
  } else if declared_schema_version(connection)?.is_some_and(|version| version >= 6) {
    return Err(workspace_error(
      "数据库迁移 v6 校验失败，工作区版本已升级但采集运行快照标记缺失",
    ));
  }
  Ok(())
}

pub(super) fn apply_collection_runtime_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    update_workspace_schema_version(&transaction, 6)?;
    ensure_foreign_key_integrity(&transaction)?;
    return transaction.commit().map_err(database_error);
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if migration_artifacts_present(&transaction)? {
    return Err(workspace_error(
      "数据库迁移 v6 发现未标记或不完整的采集运行快照结构，已拒绝自动修复",
    ));
  }
  let connector_overflow_count = transaction
    .query_row(
      "SELECT COUNT(*) FROM tikhub_connector
       WHERE config_version >= 9223372036854775807",
      [],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  if connector_overflow_count != 0 {
    return Err(workspace_error(
      "数据库迁移 v6 无法安全提升旧连接器版本，已拒绝迁移",
    ));
  }
  for sql in MIGRATION_SQL {
    transaction.execute_batch(sql).map_err(database_error)?;
  }
  if !schema_is_current(&transaction)? {
    return Err(workspace_error("数据库迁移 v6 后采集运行快照结构校验失败"));
  }
  transaction
    .execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (6, ?1, ?2, ?3)",
      params![
        MIGRATION_NAME,
        Utc::now().to_rfc3339(),
        migration_checksum()
      ],
    )
    .map_err(database_error)?;
  update_workspace_schema_version(&transaction, 6)?;
  ensure_foreign_key_integrity(&transaction)?;
  transaction.commit().map_err(database_error)
}

fn validate_marker_and_schema(
  connection: &Connection,
  name: &str,
  checksum: &str,
) -> AppResult<()> {
  if name != MIGRATION_NAME || checksum != migration_checksum() {
    return Err(workspace_error(
      "数据库迁移 v6 校验失败，采集运行快照标记或 checksum 不一致",
    ));
  }
  if !schema_is_current(connection)? {
    return Err(workspace_error(
      "数据库迁移 v6 结构校验失败，采集运行快照结构与标记不一致",
    ));
  }
  Ok(())
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
      "SELECT name, checksum FROM schema_migrations WHERE version = 6",
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

fn migration_artifacts_present(connection: &Connection) -> AppResult<bool> {
  if columns(connection, "secret_ref")?
    .iter()
    .any(|column| column == "credential_revision")
  {
    return Ok(true);
  }
  for (kind, name) in [
    ("table", SNAPSHOT_TABLE),
    ("index", SNAPSHOT_INDEX),
    ("trigger", "trg_secret_ref_credential_revision_overflow"),
    ("trigger", "trg_secret_ref_credential_revision"),
    ("trigger", "trg_secret_ref_credential_invalidates_connector"),
    ("trigger", "trg_collection_runtime_snapshot_insert"),
    (
      "trigger",
      "trg_collection_runtime_snapshot_immutable_update",
    ),
    (
      "trigger",
      "trg_collection_runtime_snapshot_immutable_delete",
    ),
  ] {
    if object_sql(connection, kind, name)?.is_some() {
      return Ok(true);
    }
  }
  Ok(false)
}

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  if columns(connection, "secret_ref")?.join(",")
    != "id,provider_type,provider_id,alias,secret_store_key,masked_hint,created_at,updated_at,last_tested_at,last_test_status,credential_revision"
    || columns(connection, SNAPSHOT_TABLE)?.join(",")
      != "id,task_run_id,workspace_id,runtime_contract_version,plan_id,plan_schema_version,plan_json,connector_type,connector_id,connector_config_version,base_url,secret_ref_id,secret_revision,secret_provider_type,secret_provider_id,connector_tested_at,connector_test_status,created_at"
  {
    return Ok(false);
  }
  let expected_secret_ref_sql = expected_object_sql(SECRET_REF_TABLE_V6_SQL);
  let actual_secret_ref_sql = object_sql(connection, "table", "secret_ref")?;
  if actual_secret_ref_sql.as_deref() != Some(expected_secret_ref_sql.as_str()) {
    return Ok(false);
  }
  for (kind, name, expected_sql) in [
    ("table", SNAPSHOT_TABLE, SNAPSHOT_TABLE_SQL),
    ("index", SNAPSHOT_INDEX, SNAPSHOT_INDEX_SQL),
    (
      "trigger",
      "trg_secret_ref_credential_revision_overflow",
      CREDENTIAL_OVERFLOW_TRIGGER_SQL,
    ),
    (
      "trigger",
      "trg_secret_ref_credential_revision",
      CREDENTIAL_REVISION_TRIGGER_SQL,
    ),
    (
      "trigger",
      "trg_secret_ref_credential_invalidates_connector",
      CREDENTIAL_INVALIDATION_TRIGGER_SQL,
    ),
    (
      "trigger",
      "trg_collection_runtime_snapshot_insert",
      SNAPSHOT_INSERT_TRIGGER_SQL,
    ),
    (
      "trigger",
      "trg_collection_runtime_snapshot_immutable_update",
      SNAPSHOT_UPDATE_TRIGGER_SQL,
    ),
    (
      "trigger",
      "trg_collection_runtime_snapshot_immutable_delete",
      SNAPSHOT_DELETE_TRIGGER_SQL,
    ),
  ] {
    let expected_sql = expected_object_sql(expected_sql);
    let actual_sql = object_sql(connection, kind, name)?;
    if actual_sql.as_deref() != Some(expected_sql.as_str()) {
      return Ok(false);
    }
  }
  for pragma in [
    "PRAGMA integrity_check('secret_ref')",
    "PRAGMA integrity_check('collection_runtime_snapshot')",
  ] {
    if !integrity_check_is_ok(connection, pragma)? {
      return Ok(false);
    }
  }
  Ok(true)
}

fn integrity_check_is_ok(connection: &Connection, pragma: &str) -> AppResult<bool> {
  let mut statement = connection.prepare(pragma).map_err(database_error)?;
  let results = statement
    .query_map([], |row| row.get::<_, String>(0))
    .map_err(database_error)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;
  Ok(results.as_slice() == ["ok"])
}

fn columns(connection: &Connection, table: &str) -> AppResult<Vec<String>> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .map_err(database_error)?;
  let rows = statement
    .query_map([], |row| row.get(1))
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn object_sql(connection: &Connection, kind: &str, name: &str) -> AppResult<Option<String>> {
  connection
    .query_row(
      "SELECT sql FROM sqlite_schema WHERE type = ?1 AND name = ?2",
      params![kind, name],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|sql| sql.map(|value| normalize_sql(&value)))
    .map_err(database_error)
}

fn expected_object_sql(sql: &str) -> String {
  normalize_sql(sql.trim_end_matches(';'))
}

fn normalize_sql(sql: &str) -> String {
  let mut normalized = String::with_capacity(sql.len());
  let mut token = String::new();
  let mut characters = sql.chars().peekable();
  while let Some(character) = characters.next() {
    if character.is_whitespace() {
      push_sql_token(&mut normalized, &mut token);
    } else if matches!(character, '\'' | '"' | '`') {
      push_sql_token(&mut normalized, &mut token);
      let quote = character;
      token.push(character);
      while let Some(quoted_character) = characters.next() {
        token.push(quoted_character);
        if quoted_character == quote {
          if characters.peek() == Some(&quote) {
            if let Some(escaped_quote) = characters.next() {
              token.push(escaped_quote);
            }
          } else {
            break;
          }
        }
      }
      push_sql_token(&mut normalized, &mut token);
    } else if character.is_alphanumeric() || matches!(character, '_' | '$') {
      token.push(character.to_ascii_lowercase());
    } else {
      push_sql_token(&mut normalized, &mut token);
      token.push(character);
      if let Some(next) = characters.peek().copied() {
        if is_compound_sql_symbol(character, next) {
          token.push(next);
          characters.next();
          if character == '-' && next == '>' && characters.peek().copied() == Some('>') {
            token.push('>');
            characters.next();
          }
        }
      }
      push_sql_token(&mut normalized, &mut token);
    }
  }
  push_sql_token(&mut normalized, &mut token);
  normalized
}

fn push_sql_token(normalized: &mut String, token: &mut String) {
  if token.is_empty() {
    return;
  }
  normalized.push_str(&token.len().to_string());
  normalized.push(':');
  normalized.push_str(token);
  token.clear();
}

fn is_compound_sql_symbol(first: char, second: char) -> bool {
  matches!(
    (first, second),
    ('<', '=')
      | ('>', '=')
      | ('=', '=')
      | ('!', '=')
      | ('<', '>')
      | ('<', '<')
      | ('>', '>')
      | ('|', '|')
      | ('-', '>')
      | ('-', '-')
      | ('/', '*')
      | ('*', '/')
  )
}
