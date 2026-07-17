use super::*;
use crate::workspace::create_workspace;

#[test]
fn provider_and_model_profile_round_trip() {
  let root_path = unique_temp_workspace("provider");
  create_workspace("供应商测试", &root_path).expect("workspace should be created");

  let provider = create_model_provider(&root_path, provider_input("openai"))
    .expect("provider should be created");
  let profile = upsert_model_profile(&root_path, profile_input("openai", "gpt-test"))
    .expect("profile should be upserted");
  set_default_model(&root_path, "openai", "gpt-test").expect("default model should be set");

  let providers = list_model_providers(&root_path, Some(true)).expect("providers should list");
  let profiles = list_model_profiles(&root_path, "openai").expect("profiles should list");
  let test_result =
    test_model_provider(&root_path, "openai", Some("gpt-test".to_string())).expect("test ok");

  assert_eq!(provider.provider_id, "openai");
  assert_eq!(profile.model_id, "gpt-test");
  assert_eq!(providers.len(), 1);
  assert_eq!(providers[0].default_model_id, Some("gpt-test".to_string()));
  assert_eq!(profiles.len(), 1);
  assert!(test_result.success);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn custom_openai_compatible_provider_requires_base_url() {
  let mut input = provider_input("custom");
  input.api_format = "openai_compatible".to_string();
  input.base_url = None;

  let error = normalize_provider_input(input).expect_err("base url should be required");

  assert_eq!(error.code, AppErrorCode::ValidationError);
}

#[test]
fn activating_a_provider_disables_its_siblings_atomically() {
  let root_path = unique_temp_workspace("active-provider");
  create_workspace("激活供应商测试", &root_path).expect("workspace should be created");
  create_model_provider(&root_path, provider_input("openai")).expect("openai should create");
  create_model_provider(&root_path, provider_input("ollama")).expect("ollama should create");

  set_active_model_provider(&root_path, "ollama").expect("ollama should activate");

  let providers = list_model_providers(&root_path, None).expect("providers should list");
  assert_eq!(
    providers
      .iter()
      .filter(|provider| provider.enabled)
      .map(|provider| provider.provider_id.as_str())
      .collect::<Vec<_>>(),
    vec!["ollama"]
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn deleting_a_missing_provider_reports_that_nothing_was_deleted() {
  let root_path = unique_temp_workspace("delete-missing-provider");
  create_workspace("删除供应商测试", &root_path).expect("workspace should be created");

  let deleted = delete_model_provider(&root_path, "missing-provider")
    .expect("deleting a missing provider should be handled");

  assert!(!deleted);
  let connection = open_workspace_connection(&root_path).expect("database should open");
  let audit_count = connection
    .query_row(
      "SELECT COUNT(*) FROM audit_log WHERE action = 'delete_model_provider'",
      [],
      |row| row.get::<_, i64>(0),
    )
    .expect("audit log should be queryable");
  assert_eq!(audit_count, 0);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn listing_a_provider_rejects_corrupted_json_instead_of_silently_using_empty_object() {
  let root_path = unique_temp_workspace("corrupted-provider-json");
  create_workspace("供应商 JSON 测试", &root_path).expect("workspace should be created");
  create_model_provider(&root_path, provider_input("openai")).expect("provider should be created");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE model_provider SET cost_policy_json = '{not-json' WHERE provider_id = 'openai'",
      [],
    )
    .expect("test should corrupt provider JSON");
  drop(connection);

  let error = list_model_providers(&root_path, None)
    .expect_err("corrupted provider JSON must not be silently accepted");

  assert_eq!(error.code, AppErrorCode::DatabaseError);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn model_provider_rejects_a_tikhub_secret_reference() {
  let root_path = unique_temp_workspace("provider-secret-type");
  create_workspace("供应商密钥类型测试", &root_path).expect("workspace should be created");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO secret_ref (
         id, provider_type, provider_id, secret_store_key, masked_hint,
         created_at, updated_at
       ) VALUES ('tikhub-secret', 'tikhub', 'default', 'keychain-ref', 'safe-hint', ?1, ?1)",
      params![Utc::now().to_rfc3339()],
    )
    .expect("fixture secret should insert");

  let mut input = provider_input("openai");
  input.auth_type = "api_key".to_string();
  input.secret_ref_id = Some("tikhub-secret".to_string());
  let error = create_model_provider(&root_path, input)
    .expect_err("a model provider must reject a TikHub secret");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert_eq!(
    list_model_providers(&root_path, None)
      .expect("providers should list")
      .len(),
    0
  );
  std::fs::remove_dir_all(root_path).ok();
}

fn provider_input(provider_id: &str) -> ModelProviderInput {
  ModelProviderInput {
    provider_id: provider_id.to_string(),
    display_name: provider_id.to_string(),
    enabled: Some(true),
    auth_type: "none".to_string(),
    secret_ref_id: None,
    base_url: Some("http://localhost:11434/v1".to_string()),
    api_format: "ollama".to_string(),
    region: None,
    cost_policy_json: None,
    rate_limit_policy_json: None,
    health_check_json: None,
  }
}

fn profile_input(provider_id: &str, model_id: &str) -> ModelProfileInput {
  ModelProfileInput {
    provider_id: provider_id.to_string(),
    model_id: model_id.to_string(),
    display_name: model_id.to_string(),
    capabilities_json: Some(serde_json::json!({ "text": true })),
    context_window: Some(128_000),
    supports_structured_output: Some(true),
    supports_streaming: Some(true),
    supports_tools: Some(false),
    supports_vision: Some(false),
    enabled: Some(true),
  }
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
