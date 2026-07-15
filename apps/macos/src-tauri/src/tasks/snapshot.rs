use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::domain::AppResult;

use super::database_error;

pub(super) fn create_runtime_snapshot(
  connection: &Connection,
  run_id: &str,
  plan_id: &str,
  created_at: &str,
) -> AppResult<bool> {
  let source = connection
    .query_row(
      "SELECT workspace.id, plan.schema_version, plan.plan_json,
              connector.id, connector.config_version, connector.base_url,
              connector.secret_ref_id, connector.last_tested_at,
              connector.last_test_status, secret.provider_type, secret.provider_id,
              secret.credential_revision
       FROM task_run AS run
       JOIN collection_plan AS plan
         ON plan.id = run.plan_id AND plan.task_id = run.task_id
       JOIN workspace ON workspace.id = (
         SELECT id FROM workspace LIMIT 1
       )
       JOIN tikhub_connector AS connector
         ON connector.workspace_id = workspace.id AND connector.id = 'default'
       JOIN secret_ref AS secret ON secret.id = connector.secret_ref_id
       WHERE run.id = ?1 AND run.plan_id = ?2 AND run.status = 'queued'
         AND run.claimed_at IS NULL AND plan.schema_version >= 2
         AND plan.validation_status = 'valid' AND plan.confirmed_by_user = 1
         AND connector.enabled = 1 AND connector.last_tested_at IS NOT NULL
         AND connector.last_test_status = 'success'
         AND secret.provider_type = 'tikhub'",
      params![run_id, plan_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, String>(2)?,
          row.get::<_, String>(3)?,
          row.get::<_, i64>(4)?,
          row.get::<_, String>(5)?,
          row.get::<_, String>(6)?,
          row.get::<_, String>(7)?,
          row.get::<_, String>(8)?,
          row.get::<_, String>(9)?,
          row.get::<_, String>(10)?,
          row.get::<_, i64>(11)?,
        ))
      },
    )
    .optional()
    .map_err(database_error)?;
  let Some((
    workspace_id,
    plan_schema_version,
    plan_json,
    connector_id,
    connector_config_version,
    base_url,
    secret_ref_id,
    connector_tested_at,
    connector_test_status,
    secret_provider_type,
    secret_provider_id,
    secret_revision,
  )) = source
  else {
    return Ok(false);
  };

  connection
    .execute(
      "INSERT INTO collection_runtime_snapshot (
         id, task_run_id, workspace_id, runtime_contract_version,
         plan_id, plan_schema_version, plan_json, connector_type, connector_id,
         connector_config_version, base_url, secret_ref_id, secret_revision,
         secret_provider_type, secret_provider_id, connector_tested_at,
         connector_test_status, created_at
       ) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, 'tikhub', ?7, ?8, ?9, ?10,
                 ?11, ?12, ?13, ?14, ?15, ?16)",
      params![
        Uuid::new_v4().to_string(),
        run_id,
        workspace_id,
        plan_id,
        plan_schema_version,
        plan_json,
        connector_id,
        connector_config_version,
        base_url,
        secret_ref_id,
        secret_revision,
        secret_provider_type,
        secret_provider_id,
        connector_tested_at,
        connector_test_status,
        created_at
      ],
    )
    .map_err(database_error)?;
  Ok(true)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::tasks::{
    confirm_collection_plan, create_collection_task, enqueue_task, save_collection_plan,
    CreateCollectionTaskInput, SaveCollectionPlanInput,
  };
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};
  use chrono::Utc;
  use serde_json::json;
  use uuid::Uuid;

  #[test]
  fn enqueue_captures_the_current_connector_and_secret_revisions() {
    let root = std::env::temp_dir().join(format!("runtime-snapshot-{}", Uuid::new_v4()));
    create_workspace("运行时快照测试", &root).expect("workspace should be created");
    let task = create_collection_task(
      &root,
      CreateCollectionTaskInput {
        name: "快照任务".to_string(),
        source_type: "form".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["item_detail".to_string()],
      },
    )
    .expect("task should be created");
    let plan = save_collection_plan(
      &root,
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: "form_generated".to_string(),
        plan_json: json!({
          "platforms": ["tiktok"],
          "data_types": ["item_detail"],
          "region": null,
          "time_range": null,
          "steps": [{
            "endpoint_key": "tiktok.item_detail",
            "platform": "tiktok",
            "data_type": "item_detail",
            "params": {"item_id": "video-1"}
          }],
          "record_limit": 1,
          "request_limit": 1,
          "budget_limit": {"currency": "USD", "amount_micros": 35000000},
          "missing_fields": [],
          "requires_user_confirmation": true
        }),
        validation_status: "valid".to_string(),
        validation_errors_json: None,
        cost_estimate_json: None,
      },
    )
    .expect("plan should be saved");
    confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should be confirmed");
    insert_ready_connector(&root);

    enqueue_task(&root, &task.id).expect("task should be queued");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let snapshot: (i64, String, i64, String, i64) = connection
      .query_row(
        "SELECT plan_schema_version, connector_id, connector_config_version,
                secret_ref_id, secret_revision
         FROM collection_runtime_snapshot",
        [],
        |row| {
          Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
          ))
        },
      )
      .expect("runtime snapshot should be created");
    assert_eq!(
      snapshot,
      (2, "default".to_string(), 1, "secret-1".to_string(), 1)
    );
    std::fs::remove_dir_all(root).ok();
  }

  fn insert_ready_connector(root: &std::path::Path) {
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let now = Utc::now().to_rfc3339();
    connection
      .execute(
        "INSERT INTO secret_ref (
           id, provider_type, provider_id, secret_store_key, masked_hint,
           created_at, updated_at
         ) VALUES ('secret-1', 'tikhub', 'default', 'test-store-key', '[REDACTED]', ?1, ?1)",
        params![now],
      )
      .expect("secret metadata should be inserted");
    let workspace_id: String = connection
      .query_row("SELECT id FROM workspace", [], |row| row.get(0))
      .expect("workspace should be readable");
    connection
      .execute(
        "INSERT INTO tikhub_connector (
           id, workspace_id, secret_ref_id, base_url, enabled, config_version,
           last_tested_at, last_test_status, created_at, updated_at
         ) VALUES ('default', ?1, 'secret-1', 'https://api.tikhub.dev', 1, 1,
                   ?2, 'success', ?2, ?2)",
        params![workspace_id, now],
      )
      .expect("connector should be ready");
  }
}
