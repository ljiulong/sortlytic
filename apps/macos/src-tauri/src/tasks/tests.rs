use super::*;
use crate::workspace::create_workspace;

#[test]
fn task_plan_confirm_enqueue_and_logs_round_trip() {
  let root_path = unique_temp_workspace("tasks");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  let confirmed = confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
  let run = enqueue_task(&root_path, &task.id).expect("task enqueued");
  let logs = list_task_logs(&root_path, &run.id).expect("logs should list");

  assert_eq!(task.status, "draft");
  assert_eq!(confirmed.status, "waiting_confirmation");
  assert!(confirmed.confirmed_at.is_some());
  assert_eq!(run.status, "queued");
  assert_eq!(logs.len(), 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn persisted_cost_estimate_counts_the_confirmed_request_limit() {
  let root_path = unique_temp_workspace("request-limit-cost");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let mut input = plan_input(&task.id);
  input.plan_json["request_limit"] = serde_json::json!(5);

  let plan = save_collection_plan(&root_path, input).expect("plan should save");
  let estimate = estimate_task_cost(&root_path, Some(task.id), None).expect("cost should load");

  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 5);
  assert_eq!(estimate.request_count_estimate, 5);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn backend_validation_overrides_client_supplied_status() {
  let root_path = unique_temp_workspace("authoritative-plan-validation");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let mut valid_input = plan_input(&task.id);
  valid_input.validation_status = "invalid".to_string();
  let valid_plan = save_collection_plan(&root_path, valid_input).expect("valid plan saved");

  let mut invalid_input = plan_input(&task.id);
  invalid_input.plan_json = invalid_plan_json();
  invalid_input.validation_status = "valid".to_string();
  invalid_input.validation_errors_json = Some(serde_json::json!([]));
  let invalid_plan = save_collection_plan(&root_path, invalid_input).expect("invalid plan saved");

  assert_eq!(valid_plan.validation_status, "valid");
  assert_eq!(invalid_plan.validation_status, "needs_review");
  assert!(invalid_plan
    .validation_errors_json
    .as_array()
    .is_some_and(|errors| !errors.is_empty()));

  let error = confirm_collection_plan(&root_path, &task.id, &invalid_plan.id)
    .expect_err("backend-invalid plan should fail");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn confirmation_revalidates_persisted_plan_content() {
  let root_path = unique_temp_workspace("confirm-revalidation");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  let connection = open_workspace_connection(&root_path).expect("database should open");

  connection
    .execute(
      "UPDATE collection_plan SET plan_json = ?1, validation_status = 'valid' WHERE id = ?2",
      rusqlite::params![invalid_plan_json().to_string(), plan.id],
    )
    .expect("test should corrupt persisted plan");

  let error = confirm_collection_plan(&root_path, &task.id, &plan.id)
    .expect_err("confirmation must revalidate persisted content");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn changing_confirmed_scope_revokes_confirmation() {
  let root_path = unique_temp_workspace("confirmation-invalidation");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");

  let updated = update_collection_task(
    &root_path,
    &task.id,
    UpdateCollectionTaskInput {
      platforms: Some(vec!["douyin".to_string()]),
      ..UpdateCollectionTaskInput::default()
    },
  )
  .expect("task scope updated");

  assert!(updated.confirmed_at.is_none());
  let error = enqueue_task(&root_path, &task.id)
    .expect_err("scope changes must require a fresh confirmation");
  assert_eq!(error.code, AppErrorCode::ValidationError);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn saving_a_new_plan_revokes_confirmation_and_rejects_stale_plan() {
  let root_path = unique_temp_workspace("new-plan-invalidation");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let first_plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &first_plan.id).expect("plan confirmed");

  let mut replacement_input = plan_input(&task.id);
  replacement_input.source = "user_edited".to_string();
  let replacement =
    save_collection_plan(&root_path, replacement_input).expect("replacement plan saved");
  let updated_task = get_task(&root_path, &task.id).expect("task should load");

  assert!(updated_task.confirmed_at.is_none());
  let stale_error = confirm_collection_plan(&root_path, &task.id, &first_plan.id)
    .expect_err("only the latest plan can be confirmed");
  assert_eq!(stale_error.code, AppErrorCode::ValidationError);

  confirm_collection_plan(&root_path, &task.id, &replacement.id)
    .expect("latest valid plan should confirm");
  std::fs::remove_dir_all(root_path).ok();
}

fn create_task_input() -> CreateCollectionTaskInput {
  CreateCollectionTaskInput {
    name: "采集 TikTok 评论".to_string(),
    source_type: "form".to_string(),
    platforms: vec!["tiktok".to_string()],
    data_types: vec!["comments".to_string()],
  }
}

fn plan_input(task_id: &str) -> SaveCollectionPlanInput {
  SaveCollectionPlanInput {
    task_id: task_id.to_string(),
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
    validation_errors_json: Some(serde_json::json!([])),
    cost_estimate_json: None,
  }
}

fn invalid_plan_json() -> Value {
  serde_json::json!({
    "platforms": ["tiktok"],
    "data_types": ["comments"],
    "region": "US",
    "time_range": null,
    "steps": [{
      "endpoint_key": "tiktok.comments",
      "platform": "tiktok",
      "data_type": "comments",
      "params": {}
    }],
    "request_limit": 1,
    "missing_fields": [],
    "requires_user_confirmation": true
  })
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
