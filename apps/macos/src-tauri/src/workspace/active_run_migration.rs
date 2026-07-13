use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_NAME: &str = "single_active_task_run";
const INDEX_NAME: &str = "idx_task_run_single_active";

const ACTIVE_RUN_CONFLICT_REPAIR_SQL: &str = r#"
DROP TABLE IF EXISTS temp.active_run_conflict_run_v5;
DROP TABLE IF EXISTS temp.active_run_conflict_task_v5;

CREATE TEMP TABLE active_run_conflict_task_v5 (
  task_id TEXT PRIMARY KEY,
  original_task_status TEXT NOT NULL,
  original_confirmed_at TEXT,
  original_completed_at TEXT,
  original_cancelled_at TEXT,
  original_updated_at TEXT NOT NULL,
  active_run_count INTEGER NOT NULL CHECK (active_run_count > 1)
);

INSERT INTO active_run_conflict_task_v5 (
  task_id, original_task_status, original_confirmed_at,
  original_completed_at, original_cancelled_at, original_updated_at, active_run_count
)
SELECT task.id, task.status, task.confirmed_at, task.completed_at, task.cancelled_at,
       task.updated_at, COUNT(run.id)
FROM collection_task AS task
JOIN task_run AS run ON run.task_id = task.id
WHERE run.status IN ('queued', 'running')
GROUP BY task.id, task.status, task.confirmed_at, task.completed_at, task.cancelled_at,
         task.updated_at
HAVING COUNT(run.id) > 1;

CREATE TEMP TABLE active_run_conflict_run_v5 (
  run_id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  original_status TEXT NOT NULL,
  original_current_stage TEXT,
  original_error_code TEXT,
  original_error_message_was_null INTEGER NOT NULL
    CHECK (original_error_message_was_null IN (0, 1)),
  original_ended_at TEXT,
  original_retryable INTEGER NOT NULL,
  original_claimed_at TEXT
);

INSERT INTO active_run_conflict_run_v5 (
  run_id, task_id, original_status, original_current_stage,
  original_error_code, original_error_message_was_null, original_ended_at,
  original_retryable, original_claimed_at
)
SELECT run.id, run.task_id, run.status, run.current_stage,
       run.error_code, run.error_message IS NULL, run.ended_at,
       run.retryable, run.claimed_at
FROM task_run AS run
JOIN active_run_conflict_task_v5 AS conflict ON conflict.task_id = run.task_id
WHERE run.status IN ('queued', 'running');

INSERT INTO task_log (
  id, task_run_id, stage, level, message, safe_details_json, created_at
)
SELECT lower(hex(randomblob(16))), snapshot.run_id, '活动运行冲突迁移', 'error',
       '检测到同一任务存在多个活动运行，所有活动运行已停止并要求人工复核',
       json_object(
         'migration_version', 5,
         'original_run_status', snapshot.original_status,
         'original_current_stage', snapshot.original_current_stage,
         'original_error_code', snapshot.original_error_code,
         'original_error_message_was_null', snapshot.original_error_message_was_null,
         'original_ended_at', snapshot.original_ended_at,
         'original_retryable', snapshot.original_retryable,
         'original_claimed_at', snapshot.original_claimed_at,
         'original_task_status', conflict.original_task_status,
         'original_confirmed_at', conflict.original_confirmed_at,
         'original_completed_at', conflict.original_completed_at,
         'original_cancelled_at', conflict.original_cancelled_at,
         'original_task_updated_at', conflict.original_updated_at,
         'active_run_count', conflict.active_run_count
       ),
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM active_run_conflict_run_v5 AS snapshot
JOIN active_run_conflict_task_v5 AS conflict ON conflict.task_id = snapshot.task_id;

INSERT INTO task_log (
  id, task_run_id, stage, level, message, safe_details_json, created_at
)
SELECT lower(hex(randomblob(16))), step.task_run_id, '活动步骤冲突迁移', 'error',
       '活动运行冲突迁移已终止未完成的运行步骤',
       json_object(
         'migration_version', 5,
         'run_step_id', step.id,
         'original_status', step.status,
         'original_stop_reason', step.stop_reason,
         'original_completed_at', step.completed_at,
         'original_updated_at', step.updated_at
       ),
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM task_run_step AS step
JOIN active_run_conflict_run_v5 AS snapshot ON snapshot.run_id = step.task_run_id
WHERE step.status IN ('pending', 'running');

INSERT INTO task_log (
  id, task_run_id, stage, level, message, safe_details_json, created_at
)
SELECT lower(hex(randomblob(16))), step.task_run_id, '请求检查点冲突迁移', 'error',
       '活动运行冲突迁移已将 requesting 检查点转为 uncertain',
       json_object(
         'migration_version', 5,
         'checkpoint_id', checkpoint.id,
         'original_status', checkpoint.status,
         'original_retryable', checkpoint.retryable,
         'original_last_error_code_was_null', checkpoint.last_error_code IS NULL,
         'original_last_error_message_was_null', checkpoint.last_error_message IS NULL,
         'original_updated_at', checkpoint.updated_at
       ),
       strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
FROM collection_page_checkpoint AS checkpoint
JOIN task_run_step AS step ON step.id = checkpoint.task_run_step_id
JOIN active_run_conflict_run_v5 AS snapshot ON snapshot.run_id = step.task_run_id
WHERE checkpoint.status = 'requesting';

UPDATE collection_page_checkpoint
SET status = 'uncertain', retryable = 0,
    last_error_code = COALESCE(last_error_code, 'UNCERTAIN_REQUEST_AFTER_CRASH'),
    last_error_message = COALESCE(last_error_message,
      '活动运行冲突迁移发现请求可能已发送，无法确认远端是否计费或返回，禁止自动重发'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE status = 'requesting'
  AND task_run_step_id IN (
    SELECT step.id
    FROM task_run_step AS step
    JOIN active_run_conflict_run_v5 AS snapshot ON snapshot.run_id = step.task_run_id
  );

UPDATE task_run_step AS step
SET status = 'failed',
    stop_reason = CASE WHEN EXISTS (
      SELECT 1
      FROM collection_page_checkpoint AS checkpoint
      JOIN task_run_step AS evidence_step ON evidence_step.id = checkpoint.task_run_step_id
      WHERE evidence_step.task_run_id = step.task_run_id
        AND checkpoint.status = 'uncertain'
    ) THEN 'uncertain_request' ELSE 'terminal_error' END,
    completed_at = COALESCE(completed_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE task_run_id IN (SELECT run_id FROM active_run_conflict_run_v5)
  AND status IN ('pending', 'running');

UPDATE collection_task
SET status = 'waiting_confirmation', confirmed_at = NULL,
    completed_at = NULL, cancelled_at = NULL,
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE id IN (
  SELECT task_id FROM active_run_conflict_task_v5
  WHERE original_task_status <> 'cancelled'
);

UPDATE task_run AS run
SET status = 'failed',
    ended_at = COALESCE(ended_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    current_stage = CASE WHEN EXISTS (
      SELECT 1
      FROM collection_page_checkpoint AS checkpoint
      JOIN task_run_step AS evidence_step ON evidence_step.id = checkpoint.task_run_step_id
      WHERE evidence_step.task_run_id = run.id AND checkpoint.status = 'uncertain'
    ) THEN '请求状态不确定' ELSE '活动运行冲突' END,
    error_code = CASE WHEN EXISTS (
      SELECT 1
      FROM collection_page_checkpoint AS checkpoint
      JOIN task_run_step AS evidence_step ON evidence_step.id = checkpoint.task_run_step_id
      WHERE evidence_step.task_run_id = run.id AND checkpoint.status = 'uncertain'
    ) THEN 'UNCERTAIN_REQUEST_AFTER_CRASH' ELSE 'ACTIVE_RUN_CONFLICT_MIGRATION' END,
    error_message = COALESCE(error_message, CASE WHEN EXISTS (
      SELECT 1
      FROM collection_page_checkpoint AS checkpoint
      JOIN task_run_step AS evidence_step ON evidence_step.id = checkpoint.task_run_step_id
      WHERE evidence_step.task_run_id = run.id AND checkpoint.status = 'uncertain'
    ) THEN '活动运行冲突中包含状态不确定的 TikHub 请求，禁止自动重发'
      ELSE '数据库迁移检测到同一任务存在多个活动运行，已全部停止并要求重新确认' END),
    retryable = 0
WHERE id IN (SELECT run_id FROM active_run_conflict_run_v5);
"#;

const ACTIVE_RUN_INDEX_SQL: &str = r#"
CREATE UNIQUE INDEX idx_task_run_single_active
ON task_run(task_id)
WHERE status IN ('queued', 'running');
"#;

const DROP_SNAPSHOT_SQL: &str = r#"
DROP TABLE active_run_conflict_run_v5;
DROP TABLE active_run_conflict_task_v5;
"#;

pub(super) fn validate_existing_active_run_migration(connection: &Connection) -> AppResult<()> {
  if columns(connection, "schema_migrations")?.is_empty() {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
  } else if declared_schema_version(connection)?.is_some_and(|version| version >= 5) {
    return Err(workspace_error(
      "数据库迁移 v5 校验失败，工作区版本已升级但活动运行迁移标记缺失",
    ));
  }
  Ok(())
}

pub(super) fn apply_active_run_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    update_workspace_schema_version(&transaction, 5)?;
    ensure_foreign_key_integrity(&transaction)?;
    return transaction.commit().map_err(database_error);
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if !schema_is_current(&transaction)? {
    transaction
      .execute_batch(ACTIVE_RUN_CONFLICT_REPAIR_SQL)
      .map_err(database_error)?;
    if active_conflict_count(&transaction)? != 0 {
      return Err(workspace_error("数据库迁移 v5 未能安全终止重复活动运行"));
    }
    transaction
      .execute_batch(ACTIVE_RUN_INDEX_SQL)
      .map_err(database_error)?;
    transaction
      .execute_batch(DROP_SNAPSHOT_SQL)
      .map_err(database_error)?;
  }
  if !schema_is_current(&transaction)? {
    return Err(workspace_error("数据库迁移 v5 后活动运行唯一约束校验失败"));
  }
  transaction
    .execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (5, ?1, ?2, ?3)",
      params![
        MIGRATION_NAME,
        Utc::now().to_rfc3339(),
        migration_checksum()
      ],
    )
    .map_err(database_error)?;
  update_workspace_schema_version(&transaction, 5)?;
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
      "数据库迁移 v5 校验失败，活动运行迁移标记或 checksum 不一致",
    ));
  }
  if !schema_is_current(connection)? {
    return Err(workspace_error(
      "数据库迁移 v5 结构校验失败，活动运行唯一约束与标记不一致",
    ));
  }
  Ok(())
}

fn migration_checksum() -> String {
  let mut hasher = Sha256::new();
  hasher.update(ACTIVE_RUN_CONFLICT_REPAIR_SQL.as_bytes());
  hasher.update(ACTIVE_RUN_INDEX_SQL.as_bytes());
  hasher.update(DROP_SNAPSHOT_SQL.as_bytes());
  format!("{:x}", hasher.finalize())
}

fn marker(connection: &Connection) -> AppResult<Option<(String, String)>> {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = 5",
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

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  let expected = "create unique index idx_task_run_single_active on task_run(task_id) where status in ('queued', 'running')";
  Ok(
    object_sql(connection, "index", INDEX_NAME)?.as_deref() == Some(expected)
      && active_conflict_count(connection)? == 0,
  )
}

fn active_conflict_count(connection: &Connection) -> AppResult<i64> {
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
    .map_err(database_error)
}

fn columns(connection: &Connection, table: &str) -> AppResult<Vec<String>> {
  let mut statement = connection
    .prepare(&format!("PRAGMA table_info({table})"))
    .map_err(database_error)?;
  let columns = statement
    .query_map([], |row| row.get(1))
    .map_err(database_error)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;
  Ok(columns)
}

fn object_sql(connection: &Connection, kind: &str, name: &str) -> AppResult<Option<String>> {
  connection
    .query_row(
      "SELECT lower(sql) FROM sqlite_schema WHERE type = ?1 AND name = ?2",
      params![kind, name],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map(|sql| sql.map(|value| value.split_whitespace().collect::<Vec<_>>().join(" ")))
    .map_err(database_error)
}
