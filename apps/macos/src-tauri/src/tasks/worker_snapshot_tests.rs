use std::path::Path;

use rusqlite::params;
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::tasks::{
  confirm_collection_plan, create_collection_task, enqueue_task, save_collection_plan,
  CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

#[test]
fn worker_rejects_a_connector_changed_after_enqueue() {
  let root = std::env::temp_dir().join(format!("worker-snapshot-stale-{}", Uuid::new_v4()));
  create_workspace("快照过期测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "快照过期任务".to_string(),
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
  connection
    .execute(
      "UPDATE tikhub_connector SET config_version = 2 WHERE id = 'default'",
      [],
    )
    .expect("connector should be changed");

  let run = execute_next_task(&root)
    .expect("worker should record the stale snapshot failure")
    .expect("worker should claim the queued run");
  assert_eq!(
    run.error_code.as_deref(),
    Some("RUNTIME_SNAPSHOT_NOT_READY")
  );
  assert!(!run.retryable);
  std::fs::remove_dir_all(root).ok();
}

fn insert_ready_connector(root: &Path) {
  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  let now = "2026-07-15T00:00:00+00:00";
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
