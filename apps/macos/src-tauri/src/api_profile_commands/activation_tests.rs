use std::fs;

use serde_json::json;
use uuid::Uuid;

use super::{service, AiApiFormat, AiProviderType, ApiProfileKind, SaveApiProfileInput};
use crate::api_profiles::ApiProfileStatus;
use crate::domain::{AppError, AppErrorStage};
use crate::tikhub::TikhubConnectionTestResult;
use crate::workspace::create_workspace;

const AI_SECRET: &str = "sk-activation-regression-123456789";
const TIKHUB_SECRET: &str = "tk-activation-regression-987654321";

fn workspace(label: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("api-activation-{label}-{}", Uuid::new_v4()));
  create_workspace("API 自动激活回归测试", &root).unwrap();
  root
}

fn ai_input(id: Option<String>, name: &str, model: &str, key: Option<&str>) -> SaveApiProfileInput {
  SaveApiProfileInput::Ai {
    id,
    name: name.to_string(),
    provider_type: AiProviderType::Openai,
    api_format: AiApiFormat::OpenaiCompatible,
    base_url: "https://api.openai.com/v1".to_string(),
    default_model_id: model.to_string(),
    api_key: key.map(str::to_string),
  }
}

fn tikhub_input(name: &str, key: &str) -> SaveApiProfileInput {
  SaveApiProfileInput::Tikhub {
    id: None,
    name: name.to_string(),
    base_url: "https://api.tikhub.io".to_string(),
    api_key: Some(key.to_string()),
  }
}

fn successful_tikhub_test() -> TikhubConnectionTestResult {
  TikhubConnectionTestResult {
    success: true,
    base_url: "https://api.tikhub.io".to_string(),
    masked_email: Some("a***t@example.test".to_string()),
    balance: Some(4.0),
    free_credit: Some(1.0),
    available_credit: Some(5.0),
    email_verified: Some(true),
    api_key_status: Some(1),
    daily_usage_json: json!({"data":{"total_requests":12}}),
    message: "TikHub Token 可用".to_string(),
  }
}

fn test_tikhub_success(root: &std::path::Path, id: &str) -> service::ServiceTestResult {
  service::test_profile_with(root, ApiProfileKind::Tikhub, id, |_, _, _| {
    Ok(successful_tikhub_test())
  })
  .unwrap()
}

#[test]
fn failed_tikhub_profile_requires_explicit_activation_after_current_is_deleted() {
  let root = workspace("tikhub-failed");
  let first = service::save_profile(&root, tikhub_input("TikHub A", TIKHUB_SECRET)).unwrap();
  let first_id = first.tikhub_profiles.values().next().unwrap().id.clone();
  let first_test = test_tikhub_success(&root, &first_id);
  assert_eq!(
    first_test.registry.active_profile_ids.tikhub.as_deref(),
    Some(first_id.as_str())
  );

  let second = service::save_profile(
    &root,
    tikhub_input("TikHub B", "tk-second-activation-123456789"),
  )
  .unwrap();
  let second_id = second
    .tikhub_profiles
    .values()
    .find(|profile| profile.name == "TikHub B")
    .unwrap()
    .id
    .clone();
  let failed = service::test_profile_with(&root, ApiProfileKind::Tikhub, &second_id, |_, _, _| {
    Err(AppError::validation(
      "TikHub 测试失败",
      AppErrorStage::Collection,
    ))
  })
  .unwrap();
  assert!(!failed.success);
  assert_eq!(
    failed.registry.tikhub_profiles[&second_id].status,
    ApiProfileStatus::Failed
  );

  let deleted = service::delete_profile(&root, ApiProfileKind::Tikhub, &first_id).unwrap();
  assert!(deleted.active_profile_ids.tikhub.is_none());

  let retested = test_tikhub_success(&root, &second_id);
  assert!(retested.registry.active_profile_ids.tikhub.is_none());

  let activated = service::activate_profile(&root, ApiProfileKind::Tikhub, &second_id).unwrap();
  assert_eq!(
    activated.active_profile_ids.tikhub.as_deref(),
    Some(second_id.as_str())
  );
  fs::remove_dir_all(root).ok();
}

#[test]
fn incomplete_ai_profile_requires_explicit_activation_after_current_is_deleted() {
  let root = workspace("ai-needs-rebind");
  let first = service::save_profile(
    &root,
    ai_input(None, "OpenAI A", "gpt-test", Some(AI_SECRET)),
  )
  .unwrap();
  let first_id = first.ai_profiles.values().next().unwrap().id.clone();
  let first_test = service::test_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
  assert_eq!(
    first_test.registry.active_profile_ids.ai.as_deref(),
    Some(first_id.as_str())
  );

  let second = service::save_profile(
    &root,
    ai_input(None, "OpenAI B", "", Some("sk-second-activation-987654321")),
  )
  .unwrap();
  let second_id = second
    .ai_profiles
    .values()
    .find(|profile| profile.name == "OpenAI B")
    .unwrap()
    .id
    .clone();
  let incomplete = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert!(!incomplete.success);
  assert_eq!(
    incomplete.registry.ai_profiles[&second_id].status,
    ApiProfileStatus::NeedsRebind
  );

  let deleted = service::delete_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
  assert!(deleted.active_profile_ids.ai.is_none());
  service::save_profile(
    &root,
    ai_input(Some(second_id.clone()), "OpenAI B", "gpt-test", None),
  )
  .unwrap();

  let retested = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert!(retested.success);
  assert!(retested.registry.active_profile_ids.ai.is_none());

  let activated = service::activate_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
  assert_eq!(
    activated.active_profile_ids.ai.as_deref(),
    Some(second_id.as_str())
  );
  fs::remove_dir_all(root).ok();
}
