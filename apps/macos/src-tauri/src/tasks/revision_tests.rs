use rusqlite::params;
use uuid::Uuid;

use super::*;
use crate::collection::{generate_account_collection_plan, AccountFormCollectionPlanRequest};
use crate::workspace::create_workspace;

#[test]
fn failed_and_cancelled_tasks_create_a_new_plan_version_in_place() {
  for status in ["failed", "cancelled"] {
    let root_path = workspace(&format!("revise-{status}"));
    let task = create_collection_task(&root_path, task_input()).expect("task created");
    let old_plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
    set_task_status(&root_path, &task.id, status);

    let revised = revise_collection_task(
      &root_path,
      revise_input(&task.id, "修正后的任务", 12, 999_999),
    )
    .expect("terminal editable task should be revised");

    assert_eq!(revised.task.id, task.id);
    assert_eq!(revised.task.status, "waiting_confirmation");
    assert_eq!(revised.task.name, "修正后的任务");
    assert_ne!(revised.collection_plan.id, old_plan.id);
    assert_eq!(revised.collection_plan.source, "user_edited");
    assert_eq!(revised.collection_plan.validation_status, "valid");
    assert_eq!(
      revised.collection_plan.cost_estimate_json["request_count_estimate"], 1,
      "client supplied cost must be ignored"
    );
    assert_eq!(plan_count(&root_path, &task.id), 2);
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn queued_and_running_tasks_must_be_cancelled_before_revision() {
  for status in ["queued", "running"] {
    let root_path = workspace(&format!("reject-revise-{status}"));
    let task = create_collection_task(&root_path, task_input()).expect("task created");
    save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
    set_task_status(&root_path, &task.id, status);

    let error = revise_collection_task(&root_path, revise_input(&task.id, "不应保存", 10, 1))
      .expect_err("active tasks must reject revision");

    assert_eq!(error.code, AppErrorCode::ValidationError);
    assert!(error.message.contains("先取消"));
    assert_eq!(plan_count(&root_path, &task.id), 1);
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn successful_tasks_are_copied_before_editing() {
  for status in ["success", "partial_success"] {
    let root_path = workspace(&format!("copy-revise-{status}"));
    let task = create_collection_task(&root_path, task_input()).expect("task created");
    let old_plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
    set_task_status(&root_path, &task.id, status);

    let revised = revise_collection_task(
      &root_path,
      revise_input(&task.id, "成功任务的新版本", 10, 1),
    )
    .expect("successful task should copy and revise");

    assert_ne!(revised.task.id, task.id);
    assert_eq!(
      revised.copied_from_task_id.as_deref(),
      Some(task.id.as_str())
    );
    assert_eq!(revised.task.status, "waiting_confirmation");
    assert_eq!(revised.collection_plan.task_id, revised.task.id);
    assert_eq!(get_task(&root_path, &task.id).unwrap().status, status);
    assert_eq!(
      get_latest_collection_plan(&root_path, &task.id).unwrap().id,
      old_plan.id
    );
    assert!(
      crate::ai::list_task_intents(&root_path, &revised.task.id)
        .unwrap()
        .is_empty(),
      "form tasks must not gain a natural-language intent when copied"
    );
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn copied_natural_language_tasks_keep_the_latest_original_request() {
  let root_path = workspace("copy-natural-language-intent");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "英国宠物用品账号".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("natural-language task created");
  save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  crate::prompts::seed_builtin_prompts(&root_path).expect("prompts seeded");
  let connection = open_workspace_connection(&root_path).expect("database open");
  let prompt_id: String = connection
    .query_row(
      "SELECT id FROM prompt_version ORDER BY created_at LIMIT 1",
      [],
      |row| row.get(0),
    )
    .expect("prompt exists");
  for (id, intent_text, ai_run_id, created_at) in [
    (
      "intent-old",
      "查找宠物用品账号",
      "ai-run-old",
      "2026-07-20T00:00:00Z",
    ),
    (
      "intent-latest",
      "用中文查找英国 TikTok 宠物用品账号",
      "ai-run-latest",
      "2026-07-20T00:01:00Z",
    ),
  ] {
    let snapshot_id = format!("snapshot-{ai_run_id}");
    connection
      .execute(
        "INSERT INTO runtime_snapshot (
           id, task_id, provider_id, model_id, api_format, base_url_type,
           prompt_version_id, output_schema_id, capabilities_json, config_source, created_at
         ) VALUES (?1, ?2, 'provider', 'model', 'openai_compatible', 'custom',
                   ?3, 'collection_intent_v1', '{}', 'test', ?4)",
        params![snapshot_id, task.id, prompt_id, created_at],
      )
      .expect("snapshot inserted");
    connection
      .execute(
        "INSERT INTO ai_run (
           id, task_id, runtime_snapshot_id, run_type, schema_valid,
           validation_status, created_at
         ) VALUES (?1, ?2, ?3, 'collection_intent_generation', 1, 'valid', ?4)",
        params![ai_run_id, task.id, snapshot_id, created_at],
      )
      .expect("valid AI run inserted");
    connection
      .execute(
        "INSERT INTO task_intent (
          id, task_id, intent_text, language, parse_status, parse_phase,
          ai_run_id, error_safe_details_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, 'zh-CN', 'valid', 'success', ?4, '{}', ?5, ?5)",
        params![id, task.id, intent_text, ai_run_id, created_at],
      )
      .expect("intent inserted");
  }
  connection
    .execute(
      "INSERT INTO task_intent (
         id, task_id, intent_text, language, parse_status, parse_phase,
         error_code, error_message, retryable, error_safe_details_json, created_at, updated_at
       ) VALUES ('intent-failed-later', ?1, '不应复制的失败输入', 'zh-CN', 'failed',
                 'requesting_ai', 'MODEL_REQUEST_ERROR', 'AI 服务请求超时', 1, '{}', ?2, ?2)",
      params![task.id, "2026-07-20T00:02:00Z"],
    )
    .expect("newer failed intent inserted");
  drop(connection);
  set_task_status(&root_path, &task.id, "success");

  let revised = revise_collection_task(
    &root_path,
    revise_input(&task.id, "成功任务的新版本", 10, 1),
  )
  .expect("successful natural-language task should copy and revise");

  let copied_attempts =
    crate::ai::list_task_intents(&root_path, &revised.task.id).expect("copied intent should load");
  assert_eq!(copied_attempts.len(), 1);
  let copied = &copied_attempts[0];
  assert_ne!(copied.id, "intent-latest");
  assert_eq!(copied.task_id, revised.task.id);
  assert_eq!(copied.intent_text, "用中文查找英国 TikTok 宠物用品账号");
  assert_eq!(copied.language.as_deref(), Some("zh-CN"));
  assert_eq!(copied.parse_status, "needs_review");
  assert_eq!(copied.parse_phase.as_deref(), Some("needs_review"));
  assert_eq!(copied.ai_run_id, None, "old AI runs must not be reused");
  assert_eq!(
    copied.error_safe_details_json,
    serde_json::json!({
      "source": "user_edited_copy",
      "copied_from_task_id": task.id,
      "origin_attempt_id": "intent-latest",
    })
  );

  let original_attempts = crate::ai::list_task_intents(&root_path, &task.id)
    .expect("original intents should remain available");
  assert_eq!(original_attempts.len(), 3);
  assert_eq!(original_attempts[0].id, "intent-failed-later");
  assert_eq!(original_attempts[1].id, "intent-latest");
  assert_eq!(
    original_attempts[1].ai_run_id.as_deref(),
    Some("ai-run-latest")
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn natural_language_revision_persists_the_edited_original_request() {
  let root_path = workspace("revise-natural-language-intent");
  let original = "用中文查找英国 TikTok 宠物用品账号";
  let edited = "用中文查找英国 TikTok 宠物食品账号，最多 20 个";
  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "英国宠物用品账号".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
    Some(original),
  )
  .expect("natural-language task should be created");
  let mut serialized =
    serde_json::to_value(revise_input(&task.id, "英国宠物食品账号", 20, 1)).unwrap();
  serialized["original_intent"] = serde_json::json!(edited);
  let input: ReviseCollectionTaskInput = serde_json::from_value(serialized).unwrap();

  revise_collection_task(&root_path, input).expect("natural-language revision should save");

  let attempts = crate::ai::list_task_intents(&root_path, &task.id).unwrap();
  assert_eq!(attempts.len(), 2);
  assert_eq!(attempts[0].intent_text, edited);
  assert_eq!(attempts[0].parse_status, "needs_review");
  assert_eq!(attempts[0].ai_run_id, None);
  assert_eq!(attempts[0].error_safe_details_json["source"], "user_edited");
  assert_eq!(attempts[1].intent_text, original);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn old_runs_remain_bound_to_the_plan_that_actually_ran() {
  let root_path = workspace("revise-run-binding");
  let task = create_collection_task(&root_path, task_input()).expect("task created");
  let old_plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &old_plan.id).expect("plan confirmed");
  let run = enqueue_task(&root_path, &task.id).expect("run queued");
  set_task_status(&root_path, &task.id, "failed");

  let revised = revise_collection_task(&root_path, revise_input(&task.id, "失败后修订", 10, 1))
    .expect("failed task should revise");
  let connection = open_workspace_connection(&root_path).expect("database open");
  let persisted_plan_id = connection
    .query_row(
      "SELECT plan_id FROM task_run WHERE id = ?1",
      params![run.id],
      |row| row.get::<_, String>(0),
    )
    .expect("run plan should load");

  assert_eq!(persisted_plan_id, old_plan.id);
  assert_ne!(persisted_plan_id, revised.collection_plan.id);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn endpoint_whitelist_errors_keep_the_revision_editable() {
  let root_path = workspace("revise-invalid-whitelist");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "宠物园区".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["xiaohongshu".to_string()],
      data_types: vec!["account".to_string()],
    },
  )
  .expect("task created");
  let mut plan = generate_account_collection_plan(AccountFormCollectionPlanRequest {
    platform: "xiaohongshu".to_string(),
    account_source: "user_search".to_string(),
    selected_fields: Vec::new(),
    enrichment_policy: "auto_costed".to_string(),
    params: serde_json::json!({ "keyword": "宠物园区" }),
    age_range: None,
    gender_filter: None,
    request_limit: Some(1),
    record_limit: Some(1),
    budget_limit_micros: Some(100_000),
  })
  .expect("valid base plan")
  .plan_json;
  plan["region"] = serde_json::json!("CN");
  plan["time_range"] = serde_json::json!("7");
  plan["steps"][0]["params"]["region"] = serde_json::json!("CN");
  plan["steps"][0]["params"]["time_range"] = serde_json::json!("7");

  let revised = revise_collection_task(
    &root_path,
    ReviseCollectionTaskInput::user_edited_for_test(
      task.id,
      "宠物园区",
      vec!["xiaohongshu".to_string()],
      vec!["account".to_string()],
      plan,
    ),
  )
  .expect("invalid revision should remain editable");

  assert_eq!(revised.task.status, "draft");
  assert_eq!(revised.collection_plan.validation_status, "needs_review");
  assert!(revised
    .collection_plan
    .validation_errors_json
    .as_array()
    .unwrap()
    .iter()
    .any(|error| error.as_str().is_some_and(|value| value.contains("白名单"))));
  std::fs::remove_dir_all(root_path).ok();
}

fn task_input() -> CreateCollectionTaskInput {
  CreateCollectionTaskInput {
    name: "待修订任务".to_string(),
    source_type: "form".to_string(),
    platforms: vec!["tiktok".to_string()],
    data_types: vec!["keyword_search".to_string()],
  }
}

fn plan_input(task_id: &str) -> SaveCollectionPlanInput {
  let input = revise_input(task_id, "待修订任务", 10, 1);
  SaveCollectionPlanInput {
    task_id: input.task_id,
    source: "form_generated".to_string(),
    plan_json: input.plan_json,
    validation_status: "valid".to_string(),
    validation_errors_json: None,
    cost_estimate_json: None,
  }
}

fn revise_input(
  task_id: &str,
  name: &str,
  record_limit: i64,
  client_request_estimate: i64,
) -> ReviseCollectionTaskInput {
  ReviseCollectionTaskInput::user_edited_for_test(
    task_id,
    name,
    vec!["tiktok".to_string()],
    vec!["keyword_search".to_string()],
    serde_json::json!({
      "schema_version": 2,
      "platforms": ["tiktok"],
      "data_types": ["keyword_search"],
      "region": "GB",
      "time_range": "近 30 天",
      "steps": [{
        "endpoint_key": "tiktok.keyword_search",
        "platform": "tiktok",
        "data_type": "keyword_search",
        "params": { "keyword": "pet supplies", "region": "GB", "time_range": "近 30 天" }
      }],
      "record_limit": record_limit,
      "request_limit": 1,
      "budget_limit": { "currency": "USD", "amount_micros": 100_000 },
      "cost_estimate": { "request_count_estimate": client_request_estimate },
      "missing_fields": [],
      "requires_user_confirmation": true
    }),
  )
}

fn workspace(label: &str) -> std::path::PathBuf {
  let root_path = std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()));
  create_workspace("任务修订测试", &root_path).expect("workspace created");
  root_path
}

fn set_task_status(root_path: &std::path::Path, task_id: &str, status: &str) {
  let connection = open_workspace_connection(root_path).expect("database open");
  connection
    .execute(
      "UPDATE collection_task SET status = ?1 WHERE id = ?2",
      params![status, task_id],
    )
    .expect("test status updated");
}

fn plan_count(root_path: &std::path::Path, task_id: &str) -> i64 {
  let connection = open_workspace_connection(root_path).expect("database open");
  connection
    .query_row(
      "SELECT COUNT(*) FROM collection_plan WHERE task_id = ?1",
      params![task_id],
      |row| row.get(0),
    )
    .expect("plan count")
}
