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
fn invalid_plan_cannot_be_confirmed() {
  let root_path = unique_temp_workspace("invalid-plan");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let mut input = plan_input(&task.id);
  input.validation_status = "invalid".to_string();
  let plan = save_collection_plan(&root_path, input).expect("plan saved");
  let error =
    confirm_collection_plan(&root_path, &task.id, &plan.id).expect_err("invalid plan should fail");

  assert_eq!(error.code, AppErrorCode::ValidationError);
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
      "region": { "value": "US", "source": "user", "confidence": 1.0 },
      "steps": [{ "endpoint_key": "tiktok.comments.search" }]
    }),
    validation_status: "valid".to_string(),
    validation_errors_json: Some(serde_json::json!([])),
    cost_estimate_json: None,
  }
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
