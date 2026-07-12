use super::*;
use crate::workspace::create_workspace;

#[test]
fn queued_run_can_be_claimed_and_completed_atomically() {
  let root_path = prepared_task_workspace("execution-success");
  let task = list_tasks(&root_path, None)
    .expect("tasks should list")
    .remove(0);
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");

  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let running_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(running.id, queued.id);
  assert_eq!(running.status, "running");
  assert_eq!(running_task.status, "running");

  let completed = complete_task_run(
    &root_path,
    &running.id,
    serde_json::json!({ "request_count": 1 }),
  )
  .expect("running task should complete");
  let completed_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(completed.status, "success");
  assert!(completed.ended_at.is_some());
  assert_eq!(completed_task.status, "success");
  assert!(completed_task.completed_at.is_some());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn failed_run_can_create_a_new_retry_run() {
  let root_path = prepared_task_workspace("execution-retry");
  let task = list_tasks(&root_path, None)
    .expect("tasks should list")
    .remove(0);
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");

  let failed = fail_task_run(
    &root_path,
    &running.id,
    "TIKHUB_REQUEST_ERROR",
    "网络超时",
    true,
  )
  .expect("running task should fail");
  let failed_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(failed.status, "failed");
  assert!(failed.retryable);
  assert_eq!(failed_task.status, "failed");

  let retry = retry_task(&root_path, &task.id, None).expect("retry should enqueue");
  assert_ne!(retry.id, running.id);
  assert_eq!(retry.status, "queued");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn interrupted_running_task_is_requeued_and_claimable_again() {
  let root_path = prepared_task_workspace("execution-recovery");
  let task = list_tasks(&root_path, None)
    .expect("tasks should list")
    .remove(0);
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let first_claim = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");

  let recovered = recover_interrupted_runs(&root_path).expect("recovery should succeed");
  let recovered_run = get_task_run(
    &open_workspace_connection(&root_path).expect("database should open"),
    &first_claim.id,
  )
  .expect("run should reload");
  let recovered_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(recovered, 1);
  assert_eq!(recovered_run.status, "queued");
  assert_eq!(recovered_task.status, "queued");

  let second_claim = claim_next_task(&root_path)
    .expect("second claim should succeed")
    .expect("recovered run should be claimable");
  assert_eq!(second_claim.id, first_claim.id);

  std::fs::remove_dir_all(root_path).ok();
}

fn prepared_task_workspace(label: &str) -> std::path::PathBuf {
  let root_path = unique_temp_workspace(label);
  create_workspace("执行器测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "执行任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should create");
  let plan = save_collection_plan(
    &root_path,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: "form_generated".to_string(),
      plan_json: serde_json::json!({
        "platforms": ["tiktok"],
        "data_types": ["comments"],
        "region": "US",
        "time_range": "2026-07-01/2026-07-07",
        "steps": [{
          "endpoint_key": "tiktok.comments",
          "platform": "tiktok",
          "data_type": "comments",
          "params": {
            "item_id": "video-1",
            "region": "US",
            "time_range": "2026-07-01/2026-07-07"
          }
        }],
        "request_limit": 1,
        "missing_fields": [],
        "requires_user_confirmation": true
      }),
      validation_status: "valid".to_string(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )
  .expect("plan should save");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan should confirm");
  root_path
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
