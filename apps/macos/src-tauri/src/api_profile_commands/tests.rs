use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use rusqlite::params;
use serde_json::json;
use uuid::Uuid;

use super::*;
use crate::domain::AppError;
use crate::tikhub::TikhubConnectionTestResult;
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

const AI_SECRET: &str = "sk-ai-safe-view-sentinel-123456789";
const TIKHUB_SECRET: &str = "tk-safe-view-sentinel-987654321";

pub(super) fn run_after_mutability_check_hook() {
  super::mutation_race_tests::run_after_mutability_check_hook();
}

fn workspace(label: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("api-command-{label}-{}", Uuid::new_v4()));
  create_workspace("API 命令测试", &root).unwrap();
  root
}

fn ai_input(id: Option<String>, name: &str, key: Option<&str>) -> SaveApiProfileInput {
  SaveApiProfileInput::Ai {
    id,
    name: name.to_string(),
    provider_type: AiProviderType::Openai,
    api_format: AiApiFormat::OpenaiCompatible,
    base_url: "https://api.openai.com/v1".to_string(),
    default_model_id: "gpt-test".to_string(),
    api_key: key.map(str::to_string),
  }
}

fn successful_tikhub_test() -> TikhubConnectionTestResult {
  TikhubConnectionTestResult {
    success: true,
    base_url: "https://api.tikhub.io".to_string(),
    masked_email: Some("s***n@example.test".to_string()),
    balance: Some(4.0),
    free_credit: Some(1.0),
    available_credit: Some(5.0),
    email_verified: Some(true),
    api_key_status: Some(1),
    daily_usage_json: json!({"data":{"total_requests":12}}),
    message: "TikHub Token 可用".to_string(),
  }
}

#[test]
fn ai_test_must_reach_the_configured_model_endpoint() {
  let root = workspace("ai-real-connectivity");
  let (base_url, server) = serve_ai_probe_once();
  let registry = service::save_profile(
    &root,
    SaveApiProfileInput::Ai {
      id: None,
      name: "真实连通测试".to_string(),
      provider_type: AiProviderType::CustomOpenaiCompatible,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url,
      default_model_id: "model-test".to_string(),
      api_key: Some(AI_SECRET.to_string()),
    },
  )
  .unwrap();
  let profile_id = registry.ai_profiles.values().next().unwrap().id.clone();

  let result = service::test_profile(&root, ApiProfileKind::Ai, &profile_id).unwrap();
  let request_received = server.join().expect("probe server should finish");

  assert!(request_received, "AI 配置测试必须发起真实 HTTP 请求");
  assert!(result.success);
  assert!(result.message.contains("连通"));
  assert!(!result.message.contains("完整性"));
  fs::remove_dir_all(root).ok();
}

#[test]
fn ai_profile_requires_explicit_activation_after_active_profile_is_deleted() {
  let root = workspace("ai-explicit-reactivation");
  let first = service::save_profile(&root, ai_input(None, "OpenAI A", Some(AI_SECRET))).unwrap();
  let first_id = first.ai_profiles.values().next().unwrap().id.clone();
  let first_test = service::test_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
  assert_eq!(
    first_test.registry.active_profile_ids.ai.as_deref(),
    Some(first_id.as_str())
  );

  let second = service::save_profile(
    &root,
    ai_input(None, "OpenAI B", Some("sk-second-123456789")),
  )
  .unwrap();
  let second_id = second
    .ai_profiles
    .values()
    .find(|profile| profile.name == "OpenAI B")
    .unwrap()
    .id
    .clone();
  let second_test = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert_eq!(
    second_test.registry.active_profile_ids.ai.as_deref(),
    Some(first_id.as_str())
  );

  let deleted = service::delete_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
  assert!(deleted.active_profile_ids.ai.is_none());
  let retested = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert!(retested.registry.active_profile_ids.ai.is_none());
  let activated = service::activate_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert_eq!(
    activated.active_profile_ids.ai.as_deref(),
    Some(second_id.as_str())
  );
  fs::remove_dir_all(root).ok();
}

#[test]
fn tikhub_profile_requires_explicit_activation_after_active_profile_is_deleted() {
  let root = workspace("tikhub-explicit-reactivation");
  let first = service::save_profile(
    &root,
    SaveApiProfileInput::Tikhub {
      id: None,
      name: "TikHub A".to_string(),
      base_url: "https://api.tikhub.io".to_string(),
      api_key: Some(TIKHUB_SECRET.to_string()),
    },
  )
  .unwrap();
  let first_id = first.tikhub_profiles.values().next().unwrap().id.clone();
  let first_test =
    service::test_profile_with(&root, ApiProfileKind::Tikhub, &first_id, |_, _, _| {
      Ok(successful_tikhub_test())
    })
    .unwrap();
  assert_eq!(
    first_test.registry.active_profile_ids.tikhub.as_deref(),
    Some(first_id.as_str())
  );

  let second = service::save_profile(
    &root,
    SaveApiProfileInput::Tikhub {
      id: None,
      name: "TikHub B".to_string(),
      base_url: "https://api.tikhub.dev".to_string(),
      api_key: Some("tk-second-123456789".to_string()),
    },
  )
  .unwrap();
  let second_id = second
    .tikhub_profiles
    .values()
    .find(|profile| profile.name == "TikHub B")
    .unwrap()
    .id
    .clone();
  let second_test =
    service::test_profile_with(&root, ApiProfileKind::Tikhub, &second_id, |_, _, _| {
      Ok(successful_tikhub_test())
    })
    .unwrap();
  assert_eq!(
    second_test.registry.active_profile_ids.tikhub.as_deref(),
    Some(first_id.as_str())
  );

  let deleted = service::delete_profile(&root, ApiProfileKind::Tikhub, &first_id).unwrap();
  assert!(deleted.active_profile_ids.tikhub.is_none());
  let retested =
    service::test_profile_with(&root, ApiProfileKind::Tikhub, &second_id, |_, _, _| {
      Ok(successful_tikhub_test())
    })
    .unwrap();
  assert!(retested.registry.active_profile_ids.tikhub.is_none());
  let activated = service::activate_profile(&root, ApiProfileKind::Tikhub, &second_id).unwrap();
  assert_eq!(
    activated.active_profile_ids.tikhub.as_deref(),
    Some(second_id.as_str())
  );
  fs::remove_dir_all(root).ok();
}

#[test]
fn safe_views_switch_profiles_and_keep_blank_edit_keys() {
  let root = workspace("ai");
  let first = service::save_profile(&root, ai_input(None, "OpenAI A", Some(AI_SECRET))).unwrap();
  let first_id = first.ai_profiles.values().next().unwrap().id.clone();
  let view_json = serde_json::to_string(&safe_registry_view(&first)).unwrap();
  assert!(!view_json.contains(AI_SECRET));
  assert!(view_json.contains("maskedKey"));
  assert!(first.active_profile_ids.ai.is_none());

  let tested = service::test_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
  assert!(tested.success);
  assert_eq!(
    tested.registry.active_profile_ids.ai.as_deref(),
    Some(first_id.as_str())
  );

  let second = service::save_profile(
    &root,
    ai_input(None, "OpenAI B", Some("sk-second-123456789")),
  )
  .unwrap();
  let second_id = second
    .ai_profiles
    .values()
    .find(|profile| profile.name == "OpenAI B")
    .unwrap()
    .id
    .clone();
  let tested_second = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert_eq!(
    tested_second.registry.active_profile_ids.ai.as_deref(),
    Some(first_id.as_str())
  );
  let activated = service::activate_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert_eq!(
    activated.active_profile_ids.ai.as_deref(),
    Some(second_id.as_str())
  );

  let edited = service::save_profile(
    &root,
    ai_input(Some(second_id.clone()), "OpenAI B 编辑", Some("")),
  )
  .unwrap();
  let edited_profile = edited.ai_profiles.get(&second_id).unwrap();
  assert_eq!(edited_profile.status, ApiProfileStatus::Untested);
  assert!(edited
    .credentials
    .contains_key(edited_profile.credential_ref_id.as_ref().unwrap()));
  assert!(edited.active_profile_ids.ai.is_none());
  assert!(service::activate_profile(&root, ApiProfileKind::Ai, &second_id).is_err());

  assert!(
    service::test_profile(&root, ApiProfileKind::Ai, &second_id)
      .unwrap()
      .success
  );
  let reactivated = service::activate_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert_eq!(
    reactivated.active_profile_ids.ai.as_deref(),
    Some(second_id.as_str())
  );
  let deleted_current = service::delete_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert!(deleted_current.active_profile_ids.ai.is_none());

  let deleted = service::delete_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
  assert!(!deleted.ai_profiles.contains_key(&first_id));
  let audit: String = open_workspace_database(root.join(DATABASE_FILE_NAME))
    .unwrap()
    .query_row(
      "SELECT COALESCE(group_concat(safe_details_json, '\n'), '') FROM audit_log",
      [],
      |row| row.get(0),
    )
    .unwrap();
  assert!(!audit.contains(AI_SECRET));
  fs::remove_dir_all(root).ok();
}

#[test]
fn registry_status_view_does_not_open_the_secret_registry() {
  let root = workspace("safe-status-view");
  service::save_profile(&root, ai_input(None, "只读状态", Some(AI_SECRET))).unwrap();
  fs::write(
    crate::api_profiles::api_profile_registry_path(&root),
    b"{ secret registry must not be opened by status reads",
  )
  .unwrap();

  assert!(service::get_registry(&root).is_err());
  let view = service::get_registry_view(&root).unwrap();
  let view_json = serde_json::to_string(&view).unwrap();

  assert_eq!(view.ai_profiles.len(), 1);
  assert_eq!(view.ai_profiles[0].name, "只读状态");
  assert!(view.ai_profiles[0].has_credential);
  assert!(!view_json.contains(AI_SECRET));
  fs::remove_dir_all(root).ok();
}

#[test]
fn changing_ai_provider_never_reuses_the_previous_provider_key() {
  let root = workspace("ai-provider-change");
  let original = service::save_profile(&root, ai_input(None, "OpenAI", Some(AI_SECRET))).unwrap();
  let profile_id = original.ai_profiles.values().next().unwrap().id.clone();

  let rejected = service::save_profile(
    &root,
    SaveApiProfileInput::Ai {
      id: Some(profile_id.clone()),
      name: "Anthropic".to_string(),
      provider_type: AiProviderType::Anthropic,
      api_format: AiApiFormat::AnthropicMessages,
      base_url: "https://api.anthropic.com".to_string(),
      default_model_id: "claude-sonnet-4-5".to_string(),
      api_key: Some(String::new()),
    },
  )
  .unwrap_err();

  assert!(rejected.message.contains("必须重新输入 API Key"));
  let unchanged = service::get_registry(&root).unwrap();
  assert_eq!(
    unchanged
      .ai_profiles
      .get(&profile_id)
      .unwrap()
      .provider_type,
    AiProviderType::Openai
  );
  assert!(unchanged
    .credentials
    .values()
    .any(|value| value.secret == AI_SECRET));

  let ollama = service::save_profile(
    &root,
    SaveApiProfileInput::Ai {
      id: Some(profile_id.clone()),
      name: "Ollama".to_string(),
      provider_type: AiProviderType::Ollama,
      api_format: AiApiFormat::Ollama,
      base_url: "http://localhost:11434".to_string(),
      default_model_id: "qwen3".to_string(),
      api_key: Some(String::new()),
    },
  )
  .unwrap();
  assert!(ollama
    .ai_profiles
    .get(&profile_id)
    .unwrap()
    .credential_ref_id
    .is_none());
  assert!(!ollama
    .credentials
    .values()
    .any(|value| value.secret == AI_SECRET));

  let cloud_without_new_key = service::save_profile(
    &root,
    SaveApiProfileInput::Ai {
      id: Some(profile_id.clone()),
      name: "Anthropic".to_string(),
      provider_type: AiProviderType::Anthropic,
      api_format: AiApiFormat::AnthropicMessages,
      base_url: "https://api.anthropic.com".to_string(),
      default_model_id: "claude-sonnet-4-5".to_string(),
      api_key: Some(String::new()),
    },
  );
  assert!(cloud_without_new_key.is_err());

  fs::remove_dir_all(root).ok();
}

#[test]
fn changing_ai_endpoint_authority_requires_reentering_the_api_key() {
  let root = workspace("ai-endpoint-change");
  let original = service::save_profile(
    &root,
    SaveApiProfileInput::Ai {
      id: None,
      name: "自定义 AI".to_string(),
      provider_type: AiProviderType::CustomOpenaiCompatible,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url: "https://first.example.test/v1".to_string(),
      default_model_id: "model-test".to_string(),
      api_key: Some(AI_SECRET.to_string()),
    },
  )
  .unwrap();
  let profile_id = original.ai_profiles.values().next().unwrap().id.clone();

  let error = service::save_profile(
    &root,
    SaveApiProfileInput::Ai {
      id: Some(profile_id.clone()),
      name: "自定义 AI".to_string(),
      provider_type: AiProviderType::CustomOpenaiCompatible,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url: "https://second.example.test/v1".to_string(),
      default_model_id: "model-test".to_string(),
      api_key: Some(String::new()),
    },
  )
  .expect_err("changing endpoint authority must require a new key");

  assert!(error.message.contains("重新输入 API Key"));
  let unchanged = service::get_registry(&root).unwrap();
  assert_eq!(
    unchanged.ai_profiles[&profile_id].base_url,
    "https://first.example.test/v1"
  );
  assert!(unchanged
    .credentials
    .values()
    .any(|credential| credential.secret == AI_SECRET));

  fs::remove_dir_all(root).ok();
}

#[test]
fn ai_urls_enforce_official_hosts_and_secure_transports() {
  let root = workspace("ai-url-policy");
  let rejected = [
    (
      AiProviderType::Openai,
      AiApiFormat::OpenaiCompatible,
      "https://attacker.example.test/v1",
    ),
    (
      AiProviderType::Anthropic,
      AiApiFormat::AnthropicMessages,
      "https://attacker.example.test",
    ),
    (
      AiProviderType::Gemini,
      AiApiFormat::Gemini,
      "https://attacker.example.test",
    ),
    (
      AiProviderType::CustomOpenaiCompatible,
      AiApiFormat::OpenaiCompatible,
      "http://example.test/v1",
    ),
    (
      AiProviderType::Ollama,
      AiApiFormat::Ollama,
      "http://192.0.2.10:11434",
    ),
  ];
  for (provider_type, api_format, base_url) in rejected {
    let result = service::save_profile(
      &root,
      SaveApiProfileInput::Ai {
        id: None,
        name: "不安全 AI".to_string(),
        provider_type,
        api_format,
        base_url: base_url.to_string(),
        default_model_id: "model-test".to_string(),
        api_key: Some(AI_SECRET.to_string()),
      },
    );
    assert!(result.is_err(), "{provider_type:?} must reject {base_url}");
  }

  for (provider_type, api_format, base_url) in [
    (
      AiProviderType::CustomOpenaiCompatible,
      AiApiFormat::OpenaiCompatible,
      "https://gateway.example.test/v1",
    ),
    (
      AiProviderType::Ollama,
      AiApiFormat::Ollama,
      "http://127.0.0.1:11434",
    ),
  ] {
    service::save_profile(
      &root,
      SaveApiProfileInput::Ai {
        id: None,
        name: format!("安全 AI {provider_type:?}"),
        provider_type,
        api_format,
        base_url: base_url.to_string(),
        default_model_id: "model-test".to_string(),
        api_key: Some(AI_SECRET.to_string()),
      },
    )
    .unwrap_or_else(|error| panic!("{provider_type:?} should accept {base_url}: {error:?}"));
  }

  fs::remove_dir_all(root).ok();
}

#[test]
fn tikhub_test_persists_safe_summary_and_redacts_failures() {
  let root = workspace("tikhub");
  let registry = service::save_profile(
    &root,
    SaveApiProfileInput::Tikhub {
      id: None,
      name: "TikHub 主账号".to_string(),
      base_url: "https://api.tikhub.io".to_string(),
      api_key: Some(TIKHUB_SECRET.to_string()),
    },
  )
  .unwrap();
  let profile_id = registry.tikhub_profiles.values().next().unwrap().id.clone();
  let success =
    service::test_profile_with(&root, ApiProfileKind::Tikhub, &profile_id, |_, _, _| {
      Ok(successful_tikhub_test())
    })
    .unwrap();
  assert!(success.success);
  assert_eq!(
    success.registry.active_profile_ids.tikhub.as_deref(),
    Some(profile_id.as_str())
  );
  assert_eq!(
    success.registry.tikhub_profiles[&profile_id]
      .test_summary
      .as_ref()
      .unwrap()
      .today_usage,
    Some(12.0)
  );
  assert!(
    !serde_json::to_string(&safe_registry_view(&success.registry))
      .unwrap()
      .contains(TIKHUB_SECRET)
  );

  let failed = service::test_profile_with(&root, ApiProfileKind::Tikhub, &profile_id, |_, _, _| {
    Err(AppError::validation(
      format!("失败：{TIKHUB_SECRET}"),
      AppErrorStage::Collection,
    ))
  })
  .unwrap();
  assert!(!failed.success);
  assert!(!failed.message.contains(TIKHUB_SECRET));
  assert_eq!(
    failed.registry.tikhub_profiles[&profile_id].status,
    ApiProfileStatus::Failed
  );
  fs::remove_dir_all(root).ok();
}

#[test]
fn rejects_sensitive_ai_urls_before_persisting_profile_data() {
  let root = workspace("ai-sensitive-url");
  let sentinel = "url-secret-sentinel-987654321";
  let input = SaveApiProfileInput::Ai {
    id: None,
    name: "不安全端点".to_string(),
    provider_type: AiProviderType::CustomOpenaiCompatible,
    api_format: AiApiFormat::OpenaiCompatible,
    base_url: format!("https://user:{sentinel}@example.test/v1?api_key={sentinel}#token"),
    default_model_id: "model-test".to_string(),
    api_key: Some(AI_SECRET.to_string()),
  };

  let error = service::save_profile(&root, input).unwrap_err();

  assert!(error.message.contains("AI Base URL"));
  let registry_json = fs::read(root.join("secrets/api-config.json")).unwrap();
  let database = fs::read(root.join(DATABASE_FILE_NAME)).unwrap();
  assert!(!registry_json
    .windows(sentinel.len())
    .any(|value| value == sentinel.as_bytes()));
  assert!(!database
    .windows(sentinel.len())
    .any(|value| value == sentinel.as_bytes()));
  fs::remove_dir_all(root).ok();
}

#[test]
fn active_runtime_snapshot_blocks_tikhub_edit_and_delete() {
  let root = workspace("snapshot");
  let registry = service::save_profile(
    &root,
    SaveApiProfileInput::Tikhub {
      id: None,
      name: "快照账号".to_string(),
      base_url: "https://api.tikhub.io".to_string(),
      api_key: Some(TIKHUB_SECRET.to_string()),
    },
  )
  .unwrap();
  let profile = registry.tikhub_profiles.values().next().unwrap();
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  connection
    .execute_batch("DROP TRIGGER trg_collection_runtime_snapshot_insert;")
    .unwrap();
  connection.execute(
    "INSERT INTO collection_task (id,name,source_type,status,created_at,updated_at) VALUES ('task','t','form','running',?1,?1)",
    params!["2026-07-17T00:00:00+00:00"],
  ).unwrap();
  connection.execute(
    "INSERT INTO task_run (id,task_id,status,started_at,claimed_at) VALUES ('run','task','running',?1,?1)",
    params!["2026-07-17T00:00:00+00:00"],
  ).unwrap();
  connection
    .execute(
      "INSERT INTO collection_runtime_snapshot (
       id,task_run_id,workspace_id,runtime_contract_version,plan_id,plan_schema_version,
       plan_json,connector_type,connector_id,connector_config_version,base_url,secret_ref_id,
       secret_revision,secret_provider_type,secret_provider_id,connector_tested_at,
       connector_test_status,created_at
     ) SELECT 'snapshot','run',id,1,'plan',2,'{}','tikhub','default',1,?1,?2,1,
              'tikhub',?3,?4,'success',?4 FROM workspace",
      params![
        profile.base_url,
        profile.credential_ref_id,
        profile.id,
        "2026-07-17T00:00:00+00:00"
      ],
    )
    .unwrap();
  drop(connection);

  let edit = SaveApiProfileInput::Tikhub {
    id: Some(profile.id.clone()),
    name: "禁止编辑".to_string(),
    base_url: profile.base_url.clone(),
    api_key: None,
  };
  assert!(service::save_profile(&root, edit).is_err());
  assert!(service::delete_profile(&root, ApiProfileKind::Tikhub, &profile.id).is_err());
  fs::remove_dir_all(root).ok();
}

#[test]
fn legacy_runtime_snapshots_block_profiles_by_stable_credential_reference() {
  for status in ["queued", "running"] {
    let root = workspace(&format!("legacy-snapshot-{status}"));
    let registry = service::save_profile(
      &root,
      SaveApiProfileInput::Tikhub {
        id: None,
        name: "迁移后账号".to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        api_key: Some(TIKHUB_SECRET.to_string()),
      },
    )
    .unwrap();
    let profile = registry.tikhub_profiles.values().next().unwrap();
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute_batch("DROP TRIGGER trg_collection_runtime_snapshot_insert;")
      .unwrap();
    connection.execute(
      "INSERT INTO collection_task (id,name,source_type,status,created_at,updated_at) VALUES ('task','t','form',?1,?2,?2)",
      params![status, "2026-07-17T00:00:00+00:00"],
    ).unwrap();
    connection.execute(
      "INSERT INTO task_run (id,task_id,status,started_at,claimed_at,current_stage) VALUES ('run','task',?1,?2,?2,'恢复待发送')",
      params![status, "2026-07-17T00:00:00+00:00"],
    ).unwrap();
    connection
      .execute(
        "INSERT INTO collection_runtime_snapshot (
         id,task_run_id,workspace_id,runtime_contract_version,plan_id,plan_schema_version,
         plan_json,connector_type,connector_id,connector_config_version,base_url,secret_ref_id,
         secret_revision,secret_provider_type,secret_provider_id,connector_tested_at,
         connector_test_status,created_at
       ) SELECT 'legacy-snapshot','run',id,1,'plan',2,'{}','tikhub','default',1,?1,?2,1,
                'tikhub','default',?3,'success',?3 FROM workspace",
        params![
          profile.base_url,
          profile.credential_ref_id,
          "2026-07-17T00:00:00+00:00"
        ],
      )
      .unwrap();
    drop(connection);

    let edit = SaveApiProfileInput::Tikhub {
      id: Some(profile.id.clone()),
      name: "不应写入".to_string(),
      base_url: profile.base_url.clone(),
      api_key: None,
    };
    let edit_error = service::save_profile(&root, edit).unwrap_err();
    let delete_error =
      service::delete_profile(&root, ApiProfileKind::Tikhub, &profile.id).unwrap_err();
    assert!(!edit_error.message.contains(TIKHUB_SECRET));
    assert!(!delete_error.message.contains(TIKHUB_SECRET));

    let unchanged = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .unwrap()
      .query_row(
        "SELECT secret_provider_id,secret_ref_id FROM collection_runtime_snapshot WHERE id = 'legacy-snapshot'",
        [],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
      )
      .unwrap();
    assert_eq!(
      unchanged,
      ("default".to_string(), profile.credential_ref_id.clone())
    );
    assert_eq!(
      service::get_registry(&root).unwrap().tikhub_profiles[&profile.id].name,
      "迁移后账号"
    );
    fs::remove_dir_all(root).ok();
  }
}

fn serve_ai_probe_once() -> (String, thread::JoinHandle<bool>) {
  let listener = TcpListener::bind("127.0.0.1:0").expect("probe server should bind");
  listener
    .set_nonblocking(true)
    .expect("probe listener should be nonblocking");
  let address = listener.local_addr().expect("probe address should resolve");
  let server = thread::spawn(move || {
    let deadline = Instant::now() + Duration::from_secs(2);
    let (mut stream, _) = loop {
      match listener.accept() {
        Ok(connection) => break connection,
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
          if Instant::now() >= deadline {
            return false;
          }
          thread::sleep(Duration::from_millis(10));
        }
        Err(error) => panic!("probe server failed: {error}"),
      }
    };
    stream
      .set_nonblocking(false)
      .expect("accepted probe stream should block while reading");
    stream
      .set_read_timeout(Some(Duration::from_secs(2)))
      .expect("probe request read timeout should set");
    let mut request = Vec::new();
    let mut buffer = [0_u8; 512];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
      let bytes_read = stream.read(&mut buffer).expect("probe request should read");
      if bytes_read == 0 {
        break;
      }
      request.extend_from_slice(&buffer[..bytes_read]);
      assert!(
        request.len() <= 32 * 1024,
        "probe request headers too large"
      );
    }
    let request = String::from_utf8_lossy(&request);
    assert!(request.starts_with("POST /v1/chat/completions HTTP/1.1"));
    let content = r#"{"ok":true}"#;
    let body = serde_json::json!({
      "choices": [{ "message": { "content": content } }],
      "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
    })
    .to_string();
    write!(
      stream,
      "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
      body.len()
    )
    .expect("probe response should write");
    true
  });
  (format!("http://{address}"), server)
}
