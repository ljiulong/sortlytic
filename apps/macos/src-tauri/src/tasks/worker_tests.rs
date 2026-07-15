use super::*;
use crate::tasks::{
  claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task, retry_task,
  save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::create_workspace;
use serde_json::json;
use uuid::Uuid;

#[test]
fn worker_tick_does_not_leave_a_queued_task_unprocessed() {
  let root = std::env::temp_dir().join(format!("worker-{}", Uuid::new_v4()));
  create_workspace("执行器测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "无连接器任务".to_string(),
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

  let run = execute_next_task(&root)
    .expect("worker tick should complete its state transition")
    .expect("worker should claim the queued task");

  assert_eq!(run.status, "failed");
  assert_eq!(
    run.error_code.as_deref(),
    Some("RUNTIME_SNAPSHOT_NOT_READY")
  );
  assert!(run.retryable);
  let retry =
    retry_task(&root, &task.id, None).expect("connector setup failure should be retryable");
  assert_eq!(retry.status, "queued");
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_persists_a_page_and_completes_the_run() {
  let root = std::env::temp_dir().join(format!("worker-success-{}", Uuid::new_v4()));
  create_workspace("执行器成功测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "单页任务".to_string(),
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

  execute_claimed_run_with_fetcher(&root, &run, |request| {
    assert!(request.idempotency_key().is_some());
    Ok(CollectionPage {
      records: vec![json!({"aweme_id": "video-1", "desc": "test"})],
      next_cursor: None,
      has_more: false,
      raw_response: json!({
        "code": 200,
        "data": {"aweme_id": "video-1", "desc": "test"}
      }),
    })
  })
  .expect("page should execute");
  let completed = complete_task_run(&root, &run.id, Value::Null)
    .expect("run should complete from checkpoint evidence");

  assert_eq!(completed.status, "success");
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let checkpoint: (String, i64, i64) = connection
    .query_row(
      "SELECT status, record_count_received, record_count_persisted
         FROM collection_page_checkpoint",
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("checkpoint should be persisted");
  assert_eq!(checkpoint, ("completed".to_string(), 1, 1));
  let task_status: String = connection
    .query_row(
      "SELECT status FROM collection_task WHERE id = ?1",
      [&task.id],
      |row| row.get(0),
    )
    .expect("task should be readable");
  assert_eq!(task_status, "success");
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_marks_checkpoint_uncertain_when_record_persistence_fails() {
  let root = std::env::temp_dir().join(format!("worker-persist-failure-{}", Uuid::new_v4()));
  create_workspace("执行器落库失败测试", &root).expect("workspace should be created");
  let (task, plan) = create_confirmed_item_detail_task(&root);
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");

  execute_claimed_run_with_fetcher(&root, &run, |_request| {
    Ok(CollectionPage {
      records: vec![json!({"desc": "missing id"})],
      next_cursor: None,
      has_more: false,
      raw_response: json!({
        "code": 200,
        "data": {"desc": "missing id"}
      }),
    })
  })
  .expect_err("invalid records must fail the worker");

  let connection = super::open_workspace_connection(&root).expect("database should open");
  let checkpoint_status: String = connection
    .query_row(
      "SELECT status FROM collection_page_checkpoint
         WHERE task_run_step_id IN (SELECT id FROM task_run_step WHERE task_run_id = ?1)",
      [&run.id],
      |row| row.get(0),
    )
    .expect("checkpoint should be persisted");
  assert_eq!(checkpoint_status, "uncertain");
  let _ = plan;
  std::fs::remove_dir_all(root).ok();
}

fn create_confirmed_item_detail_task(
  root: &std::path::Path,
) -> (
  crate::tasks::CollectionTaskView,
  crate::tasks::CollectionPlanView,
) {
  let task = create_collection_task(
    root,
    CreateCollectionTaskInput {
      name: "单页任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["item_detail".to_string()],
    },
  )
  .expect("task should be created");
  let plan = save_collection_plan(
    root,
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
  confirm_collection_plan(root, &task.id, &plan.id).expect("plan should be confirmed");
  (task, plan)
}
