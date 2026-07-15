use std::path::Path;

use rusqlite::{params, OptionalExtension};

use crate::domain::AppResult;
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

use super::super::database_error;
use super::worker_error;

pub(super) struct RuntimeSnapshot {
  pub(super) base_url: String,
  pub(super) secret_ref_id: String,
}

pub(super) fn load_runtime_snapshot(
  root_path: impl AsRef<Path>,
  run_id: &str,
) -> AppResult<RuntimeSnapshot> {
  let connection = open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))?;
  let snapshot = connection
    .query_row(
      "SELECT snapshot.base_url, snapshot.secret_ref_id
       FROM collection_runtime_snapshot AS snapshot
       JOIN task_run AS run ON run.id = snapshot.task_run_id
         AND run.plan_id = snapshot.plan_id AND run.status = 'running'
       JOIN collection_plan AS plan ON plan.id = snapshot.plan_id
         AND plan.task_id = run.task_id
         AND plan.schema_version = snapshot.plan_schema_version
         AND plan.plan_json = snapshot.plan_json
         AND plan.validation_status = 'valid'
         AND plan.confirmed_by_user = 1
       JOIN tikhub_connector AS connector
         ON connector.id = snapshot.connector_id
        AND connector.workspace_id = snapshot.workspace_id
       JOIN secret_ref AS secret ON secret.id = snapshot.secret_ref_id
       WHERE snapshot.task_run_id = ?1
         AND connector.enabled = 1
         AND connector.config_version = snapshot.connector_config_version
         AND connector.base_url = snapshot.base_url
         AND connector.secret_ref_id = snapshot.secret_ref_id
         AND connector.last_tested_at = snapshot.connector_tested_at
         AND connector.last_test_status = snapshot.connector_test_status
         AND secret.credential_revision = snapshot.secret_revision
         AND secret.provider_type = snapshot.secret_provider_type
         AND secret.provider_id = snapshot.secret_provider_id
         AND snapshot.connector_type = 'tikhub'
         AND snapshot.secret_provider_type = 'tikhub'",
      params![run_id],
      |row| {
        Ok(RuntimeSnapshot {
          base_url: row.get(0)?,
          secret_ref_id: row.get(1)?,
        })
      },
    )
    .optional()
    .map_err(database_error)?;
  if let Some(snapshot) = snapshot {
    return Ok(snapshot);
  }

  let has_snapshot = connection
    .query_row(
      "SELECT EXISTS(
         SELECT 1 FROM collection_runtime_snapshot WHERE task_run_id = ?1
       )",
      params![run_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?
    == 1;
  Err(worker_error(
    "RUNTIME_SNAPSHOT_NOT_READY",
    "运行时快照缺失或已与当前连接器、密钥版本不一致",
    !has_snapshot,
  ))
}
