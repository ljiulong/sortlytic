use std::path::Path;

use rusqlite::params;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::*;
use crate::tasks::{
  confirm_collection_plan, create_collection_task, enqueue_task, recover_interrupted_runs,
  save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

#[test]
fn runtime_snapshot_remains_valid_after_current_connector_switch() {
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
  let claimed = claim_next_task(&root)
    .expect("task claim should succeed")
    .expect("queued task should be claimed");

  let original = load_runtime_snapshot(&root, &claimed.id)
    .expect("claimed run should load its immutable snapshot");
  assert_eq!(original.base_url, "https://api.tikhub.dev");
  assert_eq!(original.secret_ref_id, "secret-1");

  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  connection
    .execute(
      "UPDATE tikhub_connector SET config_version = 2 WHERE id = 'default'",
      [],
    )
    .expect("connector should be changed");

  let after_switch = load_runtime_snapshot(&root, &claimed.id)
    .expect("running task should keep using its immutable snapshot after a switch");
  assert_eq!(after_switch.base_url, original.base_url);
  assert_eq!(after_switch.secret_ref_id, original.secret_ref_id);
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_rejects_a_page_that_exceeds_record_limit_before_persisting() {
  let root = std::env::temp_dir().join(format!("worker-record-limit-{}", Uuid::new_v4()));
  create_workspace("记录上限测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "记录上限任务".to_string(),
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
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");

  let error = super::execute_claimed_run_with_fetcher(&root, &run, |_request| {
    Ok(crate::tikhub::CollectionPage {
      records: vec![
        json!({"aweme_id": "video-1"}),
        json!({"aweme_id": "video-2"}),
      ],
      next_cursor: None,
      has_more: false,
      raw_response: json!({
        "code": 200,
        "data": {"aweme_id": "video-1"}
      }),
    })
  })
  .expect_err("record limit must be enforced before persistence");
  assert!(error.message.contains("RECORD_LIMIT_REACHED"));

  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  let persisted_records: i64 = connection
    .query_row("SELECT COUNT(*) FROM raw_record", [], |row| row.get(0))
    .expect("raw record count should load");
  assert_eq!(persisted_records, 0);
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_resumes_a_prepared_checkpoint_after_recovery() {
  let root = std::env::temp_dir().join(format!("worker-recovery-prepared-{}", Uuid::new_v4()));
  create_workspace("恢复检查点测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "恢复检查点任务".to_string(),
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
          "params": {"item_id": "video-recovery"}
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
  enqueue_task(&root, &task.id).expect("task should be queued");
  let claimed = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");
  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  let run_step_id: String = connection
    .query_row(
      "SELECT id FROM task_run_step WHERE task_run_id = ?1",
      params![claimed.id],
      |row| row.get(0),
    )
    .expect("run step should exist");
  let checkpoint_id = Uuid::new_v4().to_string();
  let idempotency_key = "recovery-idempotency-key";
  connection
    .execute(
      "UPDATE task_run_step
       SET status = 'running',
           started_at = (SELECT started_at FROM task_run WHERE id = ?2),
           updated_at = (SELECT started_at FROM task_run WHERE id = ?2)
       WHERE id = ?1",
      params![run_step_id, claimed.id],
    )
    .expect("run step should be running before simulated interruption");
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, status,
         created_at, updated_at
       ) VALUES (?1, ?2, 0, ?3, 'prepared',
                 '2026-07-15T00:01:00+00:00', '2026-07-15T00:01:00+00:00')",
      params![checkpoint_id, run_step_id, idempotency_key],
    )
    .expect("prepared checkpoint should exist before simulated interruption");
  drop(connection);

  assert_eq!(
    recover_interrupted_runs(&root).expect("interrupted run should recover"),
    1
  );
  let recovered = claim_next_task(&root)
    .expect("recovered task should be claimable")
    .expect("recovered task should be claimed");
  assert_eq!(recovered.id, claimed.id);

  super::execute_claimed_run_with_fetcher(&root, &recovered, |request| {
    assert_eq!(request.idempotency_key(), Some(idempotency_key));
    Ok(crate::tikhub::CollectionPage {
      records: vec![json!({"aweme_id": "video-recovery"})],
      next_cursor: None,
      has_more: false,
      raw_response: json!({"code": 200, "data": {"aweme_id": "video-recovery"}}),
    })
  })
  .expect("prepared checkpoint should be resumed");
  complete_task_run(&root, &recovered.id, Value::Null).expect("recovered run should complete");

  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should reopen");
  let checkpoint_state: (String, i64) = connection
    .query_row(
      "SELECT status, request_attempt_count
       FROM collection_page_checkpoint WHERE id = ?1",
      params![checkpoint_id],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("checkpoint state should load");
  assert_eq!(checkpoint_state, ("completed".to_string(), 1));
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_persists_a_response_received_checkpoint_after_recovery() {
  let root = std::env::temp_dir().join(format!("worker-recovery-response-{}", Uuid::new_v4()));
  create_workspace("响应恢复测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "响应恢复任务".to_string(),
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
          "params": {"item_id": "video-response-recovery"}
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
  enqueue_task(&root, &task.id).expect("task should be queued");
  let claimed = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");
  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
  let run_step_id: String = connection
    .query_row(
      "SELECT id FROM task_run_step WHERE task_run_id = ?1",
      params![claimed.id],
      |row| row.get(0),
    )
    .expect("run step should exist");
  let (run_started_at, claimed_at): (String, String) = connection
    .query_row(
      "SELECT started_at, claimed_at FROM task_run WHERE id = ?1",
      params![claimed.id],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("run timestamps should exist");
  let raw_response = json!({
    "code": 200,
    "data": {"aweme_id": "video-response-recovery"}
  })
  .to_string();
  let response_hash = format!("{:x}", Sha256::digest(raw_response.as_bytes()));
  let checkpoint_id = Uuid::new_v4().to_string();
  connection
    .execute(
      "UPDATE task_run_step
       SET status = 'running', started_at = ?2, updated_at = ?2
       WHERE id = ?1",
      params![run_step_id, run_started_at],
    )
    .expect("run step should be running before simulated interruption");
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, status,
         request_attempt_count, provider_response_json, provider_response_hash,
         provider_response_size, has_more, record_count_received, record_count_persisted,
         cost_actual_json, requested_at, response_received_at, created_at, updated_at
       ) VALUES (?1, ?2, 0, ?3, 'response_received', 1, ?4, ?5, ?6, 0, 1, 0,
                 '{\"currency\":\"USD\",\"amount_micros\":0}', ?7, ?7, ?7, ?7)",
      params![
        checkpoint_id,
        run_step_id,
        "response-recovery-idempotency-key",
        raw_response,
        response_hash,
        i64::try_from(raw_response.len()).expect("response size should fit"),
        claimed_at
      ],
    )
    .expect("response checkpoint should exist before simulated interruption");
  drop(connection);

  assert_eq!(
    recover_interrupted_runs(&root).expect("interrupted run should recover"),
    1
  );
  let recovered = claim_next_task(&root)
    .expect("response recovery should remain claimable")
    .expect("recovered response should be claimed");
  super::execute_claimed_run_with_fetcher(&root, &recovered, |_request| {
    panic!("response recovery must not send a second remote request")
  })
  .expect("response checkpoint should be persisted locally");
  complete_task_run(&root, &recovered.id, Value::Null).expect("recovered run should complete");

  let connection =
    open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should reopen");
  let checkpoint_state: (String, i64, i64) = connection
    .query_row(
      "SELECT status, request_attempt_count, record_count_persisted
       FROM collection_page_checkpoint WHERE id = ?1",
      params![checkpoint_id],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("checkpoint state should load");
  assert_eq!(checkpoint_state, ("completed".to_string(), 1, 1));
  let raw_records: i64 = connection
    .query_row(
      "SELECT COUNT(*) FROM raw_record WHERE task_run_id = ?1",
      params![recovered.id],
      |row| row.get(0),
    )
    .expect("recovered raw records should load");
  assert_eq!(raw_records, 1);
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
