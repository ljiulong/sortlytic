use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use chrono::Utc;
use serde_json::Value;
use uuid::Uuid;

use super::*;
use crate::api_profiles::{
  save_api_profile_registry, ActiveApiProfileIds, AiApiProfile, ApiCredential, ApiProfileRegistry,
  CredentialProviderType,
};
use crate::tasks::{create_collection_task, get_task, CreateCollectionTaskInput};
use crate::workspace::create_workspace;

#[test]
fn natural_language_generation_requires_an_active_ai_profile() {
  let root_path = unique_temp_workspace("ai-profile-required");
  create_workspace("AI 配置前置测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "不能静默回退本地规则".to_string(),
      source_type: "natural_language".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("task should be created");

  let error = generate_collection_plan_from_text(
    &root_path,
    GenerateCollectionPlanFromTextInput {
      task_id: task.id,
      intent_text: "采集最近 7 天美国 TikTok 汽车内容".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect_err("missing active AI profile must fail closed");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("AI 配置"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn text_generation_uses_active_prompt_and_real_provider_response() {
  let root_path = unique_temp_workspace("ai-plan");
  create_workspace("AI 测试", &root_path).expect("workspace should be created");
  let plan = valid_keyword_plan();
  let response = serde_json::json!({
    "choices": [{ "message": { "content": plan.to_string() } }],
    "usage": { "prompt_tokens": 120, "completion_tokens": 80 }
  })
  .to_string();
  let (base_url, server) = serve_ai_once(200, response, |request| {
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
    assert!(request.contains("input_json.text"));
    assert!(request.contains("collection_plan_v3"));
    assert!(request.contains("最近 7 天美国 TikTok 汽车内容"));
  });
  let profile_id = configure_active_ai(&root_path, base_url);
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "自然语言任务".to_string(),
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
      intent_text: "采集最近 7 天美国 TikTok 汽车内容，预算 2 美元".to_string(),
      provider_id: None,
      model_id: None,
    },
  )
  .expect("plan should generate");
  server.join().expect("test server should finish");
  let runs = list_ai_runs(
    &root_path,
    result.ai_run.task_id.clone(),
    Some("collection_plan_generation".to_string()),
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
    "collection_plan_v3"
  );
  assert_eq!(result.runtime_snapshot.config_source, "active_api_profile");
  assert_eq!(result.collection_plan.validation_status, "valid");
  assert_eq!(
    result.collection_plan.plan_json["budget_limit"]["amount_micros"],
    2_000_000
  );
  assert_eq!(updated_task.platforms_json, serde_json::json!(["tiktok"]));
  assert_eq!(
    updated_task.data_types_json,
    serde_json::json!(["keyword_search"])
  );
  assert_eq!(runs.len(), 1);

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
  let runs = list_ai_runs(&root_path, task.id, None).expect("failed run should list");

  assert_eq!(error.code, AppErrorCode::ModelAuthError);
  assert_eq!(runs.len(), 1);
  assert_eq!(runs[0].validation_status, "failed");
  assert_eq!(runs[0].error_code.as_deref(), Some("MODEL_AUTH_ERROR"));
  let serialized = serde_json::to_string(&runs[0]).expect("run should serialize");
  assert!(!serialized.contains("sk-ai-secret"));
  assert!(!serialized.contains("provider-private-body"));

  std::fs::remove_dir_all(root_path).ok();
}

fn valid_keyword_plan() -> Value {
  serde_json::json!({
    "schema_version": 3,
    "platforms": ["tiktok"],
    "data_types": ["keyword_search"],
    "internal_data_types": [],
    "region": "US",
    "keywords": ["car"],
    "accounts": [],
    "time_range": "7",
    "age_range": null,
    "gender_filter": null,
    "steps": [{
      "step_key": "keyword_search",
      "role": "entry",
      "depends_on_step_key": null,
      "input_binding": null,
      "endpoint_key": "tiktok.keyword_search",
      "platform": "tiktok",
      "data_type": "keyword_search",
      "params": { "keyword": "car", "region": "US", "time_range": "7", "page_size": 50 },
      "request_limit": 1,
      "output_selected": true
    }],
    "record_limit": 50,
    "request_limit": 1,
    "budget_limit": { "currency": "USD", "amount_micros": 2_000_000 },
    "output_rules": {
      "entity": "account",
      "dedupe_key": ["platform", "platform_user_id"],
      "fallback_dedupe_key": ["platform", "normalized_account"],
      "selected_data_types": ["keyword_search"]
    },
    "missing_fields": [],
    "confidence": 0.96,
    "requires_user_confirmation": true
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

fn serve_ai_once(
  status: u16,
  body: String,
  inspect: impl FnOnce(&str) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
  let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
  let address = listener.local_addr().expect("test address should resolve");
  let server = thread::spawn(move || {
    let (mut stream, _) = listener.accept().expect("test server should accept");
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
    let request = String::from_utf8_lossy(&request).into_owned();
    inspect(&request);
    let reason = if status == 200 { "OK" } else { "Unauthorized" };
    write!(
      stream,
      "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
      body.len()
    )
    .expect("response should be writable");
  });
  (format!("http://{address}"), server)
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
