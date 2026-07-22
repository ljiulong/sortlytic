use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::*;
use crate::api_profiles::{
  save_api_profile_registry, sync_api_profile_mirror, update_api_profile_registry,
  ActiveApiProfileIds, AiApiProfile, ApiCredential, ApiProfileRegistry, CredentialProviderType,
  TikhubApiProfile,
};
use crate::records::list_task_record_counts;
use crate::tasks::{
  confirm_collection_plan, create_collection_task, create_collection_task_with_initial_intent,
  enqueue_task, execute_next_task, get_task, list_task_logs, CreateCollectionTaskInput,
};
use crate::tikhub::test_support::override_tikhub_base_url_for_current_test;
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

const TIKHUB_E2E_TOKEN: &str = "sortlytic-e2e-tikhub-token";

#[test]
fn expired_natural_parse_lease_is_interrupted_before_a_new_attempt() {
  let root_path = unique_temp_workspace("ai-expired-lease");
  create_workspace("AI 过期租约测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "恢复崩溃解析".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
  )
  .expect("task should be created");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  connection
    .execute(
      "INSERT INTO task_intent (
        id, task_id, intent_text, language, parse_status, parse_phase,
        error_safe_details_json, created_at, updated_at
      ) VALUES ('attempt-expired', ?1, '旧解析', 'zh-CN', 'running',
                'requesting_ai', '{}', '2000-01-01T00:00:00Z', '2000-01-01T00:00:00Z')",
      [&task.id],
    )
    .unwrap();
  drop(connection);

  let error = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: "重新解析英国 TikTok 宠物用品账号".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect_err("expired lease should be reclaimed before normal profile validation");

  assert_eq!(error.code, AppErrorCode::ModelConfigError);
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let states = connection
    .query_row(
      "SELECT
         SUM(CASE WHEN parse_status = 'interrupted' THEN 1 ELSE 0 END),
         SUM(CASE WHEN parse_status = 'failed' THEN 1 ELSE 0 END)
       FROM task_intent WHERE task_id = ?1",
      [&task.id],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )
    .unwrap();
  assert_eq!(states, (1, 1));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn direct_generation_rejects_oversized_intent_before_attempt_or_provider_request() {
  let root_path = unique_temp_workspace("ai-intent-size-limit");
  create_workspace("AI 输入上限测试", &root_path).expect("workspace should be created");
  configure_active_ai(&root_path, "http://127.0.0.1:1".to_string());
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "直接命令输入上限".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
  )
  .expect("natural-language task should be created");

  let error = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: "a".repeat(10_001),
      provider_id: None,
      model_id: None,
    },
  )
  .expect_err("oversized direct generation must fail before provider access");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("10000"));
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempts = connection
    .query_row(
      "SELECT COUNT(*) FROM task_intent WHERE task_id = ?1",
      [&task.id],
      |row| row.get::<_, i64>(0),
    )
    .unwrap();
  assert_eq!(attempts, 0);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn natural_generation_rejects_ineligible_tasks_before_the_provider_request() {
  let root_path = unique_temp_workspace("ai-task-eligibility");
  create_workspace("AI 任务资格测试", &root_path).expect("workspace should be created");
  configure_active_ai(&root_path, "http://127.0.0.1:1".to_string());

  let form_task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "表单任务不能调用 AI".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["account".to_string()],
    },
  )
  .expect("form task should be created");
  let form_error = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: form_task.id.clone(),
      intent_text: "尝试改写表单任务".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect_err("form tasks must be rejected before any provider request");
  assert_eq!(form_error.code, AppErrorCode::ValidationError);
  assert!(form_error.message.contains("自然语言"));

  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let form_attempts = connection
    .query_row(
      "SELECT COUNT(*) FROM task_intent WHERE task_id = ?1",
      [&form_task.id],
      |row| row.get::<_, i64>(0),
    )
    .unwrap();
  assert_eq!(form_attempts, 0, "资格拒绝不能创建解析记录");

  for status in ["queued", "running", "success", "partial_success"] {
    let task = create_collection_task(
      &root_path,
      CreateCollectionTaskInput {
        name: format!("拒绝 {status} 任务"),
        source_type: "natural_language".to_string(),
        platforms: vec![],
        data_types: vec![],
      },
    )
    .expect("natural-language task should be created");
    connection
      .execute(
        "UPDATE collection_task SET status = ?1 WHERE id = ?2",
        rusqlite::params![status, task.id],
      )
      .unwrap();

    let error = generate_collection_plan_from_text(
      &root_path,
      GenerateCollectionPlanFromTextInput {
        task_id: task.id.clone(),
        intent_text: "不应产生模型费用".to_string(),
        provider_id: None,
        model_id: None,
      },
    )
    .expect_err("ineligible task status must fail before any provider request");

    assert_eq!(error.code, AppErrorCode::ValidationError, "status={status}");
    let attempts = connection
      .query_row(
        "SELECT COUNT(*) FROM task_intent WHERE task_id = ?1",
        [&task.id],
        |row| row.get::<_, i64>(0),
      )
      .unwrap();
    assert_eq!(attempts, 0, "status={status} 不能创建解析记录");
  }

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn failed_and_cancelled_natural_tasks_can_reparse_in_place() {
  for status in ["failed", "cancelled"] {
    let root_path = unique_temp_workspace(&format!("ai-reparse-{status}"));
    create_workspace("AI 失败任务重新解析", &root_path).expect("workspace should be created");
    let intent = valid_collection_intent();
    let response = serde_json::json!({
      "choices": [{ "message": { "content": intent.to_string() } }]
    })
    .to_string();
    let (base_url, server) = serve_ai_once(200, response, |_| {});
    configure_active_ai(&root_path, base_url);
    let task = create_collection_task(
      &root_path,
      CreateCollectionTaskInput {
        name: format!("重新解析 {status} 任务"),
        source_type: "natural_language".to_string(),
        platforms: vec![],
        data_types: vec![],
      },
    )
    .expect("natural-language task should be created");
    let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute(
        "UPDATE collection_task SET status = ?1 WHERE id = ?2",
        rusqlite::params![status, task.id],
      )
      .unwrap();

    let result = generate_collection_plan_from_text(
      &root_path,
      GenerateCollectionPlanFromTextInput {
        task_id: task.id.clone(),
        intent_text: "采集最近 7 天美国 TikTok 汽车内容，预算 2 美元".to_string(),
        provider_id: None,
        model_id: None,
      },
    )
    .expect("failed and cancelled natural tasks should create a new plan version");
    server.join().expect("test server should finish");

    assert!(result.collection_plan.is_some(), "status={status}");
    assert_eq!(
      get_task(&root_path, &task.id).unwrap().status,
      "waiting_confirmation",
      "status={status}",
    );
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn concurrent_natural_generation_allows_only_one_provider_request() {
  let root_path = unique_temp_workspace("ai-single-flight");
  create_workspace("AI 单次在途测试", &root_path).expect("workspace should be created");
  let intent = valid_collection_intent();
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }]
  })
  .to_string();
  let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
  let address = listener.local_addr().expect("test address should resolve");
  let (request_started_tx, request_started_rx) = mpsc::channel();
  let (release_response_tx, release_response_rx) = mpsc::channel();
  let server = thread::spawn(move || {
    let (mut first_stream, _) = listener.accept().expect("first request should arrive");
    let first_request = read_http_request(&mut first_stream);
    assert!(first_request.starts_with("POST /v1/chat/completions HTTP/1.1"));
    request_started_tx.send(()).unwrap();
    let rescue_second_request = release_response_rx.recv().unwrap();
    write_json_response(&mut first_stream, 200, "OK", &response);
    if rescue_second_request {
      let (mut second_stream, _) = listener.accept().expect("second request should be rescued");
      let _ = read_http_request(&mut second_stream);
      write_json_response(&mut second_stream, 200, "OK", &response);
    }
  });
  configure_active_ai(&root_path, format!("http://{address}"));
  let intent_text = "采集最近 7 天美国 TikTok 汽车内容，预算 2 美元";
  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "同一任务只允许一次解析".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
    Some(intent_text),
  )
  .expect("task and initial attempt should be created");
  let input = GenerateCollectionPlanFromTextInput {
    task_id: task.id.clone(),
    intent_text: intent_text.to_string(),
    provider_id: None,
    model_id: None,
  };
  let first_root = root_path.clone();
  let first_input = input.clone();
  let first = thread::spawn(move || generate_collection_plan_from_text(first_root, first_input));
  request_started_rx
    .recv_timeout(Duration::from_secs(2))
    .expect("first provider request should start");

  let running_attempts = list_latest_task_intents(&root_path)
    .expect("running attempt should be visible while the provider response is blocked");
  assert_eq!(running_attempts.len(), 1);
  assert_eq!(running_attempts[0].parse_status, "running");
  assert_eq!(
    running_attempts[0].parse_phase.as_deref(),
    Some("requesting_ai")
  );
  assert!(running_attempts[0].ai_run_id.is_some());
  assert!(running_attempts[0].provider_id.is_some());
  assert_eq!(
    running_attempts[0].provider_name.as_deref(),
    Some("测试 AI")
  );
  assert_eq!(
    running_attempts[0].model_id.as_deref(),
    Some("deepseek-test")
  );
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let running_ai_runs = connection
    .query_row(
      "SELECT COUNT(*), MAX(validation_status) FROM ai_run WHERE task_id = ?1",
      [&task.id],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )
    .unwrap();
  assert_eq!(running_ai_runs, (1, "running".to_string()));
  connection
    .execute(
      "UPDATE task_intent SET updated_at = '2000-01-01T00:00:00Z' WHERE task_id = ?1",
      [&task.id],
    )
    .unwrap();
  drop(connection);
  assert_eq!(
    mark_interrupted_task_intents(&root_path).unwrap(),
    0,
    "an old timestamp must not interrupt a provider request whose process lock is held"
  );
  assert_eq!(
    list_latest_task_intents(&root_path).unwrap()[0].parse_status,
    "running"
  );

  let second_root = root_path.clone();
  let (second_result_tx, second_result_rx) = mpsc::channel();
  let second = thread::spawn(move || {
    second_result_tx
      .send(generate_collection_plan_from_text(second_root, input))
      .unwrap();
  });
  let second_before_release = second_result_rx.recv_timeout(Duration::from_secs(2));
  release_response_tx
    .send(second_before_release.is_err())
    .expect("first response should be released");

  let first_result = first.join().expect("first generation thread should finish");
  let second_result = match second_before_release {
    Ok(result) => result,
    Err(_) => second_result_rx
      .recv_timeout(Duration::from_secs(2))
      .expect("buggy second request should finish after rescue"),
  };
  second
    .join()
    .expect("second generation thread should finish");
  server.join().expect("test server should finish");

  assert!(first_result.is_ok());
  let second_error = second_result.expect_err(
    "the concurrent call must be rejected while the first provider request is in flight",
  );
  assert_eq!(second_error.code, AppErrorCode::ModelRequestError);
  assert!(second_error.retryable);
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempts = connection
    .query_row(
      "SELECT COUNT(*) FROM task_intent WHERE task_id = ?1",
      [&task.id],
      |row| row.get::<_, i64>(0),
    )
    .unwrap();
  assert_eq!(attempts, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn natural_language_generation_requires_an_active_ai_profile() {
  let root_path = unique_temp_workspace("ai-profile-required");
  create_workspace("AI 配置前置测试", &root_path).expect("workspace should be created");
  let intent_text = "采集最近 7 天美国 TikTok 汽车内容";
  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "不能静默回退本地规则".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
    Some(intent_text),
  )
  .expect("task should be created");

  let error = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: intent_text.to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect_err("missing active AI profile must fail closed");

  assert_eq!(error.code, AppErrorCode::ModelConfigError);
  assert!(error.message.contains("AI 配置"));
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempt = connection
    .query_row(
      "SELECT intent_text, parse_status, parse_phase, error_code, error_message, retryable
       FROM task_intent WHERE task_id = ?1 ORDER BY created_at DESC LIMIT 1",
      [&task.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, Option<String>>(2)?,
          row.get::<_, Option<String>>(3)?,
          row.get::<_, Option<String>>(4)?,
          row.get::<_, Option<i64>>(5)?,
        ))
      },
    )
    .expect("前置配置失败也必须保留解析尝试");
  assert_eq!(attempt.0, "采集最近 7 天美国 TikTok 汽车内容");
  assert_eq!(attempt.1, "failed");
  assert_eq!(attempt.2.as_deref(), Some("preparing"));
  assert_eq!(attempt.3.as_deref(), Some("MODEL_CONFIG_ERROR"));
  assert!(attempt.4.is_some_and(|message| message.contains("AI 配置")));
  assert_eq!(attempt.5, Some(0));
  let attempt_count = connection
    .query_row(
      "SELECT COUNT(*) FROM task_intent WHERE task_id = ?1",
      [&task.id],
      |row| row.get::<_, i64>(0),
    )
    .unwrap();
  assert_eq!(attempt_count, 1, "首次失败必须复用原子创建的解析记录");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn retry_creates_a_new_attempt_after_the_atomic_initial_attempt_fails() {
  let root_path = unique_temp_workspace("ai-initial-attempt-retry");
  create_workspace("AI 初始记录重试", &root_path).expect("workspace should be created");
  let intent_text = "采集英国 TikTok 宠物用品账号";
  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "初始记录重试".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
    Some(intent_text),
  )
  .expect("task and initial attempt should be created");

  for _ in 0..2 {
    generate_collection_plan_from_text(
      &root_path,
      GenerateCollectionPlanFromTextInput {
        task_id: task.id.clone(),
        intent_text: intent_text.to_string(),
        provider_id: None,
        model_id: None,
      },
    )
    .expect_err("missing AI profile should persist each failed attempt");
  }

  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempts = connection
    .query_row(
      "SELECT COUNT(*), SUM(CASE WHEN parse_status = 'failed' THEN 1 ELSE 0 END)
       FROM task_intent WHERE task_id = ?1",
      [task.id],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )
    .unwrap();
  assert_eq!(attempts, (2, 2));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn atomic_initial_attempt_can_only_be_claimed_once() {
  let root_path = unique_temp_workspace("ai-initial-attempt-single-claim");
  create_workspace("AI 初始记录单次认领", &root_path).expect("workspace should be created");
  let intent_text = "采集英国 TikTok 宠物用品账号";
  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "初始记录单次认领".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
    Some(intent_text),
  )
  .expect("task and initial attempt should be created");
  let first_connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let second_connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();

  let first = generation::claim_initial_task_intent_attempt(
    &first_connection,
    &task.id,
    intent_text,
    "2026-07-21T00:00:00Z",
  )
  .unwrap();
  let second = generation::claim_initial_task_intent_attempt(
    &second_connection,
    &task.id,
    intent_text,
    "2026-07-21T00:00:01Z",
  )
  .unwrap();

  assert!(first.is_some());
  assert!(second.is_none());
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn atomic_initial_attempt_is_not_claimed_for_different_input() {
  let root_path = unique_temp_workspace("ai-initial-attempt-input-match");
  create_workspace("AI 初始记录原文匹配", &root_path).expect("workspace should be created");
  let initial_text = "采集英国 TikTok 宠物用品账号";
  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "初始记录原文匹配".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
    Some(initial_text),
  )
  .expect("task and initial attempt should be created");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();

  let claimed = generation::claim_initial_task_intent_attempt(
    &connection,
    &task.id,
    "采集美国 TikTok 汽车账号",
    "2026-07-21T00:00:00Z",
  )
  .unwrap();
  let phase = connection
    .query_row(
      "SELECT parse_phase FROM task_intent WHERE task_id = ?1",
      [&task.id],
      |row| row.get::<_, String>(0),
    )
    .unwrap();

  assert!(claimed.is_none());
  assert_eq!(phase, "preparing");
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn text_generation_uses_active_prompt_and_real_provider_response() {
  let root_path = unique_temp_workspace("ai-plan");
  create_workspace("AI 测试", &root_path).expect("workspace should be created");
  let intent = valid_collection_intent();
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }],
    "usage": { "prompt_tokens": 120, "completion_tokens": 80 }
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |request| {
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(request.contains("input_json.text"));
    assert!(request.contains("collection_intent_v1"));
    assert!(request.contains("最近 7 天美国 TikTok 汽车内容"));
  });
  let profile_id = configure_active_ai(&root_path, base_url);
  let intent_text = "采集最近 7 天美国 TikTok 汽车内容，预算 2 美元";
  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "自然语言任务".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["xiaohongshu".to_string()],
      data_types: vec!["comments".to_string()],
    },
    Some(intent_text),
  )
  .expect("task should be created");

  let result = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: intent_text.to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect("plan should generate");
  server.join().expect("test server should finish");
  let runs = list_ai_runs(
    &root_path,
    result.ai_run.task_id.clone(),
    Some("collection_intent_generation".to_string()),
  )
  .expect("runs should list");
  let updated_task = get_task(&root_path, &task.id).expect("task should reload");

  assert!(result.ai_run.schema_valid);
  assert_eq!(result.ai_run.validation_status, "valid");
  assert_eq!(result.ai_run.input_tokens, Some(120));
  assert_eq!(result.ai_run.output_tokens, Some(80));
  assert_eq!(result.runtime_snapshot.provider_id, profile_id);
  assert_eq!(result.runtime_snapshot.model_id, "deepseek-test");
  assert_eq!(
    result.runtime_snapshot.output_schema_id,
    "collection_intent_v1"
  );
  assert_eq!(result.runtime_snapshot.config_source, "active_api_profile");
  assert_eq!(
    result
      .parsed_intent
      .as_ref()
      .and_then(|intent| intent.region_code.as_deref()),
    Some("US")
  );
  let collection_plan = result
    .collection_plan
    .as_ref()
    .expect("后端必须生成 v4 计划");
  assert_eq!(collection_plan.validation_status, "valid");
  assert_eq!(
    collection_plan.plan_json["budget_limit"]["amount_micros"],
    2_000_000
  );
  assert_eq!(collection_plan.plan_json["schema_version"], 4);
  assert_eq!(result.ai_run.output_json.as_ref(), Some(&intent));
  assert_eq!(updated_task.platforms_json, serde_json::json!(["tiktok"]));
  assert_eq!(updated_task.data_types_json, serde_json::json!(["account"]));
  assert_eq!(runs.len(), 1);
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempt = connection
    .query_row(
      "SELECT COUNT(*), MAX(ai_run_id) FROM task_intent WHERE task_id = ?1",
      [task.id],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .unwrap();
  assert_eq!(attempt.0, 1, "首次成功必须复用原子创建的解析记录");
  assert_eq!(attempt.1.as_deref(), Some(result.ai_run.id.as_str()));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn provider_credential_echo_is_rejected_without_persisting_the_response() {
  let root_path = unique_temp_workspace("ai-credential-echo");
  create_workspace("AI 凭据回显测试", &root_path).expect("workspace should be created");
  let mut intent = valid_collection_intent();
  intent["source_input"] = serde_json::json!("sk-ai-secret");
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }]
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |_| {});
  configure_active_ai(&root_path, base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "拒绝供应商凭据回显".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
  )
  .expect("task should be created");

  let error = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: "采集英国 TikTok 宠物用品账号".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect_err("供应商回显当前 API Key 时必须拒绝整个响应");
  server.join().expect("test server should finish");

  assert_eq!(error.code, AppErrorCode::ModelProtocolError);
  assert!(!error.message.contains("sk-ai-secret"));
  let runs = list_ai_runs(
    &root_path,
    task.id.clone(),
    Some("collection_intent_generation".to_string()),
  )
  .unwrap();
  assert_eq!(runs.len(), 1);
  assert_eq!(runs[0].validation_status, "failed");
  assert!(runs[0].output_json.is_none());
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let persisted: String = connection
    .query_row(
      "SELECT COALESCE(group_concat(value, '\n'), '') FROM (
         SELECT COALESCE(output_json, '') || COALESCE(error_message, '') AS value
         FROM ai_run WHERE task_id = ?1
         UNION ALL
         SELECT COALESCE(error_message, '') || error_safe_details_json
         FROM task_intent WHERE task_id = ?1
       )",
      [&task.id],
      |row| row.get(0),
    )
    .unwrap();
  assert!(!persisted.contains("sk-ai-secret"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn invalid_model_intent_does_not_change_existing_task_scope_or_create_a_plan() {
  let root_path = unique_temp_workspace("ai-invalid-plan-scope");
  create_workspace("AI 无效计划范围测试", &root_path).expect("workspace should be created");
  let mut intent = valid_collection_intent();
  intent["platform"] = serde_json::json!("youtube");
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }]
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |_| {});
  configure_active_ai(&root_path, base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "保留原任务范围".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["xiaohongshu".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should be created");

  let result = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: "生成一个不受支持的平台计划".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect("invalid intent should be retained for review");
  server.join().expect("test server should finish");
  let updated_task = get_task(&root_path, &task.id).expect("task should reload");

  assert!(!result.ai_run.schema_valid);
  assert_eq!(result.ai_run.validation_status, "needs_review");
  assert!(result.parsed_intent.is_none());
  assert!(result.collection_plan.is_none());
  assert!(result.issues.iter().any(|issue| issue.contains("platform")));
  assert_eq!(
    updated_task.platforms_json,
    serde_json::json!(["xiaohongshu"])
  );
  assert_eq!(
    updated_task.data_types_json,
    serde_json::json!(["comments"])
  );
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempt = connection
    .query_row(
      "SELECT parse_status, parse_phase, ai_run_id, error_code, error_message,
              retryable, error_safe_details_json FROM task_intent
       WHERE task_id = ?1 ORDER BY created_at DESC LIMIT 1",
      [task.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, Option<String>>(2)?,
          row.get::<_, Option<String>>(3)?,
          row.get::<_, Option<String>>(4)?,
          row.get::<_, Option<i64>>(5)?,
          row.get::<_, String>(6)?,
        ))
      },
    )
    .unwrap();
  assert_eq!(attempt.0, "needs_review");
  assert_eq!(attempt.1.as_deref(), Some("needs_review"));
  assert_eq!(attempt.2.as_deref(), Some(result.ai_run.id.as_str()));
  assert_eq!(attempt.3.as_deref(), Some("VALIDATION_ERROR"));
  assert!(attempt
    .4
    .is_some_and(|message| message.contains("platform")));
  assert_eq!(attempt.5, Some(0));
  let safe_details: Value = serde_json::from_str(&attempt.6).unwrap();
  assert!(safe_details["issues"]
    .as_array()
    .is_some_and(|issues| issues.iter().any(|issue| issue
      .as_str()
      .is_some_and(|value| value.contains("platform")))));
  assert_eq!(safe_details["missing_fields"], serde_json::json!([]));

  let recovered = list_latest_task_intents(&root_path).unwrap();
  assert_eq!(recovered.len(), 1);
  assert_eq!(recovered[0].error_code.as_deref(), Some("VALIDATION_ERROR"));
  assert!(recovered[0]
    .error_message
    .as_deref()
    .is_some_and(|message| message.contains("platform")));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn model_cannot_rewrite_a_direct_account_identifier_from_the_original_request() {
  let root_path = unique_temp_workspace("ai-direct-identifier-preservation");
  create_workspace("AI 直接标识保留测试", &root_path).expect("workspace should be created");
  let mut intent = valid_collection_intent();
  intent["account_source"] = serde_json::json!("direct_account");
  intent["source_input"] =
    serde_json::json!("https://example.com/@OtherBrandUK?token=provider-secret&keyword=pets");
  intent["query_locale"] = Value::Null;
  intent["time_range_days"] = Value::Null;
  intent["record_limit"] = serde_json::json!(1);
  intent["region_code"] = serde_json::json!("GB");
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }]
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |_| {});
  configure_active_ai(&root_path, base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "保留账号链接".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
  )
  .expect("task should be created");

  let result = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id,
      intent_text:
        "采集英国 TikTok 账号 https://www.tiktok.com/@PetBrandUK，最多 1 个，预算 0.1 美元。"
          .to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect("rewritten identifier should be retained for review");
  server.join().expect("test server should finish");

  assert!(result.ai_run.schema_valid);
  assert_eq!(result.ai_run.validation_status, "needs_review");
  assert!(result.collection_plan.is_none());
  assert!(result.issues.iter().any(|issue| issue.contains("原样保留")));
  assert!(result
    .parsed_intent
    .as_ref()
    .is_some_and(|intent| intent.missing_fields.contains(&"source_input".to_string())));
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let safe_details_json = connection
    .query_row(
      "SELECT error_safe_details_json FROM task_intent
       WHERE task_id = ?1 ORDER BY created_at DESC LIMIT 1",
      [&result.ai_run.task_id],
      |row| row.get::<_, String>(0),
    )
    .unwrap();
  let safe_details: Value = serde_json::from_str(&safe_details_json).unwrap();
  assert_eq!(safe_details["intent"]["platform"], "tiktok");
  assert_eq!(safe_details["intent"]["region_code"], "GB");
  assert!(safe_details["intent"]["source_input"]
    .as_str()
    .is_some_and(|value| value.contains("[REDACTED]") && value.contains("keyword=pets")));
  assert!(!safe_details_json.contains("provider-secret"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn model_cannot_shorten_a_direct_identifier_to_a_matching_substring() {
  let root_path = unique_temp_workspace("ai-direct-identifier-substring");
  create_workspace("AI 直接标识截短测试", &root_path).expect("workspace should be created");
  let mut intent = valid_collection_intent();
  intent["account_source"] = serde_json::json!("direct_account");
  intent["source_input"] = serde_json::json!("Pet");
  intent["query_locale"] = Value::Null;
  intent["time_range_days"] = Value::Null;
  intent["record_limit"] = serde_json::json!(1);
  intent["region_code"] = serde_json::json!("GB");
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }]
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |_| {});
  configure_active_ai(&root_path, base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "拒绝截短账号标识".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec![],
      data_types: vec![],
    },
  )
  .expect("task should be created");

  let result = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id,
      intent_text:
        "采集英国 TikTok 账号 https://www.tiktok.com/@PetBrandUK，最多 1 个，预算 0.1 美元。"
          .to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect("shortened identifier should be retained for review");
  server.join().expect("test server should finish");

  assert_eq!(result.ai_run.validation_status, "needs_review");
  assert!(result.collection_plan.is_none());
  assert!(result.issues.iter().any(|issue| issue.contains("原样保留")));
  assert!(result
    .parsed_intent
    .as_ref()
    .is_some_and(|intent| intent.missing_fields.contains(&"source_input".to_string())));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn model_intent_missing_required_field_needs_review_without_a_plan() {
  let root_path = unique_temp_workspace("ai-incomplete-plan-schema");
  create_workspace("AI 不完整计划测试", &root_path).expect("workspace should be created");
  let mut intent = valid_collection_intent();
  intent
    .as_object_mut()
    .expect("intent should be an object")
    .remove("budget_limit_micros");
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }]
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |_| {});
  configure_active_ai(&root_path, base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "拒绝不完整模型计划".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["xiaohongshu".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should be created");

  let result = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: "生成缺少步骤输出边界的计划".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect("incomplete plan should be saved for review");
  server.join().expect("test server should finish");
  let updated_task = get_task(&root_path, &task.id).expect("task should reload");

  assert!(!result.ai_run.schema_valid);
  assert_eq!(result.ai_run.validation_status, "needs_review");
  assert!(result.parsed_intent.is_none());
  assert!(result.collection_plan.is_none());
  assert!(result
    .issues
    .iter()
    .any(|issue| issue.contains("budget_limit_micros")));
  assert_eq!(
    updated_task.platforms_json,
    serde_json::json!(["xiaohongshu"])
  );
  assert_eq!(
    updated_task.data_types_json,
    serde_json::json!(["comments"])
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn natural_language_plan_runs_through_tikhub_and_persists_records() {
  let root_path = unique_temp_workspace("ai-tikhub-e2e");
  create_workspace("自然语言采集端到端测试", &root_path).expect("workspace should be created");
  let intent = valid_collection_intent();
  let ai_response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }],
    "usage": { "prompt_tokens": 44, "completion_tokens": 66 }
  })
  .to_string();
  let (ai_base_url, ai_server) = serve_ai_once(200, ai_response, |request| {
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(request.contains("input_json.text"));
    assert!(request.contains("collection_intent_v1"));
    assert!(request.contains("最近 7 天美国 TikTok 汽车内容"));
  });
  let ai_profile_id = configure_active_ai(&root_path, ai_base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "AI 到 TikHub 完整链路".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("task should be created");

  let generated = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: "采集最近 7 天美国 TikTok 汽车内容，预算 2 美元".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect("AI plan should generate");
  ai_server.join().expect("AI test server should finish");

  let (tikhub_base_url, tikhub_server) = serve_tikhub_collection_flow();
  configure_active_tikhub(&root_path);
  let generated_plan = generated
    .collection_plan
    .as_ref()
    .expect("后端必须生成 v4 计划");
  confirm_collection_plan(&root_path, &task.id, &generated_plan.id)
    .expect("AI plan should be confirmed");
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let _base_url_override = override_tikhub_base_url_for_current_test(tikhub_base_url);
  let completed = execute_next_task(&root_path)
    .expect("worker should execute")
    .expect("queued task should exist");
  assert_eq!(completed.status, "success", "{completed:?}");
  tikhub_server
    .join()
    .expect("TikHub test server should finish");

  let completed_task = get_task(&root_path, &task.id).expect("task should reload");
  let record_counts = list_task_record_counts(&root_path).expect("record counts should list");
  let logs = list_task_logs(&root_path, &completed.id).expect("task logs should list");
  let safe_runtime_output = serde_json::json!({
    "ai_run": generated.ai_run,
    "task_run": completed,
    "task_logs": logs
  })
  .to_string();

  assert_eq!(queued.status, "queued");
  assert_eq!(completed_task.status, "success");
  assert_eq!(generated.runtime_snapshot.provider_id, ai_profile_id);
  assert_eq!(generated.runtime_snapshot.model_id, "deepseek-test");
  assert_eq!(
    generated.runtime_snapshot.output_schema_id,
    "collection_intent_v1"
  );
  assert_eq!(generated.ai_run.input_tokens, Some(44));
  assert_eq!(generated.ai_run.output_tokens, Some(66));
  assert_eq!(
    record_counts
      .iter()
      .find(|count| count.task_id == task.id)
      .map(|count| count.record_count),
    Some(1)
  );
  assert!(!safe_runtime_output.contains(TIKHUB_E2E_TOKEN));
  assert!(!safe_runtime_output.contains("sk-ai-secret"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn prompt_regression_calls_the_active_model_with_the_candidate_content() {
  let root_path = unique_temp_workspace("ai-prompt-regression");
  create_workspace("提示词真实回归测试", &root_path).expect("workspace should be created");
  let intent = valid_collection_intent();
  let response = serde_json::json!({
    "choices": [{ "message": { "content": intent.to_string() } }],
    "usage": { "prompt_tokens": 36, "completion_tokens": 64 }
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |request| {
    assert!(request.contains("候选提示词正文-必须真实发送"));
    assert!(request.contains("回归样例-最近 7 天美国 TikTok 汽车内容"));
    assert!(request.contains("collection_intent_v1"));
  });
  let profile_id = configure_active_ai(&root_path, base_url);

  let result = run_collection_prompt_regression(
    &root_path,
    "候选提示词正文-必须真实发送",
    "回归样例-最近 7 天美国 TikTok 汽车内容",
  )
  .expect("prompt regression should call the active model");
  server.join().expect("test server should finish");

  assert_eq!(result.provider_id, profile_id);
  assert_eq!(result.model_id, "deepseek-test");
  assert_eq!(result.output_json, intent);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn provider_failure_is_persisted_without_secret_or_body() {
  let root_path = unique_temp_workspace("ai-provider-failure");
  create_workspace("AI 失败记录测试", &root_path).expect("workspace should be created");
  let (base_url, server) = serve_ai_once(
    401,
    r#"{"error":"provider-private-body"}"#.to_string(),
    |_| {},
  );
  configure_active_ai(&root_path, base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "供应商失败必须可见".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("task should be created");

  let error = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id.clone(),
      intent_text: "采集美国 TikTok 汽车内容".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect_err("401 must fail");
  server.join().expect("test server should finish");
  let runs = list_ai_runs(&root_path, task.id.clone(), None).expect("failed run should list");

  assert_eq!(error.code, AppErrorCode::ModelAuthError);
  assert_eq!(runs.len(), 1);
  assert_eq!(runs[0].validation_status, "failed");
  assert_eq!(runs[0].error_code.as_deref(), Some("MODEL_AUTH_ERROR"));
  let serialized = serde_json::to_string(&runs[0]).expect("run should serialize");
  assert!(!serialized.contains("sk-ai-secret"));
  assert!(!serialized.contains("provider-private-body"));
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempt = connection
    .query_row(
      "SELECT parse_status, parse_phase, ai_run_id, error_code, error_message,
              retryable, error_safe_details_json
       FROM task_intent WHERE task_id = ?1 ORDER BY created_at DESC LIMIT 1",
      [task.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, Option<String>>(2)?,
          row.get::<_, Option<String>>(3)?,
          row.get::<_, Option<String>>(4)?,
          row.get::<_, Option<i64>>(5)?,
          row.get::<_, String>(6)?,
        ))
      },
    )
    .unwrap();
  assert_eq!(attempt.0, "failed");
  assert_eq!(attempt.1.as_deref(), Some("requesting_ai"));
  assert_eq!(attempt.2.as_deref(), Some(runs[0].id.as_str()));
  assert_eq!(attempt.3.as_deref(), Some("MODEL_AUTH_ERROR"));
  assert_eq!(attempt.5, Some(0));
  let persisted_attempt = serde_json::to_string(&attempt).unwrap();
  assert!(!persisted_attempt.contains("sk-ai-secret"));
  assert!(!persisted_attempt.contains("provider-private-body"));

  std::fs::remove_dir_all(root_path).ok();
}

fn valid_collection_intent() -> Value {
  serde_json::json!({
    "schema_version": 1,
    "platform": "tiktok",
    "account_source": "content_search_authors",
    "source_input": "car",
    "query_locale": "en-US",
    "region_code": "US",
    "selected_fields": [],
    "time_range_days": 7,
    "age_range": null,
    "gender_filter": null,
    "record_limit": 20,
    "budget_limit_micros": 2_000_000,
    "missing_fields": [],
    "confidence": 0.96
  })
}

fn configure_active_ai(root_path: &Path, base_url: String) -> String {
  let profile_id = Uuid::new_v4().to_string();
  let credential_id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  let mut registry = ApiProfileRegistry {
    active_profile_ids: ActiveApiProfileIds {
      tikhub: None,
      ai: Some(profile_id.clone()),
    },
    ..ApiProfileRegistry::default()
  };
  registry.ai_profiles.insert(
    profile_id.clone(),
    AiApiProfile {
      id: profile_id.clone(),
      name: "测试 AI".to_string(),
      provider_type: AiProviderType::CustomOpenaiCompatible,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url,
      default_model_id: "deepseek-test".to_string(),
      credential_ref_id: Some(credential_id.clone()),
      revision: 1,
      status: ApiProfileStatus::Success,
      last_tested_at: Some(now.clone()),
      created_at: now.clone(),
      updated_at: now,
    },
  );
  registry.credentials.insert(
    credential_id.clone(),
    ApiCredential {
      id: credential_id,
      provider_type: CredentialProviderType::CustomOpenaiCompatible,
      profile_id: profile_id.clone(),
      revision: 1,
      secret: "sk-ai-secret".to_string(),
    },
  );
  save_api_profile_registry(root_path, &registry).expect("AI registry should save");
  profile_id
}

fn configure_active_tikhub(root_path: &Path) {
  let profile_id = Uuid::new_v4().to_string();
  let credential_id = Uuid::new_v4().to_string();
  let now = Utc::now().to_rfc3339();
  update_api_profile_registry(root_path, |registry| {
    registry.tikhub_profiles.insert(
      profile_id.clone(),
      TikhubApiProfile {
        id: profile_id.clone(),
        name: "端到端 TikHub".to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        credential_ref_id: credential_id.clone(),
        revision: 1,
        status: ApiProfileStatus::Success,
        last_tested_at: Some(now.clone()),
        test_summary: None,
        created_at: now.clone(),
        updated_at: now.clone(),
      },
    );
    registry.credentials.insert(
      credential_id.clone(),
      ApiCredential {
        id: credential_id,
        provider_type: CredentialProviderType::Tikhub,
        profile_id: profile_id.clone(),
        revision: 1,
        secret: TIKHUB_E2E_TOKEN.to_string(),
      },
    );
    registry.active_profile_ids.tikhub = Some(profile_id);
    Ok(())
  })
  .expect("TikHub registry should update");
  sync_api_profile_mirror(root_path).expect("TikHub profile should mirror into the workspace");
}

fn serve_ai_once(
  status: u16,
  body: String,
  inspect: impl FnOnce(&str) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
  let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
  let address = listener.local_addr().expect("test address should resolve");
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().expect("test server should accept");
    let request = read_http_request(&mut stream);
    inspect(&request);
    let reason = if status == 200 { "OK" } else { "Unauthorized" };
    write_json_response(&mut stream, status, reason, &body);
  });
  (format!("http://{address}"), server)
}

fn serve_tikhub_collection_flow() -> (String, thread::JoinHandle<()>) {
  let listener = TcpListener::bind("127.0.0.1:0").expect("TikHub test server should bind");
  let address = listener.local_addr().expect("test address should resolve");
  let quota = serde_json::json!({
    "code": 200,
    "user_data": { "balance": 5.0, "free_credit": 1.0 }
  })
  .to_string();
  let quote = serde_json::json!({
    "code": 200,
    "data": { "total_price": 0.01, "base_price": 0.01, "currency": "USD" }
  })
  .to_string();
  let responses = vec![
    ("/api/v1/tikhub/user/get_user_info", quota.clone()),
    ("/api/v1/tikhub/user/calculate_price?", quote.clone()),
    (
      "/api/v1/tiktok/app/v3/fetch_video_search_result?",
      serde_json::json!({
        "code": 200,
        "data": {
          "aweme_list": [{
            "aweme_id": "video-e2e",
            "desc": "端到端测试记录",
            "share_url": "https://www.tiktok.com/@author/video/video-e2e",
            "last_posted_at": "2026-07-19T00:00:00Z",
            "author": {
              "user_id": "author-e2e",
              "sec_uid": "MS4wLjABAAAA-e2e",
              "unique_id": "author",
              "nickname": "测试作者",
              "region": "US"
            }
          }],
          "has_more": false
        }
      })
      .to_string(),
    ),
    ("/api/v1/tikhub/user/get_user_info", quota.clone()),
    ("/api/v1/tikhub/user/calculate_price?", quote.clone()),
    (
      "/api/v1/tiktok/app/v3/fetch_user_country_by_username?",
      serde_json::json!({
        "code": 200,
        "data": {
          "user_id": "author-e2e",
          "sec_uid": "MS4wLjABAAAA-e2e",
          "unique_id": "author",
          "country": "US"
        }
      })
      .to_string(),
    ),
    ("/api/v1/tikhub/user/get_user_info", quota),
    ("/api/v1/tikhub/user/calculate_price?", quote),
    (
      "/api/v1/tiktok/app/v3/fetch_user_post_videos?",
      serde_json::json!({
        "code": 200,
        "data": {
          "aweme_list": [{
            "aweme_id": "latest-e2e",
            "create_time": 1784476800,
            "author": {
              "user_id": "author-e2e",
              "sec_uid": "MS4wLjABAAAA-e2e",
              "unique_id": "author"
            }
          }],
          "has_more": false
        }
      })
      .to_string(),
    ),
  ];
  let server = thread::spawn(move || {
    for (index, (path_prefix, body)) in responses.into_iter().enumerate() {
      let (mut stream, _) = listener.accept().expect("TikHub request should arrive");
      let request = read_http_request(&mut stream);
      assert!(
        request.starts_with(&format!("GET {path_prefix}")),
        "unexpected TikHub request: {request}"
      );
      assert!(
        request
          .to_ascii_lowercase()
          .contains(&format!("authorization: bearer {TIKHUB_E2E_TOKEN}")),
        "TikHub request should use the configured token"
      );
      if [1, 4, 7].contains(&index) {
        assert!(request.contains("request_per_day=1"));
      }
      if index == 1 {
        assert!(
          request.contains("endpoint=%2Fapi%2Fv1%2Ftiktok%2Fapp%2Fv3%2Ffetch_video_search_result")
        );
      }
      if index == 4 {
        assert!(request
          .contains("endpoint=%2Fapi%2Fv1%2Ftiktok%2Fapp%2Fv3%2Ffetch_user_country_by_username"));
      }
      if index == 7 {
        assert!(
          request.contains("endpoint=%2Fapi%2Fv1%2Ftiktok%2Fapp%2Fv3%2Ffetch_user_post_videos")
        );
      }
      if index == 2 {
        assert!(request.contains("keyword=car"));
        assert!(request.contains("region=US"));
        assert!(request.contains("publish_time=7"));
        assert!(request.to_ascii_lowercase().contains("idempotency-key:"));
      }
      if index == 5 {
        assert!(request.contains("username=author"));
      }
      if index == 8 {
        assert!(request.contains("sec_user_id=MS4wLjABAAAA-e2e"));
      }
      write_json_response(&mut stream, 200, "OK", &body);
    }
  });
  (format!("http://{address}"), server)
}

fn read_http_request(stream: &mut TcpStream) -> String {
  let mut request = Vec::new();
  let mut buffer = [0_u8; 16 * 1024];
  loop {
    let bytes_read = stream
      .read(&mut buffer)
      .expect("request should be readable");
    if bytes_read == 0 {
      break;
    }
    request.extend_from_slice(&buffer[..bytes_read]);
    let text = String::from_utf8_lossy(&request);
    if let Some(header_end) = text.find("\r\n\r\n") {
      let content_length = text[..header_end]
        .lines()
        .find_map(|line| {
          line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
      if request.len() >= header_end + 4 + content_length {
        break;
      }
    }
  }
  String::from_utf8_lossy(&request).into_owned()
}

fn write_json_response(stream: &mut TcpStream, status: u16, reason: &str, body: &str) {
  write!(
    stream,
    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
    body.len()
  )
  .expect("response should be writable");
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
