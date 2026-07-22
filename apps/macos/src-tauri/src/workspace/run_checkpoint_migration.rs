use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_NAME: &str = "run_checkpoint";

const RUN_CHECKPOINT_MIGRATION_SQL: &str = r#"
ALTER TABLE task_run ADD COLUMN plan_id TEXT REFERENCES collection_plan(id) ON DELETE RESTRICT;
ALTER TABLE task_run ADD COLUMN attempt_number INTEGER NOT NULL DEFAULT 1
  CHECK (attempt_number >= 1);
ALTER TABLE task_run ADD COLUMN claimed_at TEXT;

UPDATE task_run
SET plan_id = (
  SELECT plan.id FROM collection_plan AS plan
  WHERE plan.task_id = task_run.task_id AND plan.created_at <= task_run.started_at
  ORDER BY plan.created_at DESC, plan.id DESC LIMIT 1
);

UPDATE task_run
SET attempt_number = (
  SELECT COUNT(*) FROM task_run AS earlier
  WHERE earlier.task_id = task_run.task_id AND earlier.plan_id = task_run.plan_id
    AND (earlier.started_at < task_run.started_at
      OR (earlier.started_at = task_run.started_at AND earlier.id <= task_run.id))
)
WHERE plan_id IS NOT NULL;

UPDATE task_run
SET status = 'failed', ended_at = COALESCE(ended_at, started_at),
  current_stage = '需要重新确认计划', error_code = 'PLAN_RECONFIRMATION_REQUIRED',
  error_message = '历史运行缺少可确认的采集计划，请重新确认后再执行', retryable = 0
WHERE plan_id IS NULL AND status IN ('queued', 'running');
UPDATE collection_task
SET status = 'failed'
WHERE status IN ('queued', 'running')
  AND EXISTS (SELECT 1 FROM task_run AS run WHERE run.task_id = collection_task.id
    AND run.plan_id IS NULL AND run.error_code = 'PLAN_RECONFIRMATION_REQUIRED')
  AND NOT EXISTS (SELECT 1 FROM task_run AS run WHERE run.task_id = collection_task.id
    AND run.plan_id IS NOT NULL AND run.status IN ('queued', 'running'));

CREATE UNIQUE INDEX idx_task_run_plan_attempt
ON task_run(task_id, plan_id, attempt_number) WHERE plan_id IS NOT NULL;
CREATE UNIQUE INDEX idx_api_call_step_plan_order ON api_call_step(plan_id, step_order);

CREATE TABLE task_run_step (
  id TEXT PRIMARY KEY, task_run_id TEXT NOT NULL, api_call_step_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('pending','running','success','failed','cancelled')),
  stop_reason TEXT CHECK (stop_reason IS NULL OR stop_reason IN (
    'provider_exhausted','request_limit','record_limit','budget_limit','user_cancelled',
    'terminal_error','uncertain_request')),
  started_at TEXT, completed_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  UNIQUE (task_run_id, api_call_step_id),
  FOREIGN KEY (task_run_id) REFERENCES task_run(id) ON DELETE CASCADE,
  FOREIGN KEY (api_call_step_id) REFERENCES api_call_step(id) ON DELETE CASCADE
);

CREATE TABLE collection_page_checkpoint (
  id TEXT PRIMARY KEY, task_run_step_id TEXT NOT NULL,
  page_index INTEGER NOT NULL CHECK (page_index >= 0), idempotency_key TEXT NOT NULL,
  input_cursor_json TEXT,
  status TEXT NOT NULL CHECK (status IN (
    'prepared','requesting','response_received','completed','failed','uncertain')),
  request_attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (request_attempt_count >= 0),
  retry_count INTEGER NOT NULL DEFAULT 0 CHECK (retry_count >= 0),
  fallback_count INTEGER NOT NULL DEFAULT 0 CHECK (fallback_count >= 0),
  final_endpoint_key TEXT, provider_response_json TEXT, provider_response_hash TEXT,
  provider_response_size INTEGER CHECK (provider_response_size IS NULL OR provider_response_size >= 0),
  has_more INTEGER CHECK (has_more IS NULL OR has_more IN (0, 1)), next_cursor_json TEXT,
  record_count_received INTEGER NOT NULL DEFAULT 0 CHECK (record_count_received >= 0),
  record_count_persisted INTEGER NOT NULL DEFAULT 0
    CHECK (record_count_persisted >= 0 AND record_count_persisted <= record_count_received),
  cost_actual_json TEXT NOT NULL DEFAULT '{}', last_error_code TEXT, last_error_message TEXT,
  retryable INTEGER NOT NULL DEFAULT 0 CHECK (retryable IN (0, 1)),
  requested_at TEXT, response_received_at TEXT, committed_at TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  UNIQUE (task_run_step_id, page_index), UNIQUE (idempotency_key),
  FOREIGN KEY (task_run_step_id) REFERENCES task_run_step(id) ON DELETE CASCADE
);
CREATE INDEX idx_collection_page_checkpoint_status ON collection_page_checkpoint(status);

CREATE TRIGGER trg_task_run_plan_insert BEFORE INSERT ON task_run
WHEN NEW.plan_id IS NOT NULL AND NOT EXISTS (
  SELECT 1 FROM collection_plan WHERE id = NEW.plan_id AND task_id = NEW.task_id)
BEGIN SELECT RAISE(ABORT, 'task_run plan/task mismatch'); END;
CREATE TRIGGER trg_task_run_plan_update BEFORE UPDATE OF plan_id, task_id ON task_run
WHEN NEW.plan_id IS NOT NULL AND NOT EXISTS (
  SELECT 1 FROM collection_plan WHERE id = NEW.plan_id AND task_id = NEW.task_id)
BEGIN SELECT RAISE(ABORT, 'task_run plan/task mismatch'); END;
CREATE TRIGGER trg_task_run_plan_step_update BEFORE UPDATE OF plan_id, task_id ON task_run
WHEN EXISTS (
  SELECT 1 FROM task_run_step AS run_step JOIN api_call_step AS step
    ON step.id = run_step.api_call_step_id
  WHERE run_step.task_run_id = OLD.id AND (NEW.plan_id IS NULL OR step.plan_id <> NEW.plan_id))
BEGIN SELECT RAISE(ABORT, 'task_run plan conflicts with run steps'); END;
CREATE TRIGGER trg_task_run_step_plan_insert BEFORE INSERT ON task_run_step
WHEN NOT EXISTS (
  SELECT 1 FROM task_run AS run JOIN api_call_step AS step
  WHERE run.id = NEW.task_run_id AND step.id = NEW.api_call_step_id
    AND run.plan_id IS NOT NULL AND run.plan_id = step.plan_id)
BEGIN SELECT RAISE(ABORT, 'task_run_step plan mismatch'); END;
CREATE TRIGGER trg_task_run_step_plan_update
BEFORE UPDATE OF task_run_id, api_call_step_id ON task_run_step
WHEN NOT EXISTS (
  SELECT 1 FROM task_run AS run JOIN api_call_step AS step
  WHERE run.id = NEW.task_run_id AND step.id = NEW.api_call_step_id
    AND run.plan_id IS NOT NULL AND run.plan_id = step.plan_id)
BEGIN SELECT RAISE(ABORT, 'task_run_step plan mismatch'); END;
CREATE TRIGGER trg_api_call_step_run_guard_update BEFORE UPDATE OF plan_id ON api_call_step
WHEN EXISTS (
  SELECT 1 FROM task_run_step AS run_step JOIN task_run AS run
    ON run.id = run_step.task_run_id
  WHERE run_step.api_call_step_id = OLD.id AND run.plan_id <> NEW.plan_id)
BEGIN SELECT RAISE(ABORT, 'api_call_step plan conflicts with run steps'); END;
"#;

pub(super) fn validate_existing_run_checkpoint_migration(connection: &Connection) -> AppResult<()> {
  if columns(connection, "schema_migrations")?.is_empty() {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
  }
  Ok(())
}

pub(super) fn apply_run_checkpoint_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker_and_schema(connection, &name, &checksum)?;
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    update_workspace_schema_version(&transaction, 4)?;
    ensure_foreign_key_integrity(&transaction)?;
    return transaction.commit().map_err(database_error);
  }

  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  if !schema_is_current(&transaction)? {
    transaction
      .execute_batch(RUN_CHECKPOINT_MIGRATION_SQL)
      .map_err(database_error)?;
  }
  if !schema_is_current(&transaction)? {
    return Err(workspace_error("数据库迁移 v4 后运行检查点结构校验失败"));
  }
  transaction
    .execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (4, ?1, ?2, ?3)",
      params![
        MIGRATION_NAME,
        Utc::now().to_rfc3339(),
        migration_checksum()
      ],
    )
    .map_err(database_error)?;
  update_workspace_schema_version(&transaction, 4)?;
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
      "数据库迁移 v4 校验失败，运行检查点标记或 checksum 不一致",
    ));
  }
  if !schema_is_current(connection)? {
    return Err(workspace_error(
      "数据库迁移 v4 结构校验失败，运行检查点结构与标记不一致",
    ));
  }
  Ok(())
}

fn migration_checksum() -> String {
  let mut hasher = Sha256::new();
  hasher.update(RUN_CHECKPOINT_MIGRATION_SQL.as_bytes());
  format!("{:x}", hasher.finalize())
}

fn marker(connection: &Connection) -> AppResult<Option<(String, String)>> {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = 4",
      [],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(database_error)
}

fn schema_is_current(connection: &Connection) -> AppResult<bool> {
  if !columns(connection, "task_run")?.join(",").starts_with(
    "id,task_id,status,started_at,ended_at,current_stage,error_code,error_message,retryable,cost_actual_json,plan_id,attempt_number,claimed_at",
  ) || !columns(connection, "task_run_step")?.join(",").starts_with(
    "id,task_run_id,api_call_step_id,status,stop_reason,started_at,completed_at,created_at,updated_at",
  ) || !columns(connection, "collection_page_checkpoint")?
    .join(",")
    .starts_with(
      "id,task_run_step_id,page_index,idempotency_key,input_cursor_json,status,request_attempt_count,retry_count,fallback_count,final_endpoint_key,provider_response_json,provider_response_hash,provider_response_size,has_more,next_cursor_json,record_count_received,record_count_persisted,cost_actual_json,last_error_code,last_error_message,retryable,requested_at,response_received_at,committed_at,created_at,updated_at",
    )
  {
    return Ok(false);
  }
  for requirement in [
    "table|task_run|plan_id text references collection_plan(id) on delete restrict",
    "table|task_run|check (attempt_number >= 1)",
    "table|task_run_step|foreign key (api_call_step_id) references api_call_step(id) on delete cascade",
    "table|task_run_step|foreign key (task_run_id) references task_run(id) on delete cascade",
    "table|task_run_step|check (status in ('pending','running','success','failed','cancelled'))",
    "table|task_run_step|stop_reason text check (stop_reason is null or stop_reason in ( 'provider_exhausted','request_limit','record_limit','budget_limit','user_cancelled', 'terminal_error','uncertain_request'))",
    "table|task_run_step|unique (task_run_id, api_call_step_id)",
    "table|collection_page_checkpoint|record_count_persisted <= record_count_received",
    "table|collection_page_checkpoint|foreign key (task_run_step_id) references task_run_step(id) on delete cascade",
    "table|collection_page_checkpoint|check (status in ( 'prepared','requesting','response_received','completed','failed','uncertain'))",
    "table|collection_page_checkpoint|unique (task_run_step_id, page_index)",
    "table|collection_page_checkpoint|unique (idempotency_key)",
    "table|collection_page_checkpoint|has_more integer check (has_more is null or has_more in (0, 1))",
    "table|collection_page_checkpoint|retryable integer not null default 0 check (retryable in (0, 1))",
    "table|collection_page_checkpoint|page_index integer not null check (page_index >= 0)",
    "table|collection_page_checkpoint|request_attempt_count integer not null default 0 check (request_attempt_count >= 0)",
    "table|collection_page_checkpoint|record_count_received integer not null default 0 check (record_count_received >= 0)",
    "index|idx_task_run_plan_attempt|where plan_id is not null",
    "index|idx_api_call_step_plan_order|api_call_step(plan_id, step_order)",
    "index|idx_collection_page_checkpoint_status|collection_page_checkpoint(status)",
    "trigger|trg_task_run_plan_insert|task_run plan/task mismatch",
    "trigger|trg_task_run_plan_update|task_run plan/task mismatch",
    "trigger|trg_task_run_plan_step_update|task_run plan conflicts with run steps",
    "trigger|trg_task_run_step_plan_insert|task_run_step plan mismatch",
    "trigger|trg_task_run_step_plan_update|task_run_step plan mismatch",
    "trigger|trg_api_call_step_run_guard_update|api_call_step plan conflicts with run steps",
  ] {
    let mut parts = requirement.splitn(3, '|');
    let kind = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    let fragment = parts.next().unwrap_or_default();
    if !object_sql(connection, kind, name)?.is_some_and(|sql| sql.contains(fragment)) {
      return Ok(false);
    }
  }
  Ok(true)
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
  let sql = connection
    .query_row(
      "SELECT lower(sql) FROM sqlite_schema WHERE type = ?1 AND name = ?2",
      params![kind, name],
      |row| row.get(0),
    )
    .optional()
    .map_err(database_error)?;
  Ok(sql.map(|sql: String| sql.split_whitespace().collect::<Vec<_>>().join(" ")))
}

#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::PathBuf;

  use rusqlite::{params, Connection};
  use uuid::Uuid;

  use super::super::*;

  const T0: &str = "2026-07-13T08:00:00+00:00";
  const T1: &str = "2026-07-13T09:00:00+00:00";
  const T2: &str = "2026-07-13T10:00:00+00:00";

  #[test]
  fn fresh_workspace_has_v4_schema_marker_and_checkpoint_constraints() {
    let (root, schema_version, connection) = fresh_workspace("v4-fresh");

    assert_eq!(schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(marker(&connection, 4).0, "run_checkpoint");
    assert_eq!(
      marker(&connection, 4).1,
      "c3cbee44d66fb7c0b95d2591dcd980d2493435139d9d05a7a629ab90534aba0a"
    );
    assert_eq!(marker(&connection, 3).0, "tikhub_connector");
    assert_eq!(
      columns(&connection, "task_run").last().map(String::as_str),
      Some("run_sequence")
    );
    for table in ["task_run_step", "collection_page_checkpoint"] {
      assert_eq!(object_count(&connection, "table", table), 1);
    }
    for index in [
      "idx_task_run_plan_attempt",
      "idx_api_call_step_plan_order",
      "idx_collection_page_checkpoint_status",
    ] {
      assert_eq!(object_count(&connection, "index", index), 1);
    }
    for trigger in [
      "trg_task_run_plan_insert",
      "trg_task_run_plan_update",
      "trg_task_run_plan_step_update",
      "trg_task_run_step_plan_insert",
      "trg_task_run_step_plan_update",
      "trg_api_call_step_run_guard_update",
    ] {
      assert_eq!(object_count(&connection, "trigger", trigger), 1);
    }

    insert_task(&connection, "task-a");
    insert_plan(&connection, "plan-a", "task-a", T0);
    insert_step(&connection, "step-a", "plan-a", 1);
    insert_bound_run(&connection, "run-a", "task-a", "plan-a", T1);
    insert_run_step(&connection, "run-step-a", "run-a", "step-a");
    connection
      .execute(
        "INSERT INTO collection_page_checkpoint (
          id, task_run_step_id, page_index, idempotency_key, status, created_at, updated_at
        ) VALUES ('page-a', 'run-step-a', 0, 'idem-a', 'prepared', ?1, ?1)",
        params![T1],
      )
      .expect("valid checkpoint should insert");

    for invalid_sql in [
      "UPDATE collection_page_checkpoint SET status = 'unknown' WHERE id = 'page-a'",
      "UPDATE collection_page_checkpoint SET retry_count = -1 WHERE id = 'page-a'",
      "UPDATE collection_page_checkpoint SET has_more = 2 WHERE id = 'page-a'",
      "UPDATE collection_page_checkpoint SET record_count_received = 1, record_count_persisted = 2 WHERE id = 'page-a'",
      "UPDATE task_run SET attempt_number = 0 WHERE id = 'run-a'",
      "UPDATE task_run_step SET stop_reason = 'unknown' WHERE id = 'run-step-a'",
      "INSERT INTO collection_page_checkpoint (id,task_run_step_id,page_index,idempotency_key,status,created_at,updated_at) VALUES ('page-b','run-step-a',0,'idem-b','prepared','2026-07-13','2026-07-13')",
      "INSERT INTO collection_page_checkpoint (id,task_run_step_id,page_index,idempotency_key,status,created_at,updated_at) VALUES ('page-c','run-step-a',1,'idem-a','prepared','2026-07-13','2026-07-13')",
      "INSERT INTO api_call_step (id,plan_id,step_order,platform,data_type,endpoint_key,status,created_at,updated_at) VALUES ('step-duplicate','plan-a',1,'tiktok','comments','tiktok.comments','pending','2026-07-13','2026-07-13')",
    ] {
      assert!(connection.execute_batch(invalid_sql).is_err(), "{invalid_sql}");
    }

    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn v3_migration_binds_latest_prior_plan_and_preserves_logs() {
    let (root, _, connection) = fresh_workspace("v4-plan-backfill");
    downgrade_to_v3(&connection);
    insert_task(&connection, "task-a");
    insert_plan(&connection, "plan-old", "task-a", T0);
    insert_plan(&connection, "plan-new", "task-a", T1);
    insert_step(&connection, "step-new", "plan-new", 1);
    insert_legacy_run(&connection, "run-a", "task-a", "queued", T2);
    connection
      .execute(
        "INSERT INTO task_log (
          id, task_run_id, stage, level, message, created_at
        ) VALUES ('log-a', 'run-a', 'queue', 'info', 'preserve', ?1)",
        params![T2],
      )
      .expect("legacy log should insert");
    drop(connection);

    let summary = open_workspace(&root).expect("v3 workspace should migrate");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("migrated database should open");
    let migrated = connection
      .query_row(
        "SELECT plan_id, attempt_number, status FROM task_run WHERE id = 'run-a'",
        [],
        |row| {
          Ok((
            row.get::<_, Option<String>>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
          ))
        },
      )
      .expect("migrated run should load");

    assert_eq!(summary.schema_version, CURRENT_SCHEMA_VERSION);
    assert_eq!(
      migrated,
      (Some("plan-new".to_string()), 1, "queued".to_string())
    );
    assert_eq!(object_count(&connection, "table", "task_log"), 1);
    assert_eq!(row_count(&connection, "task_log"), 1);
    assert_eq!(marker(&connection, 2).0, "record_observations");
    assert_eq!(marker(&connection, 3).0, "tikhub_connector");
    assert_eq!(marker(&connection, 4).0, "run_checkpoint");
    assert_eq!(foreign_key_violations(&connection), 0);
    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn v3_migration_numbers_attempts_by_start_time_and_id() {
    let (root, _, connection) = fresh_workspace("v4-attempt-backfill");
    downgrade_to_v3(&connection);
    insert_task(&connection, "task-a");
    insert_plan(&connection, "plan-a", "task-a", T0);
    insert_legacy_run(&connection, "run-b", "task-a", "failed", T1);
    insert_legacy_run(&connection, "run-a", "task-a", "failed", T1);
    insert_legacy_run(&connection, "run-c", "task-a", "success", T2);
    drop(connection);

    open_workspace(&root).expect("v3 workspace should migrate");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("migrated database should open");
    let attempts = ["run-a", "run-b", "run-c"].map(|run_id| {
      connection
        .query_row(
          "SELECT attempt_number FROM task_run WHERE id = ?1",
          params![run_id],
          |row| row.get::<_, i64>(0),
        )
        .expect("attempt should load")
    });

    assert_eq!(attempts, [1, 2, 3]);
    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn v3_migration_closes_active_unbound_runs_but_preserves_terminal_history() {
    let (root, _, connection) = fresh_workspace("v4-unbound-runs");
    downgrade_to_v3(&connection);
    insert_task(&connection, "task-a");
    insert_task(&connection, "task-b");
    insert_plan(&connection, "plan-b", "task-b", T1);
    connection
      .execute("UPDATE collection_task SET status = 'running'", [])
      .expect("legacy tasks should be active");
    insert_legacy_run(&connection, "run-queued", "task-a", "queued", T1);
    insert_legacy_run(&connection, "run-running", "task-a", "running", T1);
    insert_legacy_run(&connection, "run-success", "task-a", "success", T0);
    insert_legacy_run(&connection, "run-b-unbound", "task-b", "running", T0);
    insert_legacy_run(&connection, "run-b-bound", "task-b", "running", T2);
    drop(connection);

    open_workspace(&root).expect("v3 workspace should migrate");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("migrated database should open");
    for run_id in ["run-queued", "run-running"] {
      let state = connection
        .query_row(
          "SELECT plan_id, status, retryable, error_code, ended_at
           FROM task_run WHERE id = ?1",
          params![run_id],
          |row| {
            Ok((
              row.get::<_, Option<String>>(0)?,
              row.get::<_, String>(1)?,
              row.get::<_, i64>(2)?,
              row.get::<_, Option<String>>(3)?,
              row.get::<_, Option<String>>(4)?,
            ))
          },
        )
        .expect("closed run should load");
      assert_eq!(state.0, None);
      assert_eq!((state.1.as_str(), state.2), ("failed", 0));
      assert_eq!(state.3.as_deref(), Some("PLAN_RECONFIRMATION_REQUIRED"));
      assert!(state.4.is_some());
    }
    let terminal = connection
      .query_row(
        "SELECT plan_id, status FROM task_run WHERE id = 'run-success'",
        [],
        |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
      )
      .expect("terminal run should load");
    assert_eq!(terminal, (None, "success".to_string()));
    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM collection_task
           WHERE (id = 'task-a' AND status = 'failed')
             OR (id = 'task-b' AND status = 'running')",
          [],
          |row| row.get::<_, i64>(0)
        )
        .expect("task states should load"),
      2
    );
    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn v4_triggers_reject_cross_scope_steps_but_allow_null_legacy_runs() {
    let (root, _, connection) = fresh_workspace("v4-trigger-scope");
    insert_task(&connection, "task-a");
    insert_task(&connection, "task-b");
    insert_plan(&connection, "plan-a", "task-a", T0);
    insert_plan(&connection, "plan-a2", "task-a", T1);
    insert_plan(&connection, "plan-b", "task-b", T0);
    insert_step(&connection, "step-a", "plan-a", 1);
    insert_step(&connection, "step-a2", "plan-a2", 1);
    insert_step(&connection, "step-b", "plan-b", 1);

    connection
      .execute_batch(
        "INSERT INTO task_run (id,task_id,status,started_at)
         VALUES ('run-legacy','task-a','failed','2026-07-13');",
      )
      .expect("nullable legacy run should remain compatible");
    insert_bound_run(&connection, "run-a", "task-a", "plan-a", T2);
    insert_run_step(&connection, "run-step-a", "run-a", "step-a");
    for invalid_sql in [
      "INSERT INTO task_run (id,task_id,plan_id,status,started_at) VALUES ('run-cross','task-a','plan-b','queued','2026-07-13')",
      "INSERT INTO task_run_step (id,task_run_id,api_call_step_id,status,created_at,updated_at) VALUES ('run-step-legacy','run-legacy','step-a','pending','2026-07-13','2026-07-13')",
      "INSERT INTO task_run_step (id,task_run_id,api_call_step_id,status,created_at,updated_at) VALUES ('run-step-cross','run-a','step-b','pending','2026-07-13','2026-07-13')",
      "UPDATE task_run_step SET api_call_step_id = 'step-a2' WHERE id = 'run-step-a'",
      "UPDATE task_run SET plan_id = 'plan-a2' WHERE id = 'run-a'",
      "UPDATE task_run SET plan_id = NULL WHERE id = 'run-a'",
      "UPDATE task_run SET task_id = 'task-b' WHERE id = 'run-a'",
    ] {
      assert!(connection.execute_batch(invalid_sql).is_err(), "{invalid_sql}");
    }

    drop(connection);
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn v4_marker_or_structure_damage_is_rejected_before_repair() {
    let (checksum_root, _, connection) = fresh_workspace("v4-bad-checksum");
    connection
      .execute(
        "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 4",
        [],
      )
      .expect("v4 checksum should be corrupted");
    drop(connection);
    let checksum_error =
      open_workspace(&checksum_root).expect_err("corrupted v4 marker must be rejected");
    assert!(checksum_error.message.contains("v4") && checksum_error.message.contains("校验"));
    fs::remove_dir_all(checksum_root).ok();

    let (schema_root, _, connection) = fresh_workspace("v4-missing-table");
    connection
      .execute("DROP TABLE collection_page_checkpoint", [])
      .expect("checkpoint table should be removed");
    drop(connection);
    let schema_error =
      open_workspace(&schema_root).expect_err("missing marked v4 table must be rejected");
    assert!(schema_error.message.contains("v4") && schema_error.message.contains("结构"));
    let connection = Connection::open(schema_root.join(DATABASE_FILE_NAME))
      .expect("damaged database should reopen");
    assert_eq!(
      object_count(&connection, "table", "collection_page_checkpoint"),
      0
    );
    drop(connection);
    fs::remove_dir_all(schema_root).ok();

    let (constraint_root, _, connection) = fresh_workspace("v4-missing-constraints");
    connection
      .execute_batch(
        "DROP TABLE collection_page_checkpoint;
         CREATE TABLE collection_page_checkpoint (
           id TEXT, task_run_step_id TEXT, page_index INTEGER, idempotency_key TEXT,
           input_cursor_json TEXT, status TEXT, request_attempt_count INTEGER, retry_count INTEGER,
           fallback_count INTEGER, final_endpoint_key TEXT, provider_response_json TEXT,
           provider_response_hash TEXT, provider_response_size INTEGER, has_more INTEGER,
           next_cursor_json TEXT, record_count_received INTEGER, record_count_persisted INTEGER,
           cost_actual_json TEXT, last_error_code TEXT, last_error_message TEXT, retryable INTEGER,
           requested_at TEXT, response_received_at TEXT, committed_at TEXT, created_at TEXT,
           updated_at TEXT
         );",
      )
      .expect("same-column table without constraints should replace the checkpoint table");
    drop(connection);
    let constraint_error =
      open_workspace(&constraint_root).expect_err("missing v4 constraints must be rejected");
    assert!(constraint_error.message.contains("v4") && constraint_error.message.contains("结构"));
    fs::remove_dir_all(constraint_root).ok();
  }

  fn fresh_workspace(label: &str) -> (PathBuf, i64, Connection) {
    let root = std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()));
    let workspace = create_workspace("v4 迁移测试", &root).expect("workspace should be created");
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("workspace database should open");
    (root, workspace.schema_version, connection)
  }

  fn insert_task(connection: &Connection, id: &str) {
    connection
      .execute(
        "INSERT INTO collection_task (
          id, name, source_type, status, created_at, updated_at
        ) VALUES (?1, '任务', 'form', 'draft', ?2, ?2)",
        params![id, T0],
      )
      .expect("task should insert");
  }

  fn insert_plan(connection: &Connection, id: &str, task_id: &str, created_at: &str) {
    connection
      .execute(
        "INSERT INTO collection_plan (
          id, task_id, source, schema_version, plan_json, validation_status, created_at, updated_at
        ) VALUES (?1, ?2, 'form', 1, '{}', 'valid', ?3, ?3)",
        params![id, task_id, created_at],
      )
      .expect("plan should insert");
  }

  fn insert_step(connection: &Connection, id: &str, plan_id: &str, order: i64) {
    connection
      .execute(
        "INSERT INTO api_call_step (
          id, plan_id, step_order, platform, data_type, endpoint_key, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, 'tiktok', 'comments', 'tiktok.comments', 'pending', ?4, ?4)",
        params![id, plan_id, order, T0],
      )
      .expect("API step should insert");
  }

  fn insert_legacy_run(connection: &Connection, id: &str, task_id: &str, status: &str, at: &str) {
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, task_id, status, at],
      )
      .expect("legacy run should insert");
  }

  fn insert_bound_run(connection: &Connection, id: &str, task_id: &str, plan_id: &str, at: &str) {
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, plan_id, status, started_at)
         VALUES (?1, ?2, ?3, 'queued', ?4)",
        params![id, task_id, plan_id, at],
      )
      .expect("bound run should insert");
  }

  fn insert_run_step(connection: &Connection, id: &str, run_id: &str, step_id: &str) {
    connection
      .execute(
        "INSERT INTO task_run_step (
          id, task_run_id, api_call_step_id, status, created_at, updated_at
        ) VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
        params![id, run_id, step_id, T1],
      )
      .expect("run step should insert");
  }

  fn downgrade_to_v3(connection: &Connection) {
    if columns(connection, "secret_ref")
      .iter()
      .any(|column| column == "credential_revision")
    {
      connection
        .execute_batch(
          "DROP TRIGGER trg_collection_runtime_snapshot_immutable_delete; DROP TRIGGER trg_collection_runtime_snapshot_immutable_update;
         DROP TRIGGER trg_collection_runtime_snapshot_insert;
         DROP TRIGGER trg_secret_ref_credential_invalidates_connector;
         DROP TRIGGER trg_secret_ref_credential_revision;
         DROP TRIGGER trg_secret_ref_credential_revision_overflow;
         DROP INDEX idx_collection_runtime_snapshot_task_run_id;
         DROP TABLE collection_runtime_snapshot;
         ALTER TABLE secret_ref DROP COLUMN credential_revision;
         DELETE FROM schema_migrations WHERE version = 6;",
        )
        .expect("v6 runtime fixture should be removed");
    }
    if columns(connection, "task_run")
      .iter()
      .any(|column| column == "plan_id")
    {
      connection
        .execute_batch(
          "DROP TABLE IF EXISTS collection_page_checkpoint;
           DROP TABLE IF EXISTS task_run_step;
           DROP INDEX IF EXISTS idx_api_call_step_plan_order;
           DROP TRIGGER IF EXISTS trg_task_run_plan_insert;
           DROP TRIGGER IF EXISTS trg_task_run_plan_update;
           DROP TRIGGER IF EXISTS trg_task_run_plan_step_update;
           DROP TRIGGER IF EXISTS trg_api_call_step_run_guard_update;
           PRAGMA foreign_keys = OFF;
           CREATE TABLE task_run_v3 (
             id TEXT PRIMARY KEY,
             task_id TEXT NOT NULL,
             status TEXT NOT NULL,
             started_at TEXT NOT NULL,
             ended_at TEXT,
             current_stage TEXT,
             error_code TEXT,
             error_message TEXT,
             retryable INTEGER NOT NULL DEFAULT 0,
             cost_actual_json TEXT NOT NULL DEFAULT '{}',
             FOREIGN KEY (task_id) REFERENCES collection_task(id) ON DELETE CASCADE
           );
           INSERT INTO task_run_v3 (
             id, task_id, status, started_at, ended_at, current_stage,
             error_code, error_message, retryable, cost_actual_json
           ) SELECT id, task_id, status, started_at, ended_at, current_stage,
             error_code, error_message, retryable, cost_actual_json FROM task_run;
           DROP TABLE task_run;
           ALTER TABLE task_run_v3 RENAME TO task_run;
           CREATE INDEX idx_task_run_task_id ON task_run(task_id);
           CREATE INDEX idx_task_run_status ON task_run(status);
           PRAGMA foreign_keys = ON;",
        )
        .expect("v4 schema should downgrade to v3");
    }
    connection
      .execute_batch(
        "DELETE FROM schema_migrations WHERE version >= 4;
         UPDATE workspace SET schema_version = 3;",
      )
      .expect("workspace marker should downgrade to v3");
  }

  fn columns(connection: &Connection, table: &str) -> Vec<String> {
    let mut statement = connection
      .prepare(&format!("PRAGMA table_info({table})"))
      .expect("table info should prepare");
    statement
      .query_map([], |row| row.get::<_, String>(1))
      .expect("table info should run")
      .collect::<rusqlite::Result<Vec<_>>>()
      .expect("columns should load")
  }

  fn marker(connection: &Connection, version: i64) -> (String, String) {
    connection
      .query_row(
        "SELECT name, checksum FROM schema_migrations WHERE version = ?1",
        params![version],
        |row| Ok((row.get(0)?, row.get(1)?)),
      )
      .expect("migration marker should exist")
  }

  fn object_count(connection: &Connection, object_type: &str, name: &str) -> i64 {
    connection
      .query_row(
        "SELECT COUNT(*) FROM sqlite_schema WHERE type = ?1 AND name = ?2",
        params![object_type, name],
        |row| row.get(0),
      )
      .expect("object count should load")
  }

  fn row_count(connection: &Connection, table: &str) -> i64 {
    connection
      .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
      })
      .expect("row count should load")
  }

  fn foreign_key_violations(connection: &Connection) -> usize {
    let mut statement = connection
      .prepare("PRAGMA foreign_key_check")
      .expect("foreign key check should prepare");
    statement
      .query_map([], |_| Ok(()))
      .expect("foreign key check should run")
      .count()
  }
}
