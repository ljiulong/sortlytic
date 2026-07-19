use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

use super::*;
use crate::api_profiles::{
  save_api_profile_registry, ActiveApiProfileIds, AiApiFormat, AiApiProfile, AiProviderType,
  ApiCredential, ApiProfileRegistry, ApiProfileStatus, CredentialProviderType,
};
use crate::workspace::{
  create_workspace, open_workspace, open_workspace_database, DATABASE_FILE_NAME,
};

#[test]
fn seed_builtin_prompts_is_idempotent() {
  let root_path = unique_temp_workspace("prompts");
  create_workspace("提示词测试", &root_path).expect("workspace should be created");

  let templates = seed_builtin_prompts(&root_path).expect("builtins should seed");
  let collection_template = templates
    .iter()
    .find(|template| template.template_key == "collection_plan_from_text")
    .expect("collection template exists");
  let versions =
    list_prompt_versions(&root_path, &collection_template.id).expect("versions should list");
  let first_cases =
    list_prompt_regression_cases(&root_path, &collection_template.id).expect("cases should list");

  seed_builtin_prompts(&root_path).expect("repeated seed should succeed");
  let second_cases =
    list_prompt_regression_cases(&root_path, &collection_template.id).expect("cases should list");

  assert_eq!(templates.len(), 3);
  assert_eq!(
    collection_template.output_schema_id.as_deref(),
    Some("collection_plan_v4")
  );
  assert_eq!(versions[0].status, "active");
  assert!(versions[0].content.contains("collection_plan_v4"));
  assert!(versions[0].content.contains("证据"));
  assert_eq!(first_cases.len(), 3);
  assert!(first_cases
    .iter()
    .all(|case| case.expected_schema_id == "collection_plan_v4"));
  assert_eq!(second_cases.len(), first_cases.len());
  assert_eq!(
    second_cases
      .iter()
      .map(|case| case.name.as_str())
      .collect::<std::collections::BTreeSet<_>>()
      .len(),
    second_cases.len()
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn seeding_upgrades_a_legacy_collection_prompt_without_losing_history() {
  let root_path = unique_temp_workspace("prompt-v3-upgrade");
  create_workspace("提示词升级测试", &root_path).expect("workspace should be created");
  let templates = seed_builtin_prompts(&root_path).expect("builtins should seed");
  let template = templates
    .iter()
    .find(|template| template.template_key == "collection_plan_from_text")
    .expect("collection template exists");
  let original = list_prompt_versions(&root_path, &template.id)
    .expect("versions should list")
    .remove(0);
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");
  connection
    .execute(
      "UPDATE prompt_template SET output_schema_id = 'collection_plan_v1' WHERE id = ?1",
      params![template.id],
    )
    .expect("template should simulate legacy metadata");
  connection
    .execute(
      "UPDATE prompt_version SET content = 'legacy v1', content_hash = ?1 WHERE id = ?2",
      params![content_hash("legacy v1"), original.id],
    )
    .expect("version should simulate legacy content");
  drop(connection);

  let upgraded_templates = seed_builtin_prompts(&root_path).expect("legacy prompt should upgrade");
  let upgraded_template = upgraded_templates
    .iter()
    .find(|candidate| candidate.id == template.id)
    .expect("same template should be upgraded in place");
  let upgraded_versions =
    list_prompt_versions(&root_path, &template.id).expect("versions should list");

  assert_eq!(
    upgraded_template.output_schema_id.as_deref(),
    Some("collection_plan_v4")
  );
  assert_eq!(upgraded_versions.len(), 2);
  assert_eq!(upgraded_versions[0].status, "active");
  assert!(upgraded_versions[0].content.contains("collection_plan_v4"));
  assert_eq!(upgraded_versions[1].status, "archived");
  assert_eq!(upgraded_versions[1].content, "legacy v1");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn builtin_upgrade_preserves_the_user_activated_prompt_version() {
  let root_path = unique_temp_workspace("prompt-user-active-upgrade");
  create_workspace("用户提示词升级保护", &root_path).expect("workspace should be created");
  let templates = seed_builtin_prompts(&root_path).expect("builtins should seed");
  let template = templates
    .iter()
    .find(|template| template.template_key == "collection_plan_from_text")
    .expect("collection template exists");
  let original = list_prompt_versions(&root_path, &template.id)
    .expect("versions should list")
    .remove(0);
  let custom_id = Uuid::new_v4().to_string();
  let custom_content = "用户显式激活的自定义提示词";
  let now = Utc::now().to_rfc3339();
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");
  connection
    .execute(
      "UPDATE prompt_version
       SET content = 'legacy builtin', content_hash = ?1, status = 'archived', activated_at = NULL
       WHERE id = ?2",
      params![content_hash("legacy builtin"), original.id],
    )
    .expect("builtin version should simulate an older release");
  connection
    .execute(
      "INSERT INTO prompt_version (
        id, template_id, version, content, change_note, status, created_at, activated_at,
        content_hash
      ) VALUES (?1, ?2, 2, ?3, '用户自定义', 'active', ?4, ?4, ?5)",
      params![
        custom_id,
        template.id,
        custom_content,
        now,
        content_hash(custom_content)
      ],
    )
    .expect("custom active version should insert");
  drop(connection);

  seed_builtin_prompts(&root_path).expect("new builtin should seed without taking activation");
  let versions = list_prompt_versions(&root_path, &template.id).expect("versions should list");
  let custom = versions
    .iter()
    .find(|version| version.id == custom_id)
    .expect("custom version should remain");
  let current_builtin = versions
    .iter()
    .find(|version| version.content.contains("collection_plan_v4"))
    .expect("current builtin should be added");

  assert_eq!(custom.status, "active");
  assert_eq!(custom.activated_at.as_deref(), Some(now.as_str()));
  assert_eq!(current_builtin.status, "draft");
  assert_eq!(
    versions
      .iter()
      .filter(|version| version.status == "active")
      .count(),
    1
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn opening_legacy_workspace_deduplicates_cases_and_preserves_runs() {
  let root_path = unique_temp_workspace("prompt-case-migration");
  create_workspace("提示词测试", &root_path).expect("workspace should be created");
  let templates = seed_builtin_prompts(&root_path).expect("builtins should seed");
  let template = templates
    .iter()
    .find(|template| template.template_key == "collection_plan_from_text")
    .expect("template exists");
  let cases = list_prompt_regression_cases(&root_path, &template.id).expect("cases should list");
  let source_case = cases.first().expect("source case exists");
  let version = list_prompt_versions(&root_path, &template.id)
    .expect("versions should list")
    .remove(0);
  let duplicate_id = Uuid::new_v4().to_string();
  let run_id = Uuid::new_v4().to_string();
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should open");

  connection
    .execute(
      "DROP INDEX IF EXISTS idx_prompt_regression_case_template_name",
      [],
    )
    .expect("legacy schema should allow dropping the index");
  connection
    .execute(
      "INSERT INTO prompt_regression_case (
        id, template_id, name, input_json, expected_schema_id, expected_rules_json,
        enabled, created_at, updated_at
      )
      SELECT ?1, template_id, name, input_json, expected_schema_id, expected_rules_json,
             enabled, created_at, updated_at
      FROM prompt_regression_case
      WHERE id = ?2",
      params![duplicate_id, source_case.id],
    )
    .expect("legacy duplicate should insert");
  connection
    .execute(
      "INSERT INTO prompt_regression_run (
        id, template_id, prompt_version_id, case_id, status, schema_valid, rules_valid,
        created_at
      ) VALUES (?1, ?2, ?3, ?4, 'passed', 1, 1, ?5)",
      params![
        run_id,
        template.id,
        version.id,
        duplicate_id,
        Utc::now().to_rfc3339()
      ],
    )
    .expect("legacy run should insert");
  drop(connection);

  open_workspace(&root_path).expect("legacy workspace should migrate while opening");

  let migrated_cases =
    list_prompt_regression_cases(&root_path, &template.id).expect("cases should list");
  let connection =
    open_workspace_database(root_path.join(DATABASE_FILE_NAME)).expect("database should reopen");
  let migrated_case_id = connection
    .query_row(
      "SELECT case_id FROM prompt_regression_run WHERE id = ?1",
      params![run_id],
      |row| row.get::<_, String>(0),
    )
    .expect("run should remain after migration");

  assert_eq!(migrated_cases.len(), cases.len());
  assert_ne!(migrated_case_id, duplicate_id);
  assert!(migrated_cases
    .iter()
    .any(|case| case.id == migrated_case_id));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn activation_rejects_prompt_that_ignores_case_contract() {
  let root_path = unique_temp_workspace("prompt-regression");
  create_workspace("提示词测试", &root_path).expect("workspace should be created");
  let templates = seed_builtin_prompts(&root_path).expect("builtins should seed");
  let template = templates
    .iter()
    .find(|template| template.template_key == "collection_plan_from_text")
    .expect("template exists");
  let version = create_prompt_version(
    &root_path,
    CreatePromptVersionInput {
      template_id: template.id.clone(),
      content: "输出 JSON，包含 platforms 和 missing_fields".to_string(),
      change_note: "测试版本".to_string(),
    },
  )
  .expect("version created");

  let error = activate_prompt_version(&root_path, &version.id)
    .expect_err("field-name-only prompt must not pass real cases");
  let runs = list_prompt_regression_runs(&root_path, &version.id).expect("runs should list");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(!runs.is_empty());
  assert!(runs.iter().any(|run| run.status == "failed"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn evaluator_result_changes_when_case_input_violates_expected_rules() {
  let root_path = unique_temp_workspace("prompt-static-contract");
  create_workspace("提示词静态约束测试", &root_path).expect("workspace should be created");
  let version = PromptVersionView {
    id: "version-1".to_string(),
    template_id: "template-1".to_string(),
    version: 1,
    content: "只输出 JSON，包含 platforms 和 missing_fields".to_string(),
    change_note: "测试".to_string(),
    status: "draft".to_string(),
    created_at: "2026-01-01T00:00:00Z".to_string(),
    activated_at: None,
    rollback_from_version: None,
    content_hash: "hash".to_string(),
  };
  let case = PromptRegressionCaseView {
    id: "case-1".to_string(),
    template_id: "template-1".to_string(),
    name: "预期完整输入".to_string(),
    input_json: serde_json::json!({ "text": "采集汽车评论" }),
    expected_schema_id: "collection_plan_v4".to_string(),
    expected_rules_json: serde_json::json!({
      "expected_platforms": ["tiktok"],
      "expected_account_source": "comment_authors",
      "expected_selected_fields": [],
      "expected_missing_fields": [],
      "expected_plan_valid": false
    }),
    enabled: true,
    created_at: "2026-01-01T00:00:00Z".to_string(),
    updated_at: "2026-01-01T00:00:00Z".to_string(),
  };

  let evaluation = evaluate_prompt_case(&root_path, &version, &case);

  assert!(!evaluation.schema_valid);
  assert!(!evaluation.rules_valid);
  assert!(evaluation.provider_id.is_none());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn complete_builtin_contract_executes_all_cases_and_can_activate() {
  let root_path = unique_temp_workspace("prompt-regression-success");
  create_workspace("提示词测试", &root_path).expect("workspace should be created");
  let templates = seed_builtin_prompts(&root_path).expect("builtins should seed");
  let template = templates
    .iter()
    .find(|template| template.template_key == "collection_plan_from_text")
    .expect("collection template exists");
  let builtin = BUILTIN_PROMPTS
    .iter()
    .find(|builtin| builtin.key == "collection_plan_from_text")
    .expect("builtin contract exists");
  let version = create_prompt_version(
    &root_path,
    CreatePromptVersionInput {
      template_id: template.id.clone(),
      content: builtin.content.to_string(),
      change_note: "验证真实回归路径".to_string(),
    },
  )
  .expect("version should create");
  let (base_url, server) = serve_prompt_regressions(3, false);
  let profile_id = configure_active_ai(&root_path, base_url);

  let activated =
    activate_prompt_version(&root_path, &version.id).expect("complete contract should activate");
  server.join().expect("regression server should finish");
  let runs = list_prompt_regression_runs(&root_path, &version.id).expect("runs should list");

  assert_eq!(activated.status, "active");
  assert_eq!(runs.len(), 3);
  assert!(runs.iter().all(|run| run.schema_valid && run.rules_valid));
  assert!(runs
    .iter()
    .all(|run| run.provider_id.as_deref() == Some(profile_id.as_str())));
  assert!(runs
    .iter()
    .all(|run| run.model_id.as_deref() == Some("prompt-regression-test")));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn activation_rejects_model_output_missing_a_required_schema_field() {
  let root_path = unique_temp_workspace("prompt-regression-schema");
  create_workspace("提示词 Schema 回归", &root_path).expect("workspace should be created");
  let templates = seed_builtin_prompts(&root_path).expect("builtins should seed");
  let template = templates
    .iter()
    .find(|template| template.template_key == "collection_plan_from_text")
    .expect("collection template exists");
  let builtin = BUILTIN_PROMPTS
    .iter()
    .find(|builtin| builtin.key == "collection_plan_from_text")
    .expect("builtin contract exists");
  let version = create_prompt_version(
    &root_path,
    CreatePromptVersionInput {
      template_id: template.id.clone(),
      content: builtin.content.to_string(),
      change_note: "拒绝不完整模型输出".to_string(),
    },
  )
  .expect("version should create");
  let (base_url, server) = serve_prompt_regressions(3, true);
  configure_active_ai(&root_path, base_url);

  let error = activate_prompt_version(&root_path, &version.id)
    .expect_err("missing output_selected must fail prompt regression");
  server.join().expect("regression server should finish");
  let versions = list_prompt_versions(&root_path, &template.id).expect("versions should list");
  let failed = versions
    .iter()
    .find(|candidate| candidate.id == version.id)
    .expect("candidate version should remain");
  let runs = list_prompt_regression_runs(&root_path, &version.id).expect("runs should list");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert_eq!(failed.status, "failed_regression");
  assert_eq!(runs.len(), 3);
  assert!(runs.iter().all(|run| !run.schema_valid));

  std::fs::remove_dir_all(root_path).ok();
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
      name: "提示词回归 AI".to_string(),
      provider_type: AiProviderType::CustomOpenaiCompatible,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url,
      default_model_id: "prompt-regression-test".to_string(),
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
      secret: "sk-prompt-regression-secret".to_string(),
    },
  );
  save_api_profile_registry(root_path, &registry).expect("AI registry should save");
  profile_id
}

fn serve_prompt_regressions(
  expected_requests: usize,
  omit_output_selected: bool,
) -> (String, thread::JoinHandle<()>) {
  let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
  let address = listener.local_addr().expect("test address should resolve");
  let server = thread::spawn(move || {
    for _ in 0..expected_requests {
      let (mut stream, _) = listener.accept().expect("test server should accept");
      let request = read_http_request(&mut stream);
      assert!(request.contains("input_json.text"));
      assert!(request.contains("collection_plan_v4"));
      let mut plan = if request.contains("新能源汽车") {
        valid_account_plan(
          "douyin",
          &["avatar_url", "gender", "age", "followers_count"],
          3_000_000,
        )
      } else if request.contains("智能汽车") {
        valid_account_plan(
          "xiaohongshu",
          &["avatar_url", "bio", "followers_count", "posts_count"],
          4_000_000,
        )
      } else {
        valid_account_plan(
          "tiktok",
          &["avatar_url", "bio", "country_region", "followers_count"],
          2_000_000,
        )
      };
      if omit_output_selected {
        plan["steps"][0]
          .as_object_mut()
          .expect("step should be an object")
          .remove("output_selected");
      }
      let body = serde_json::json!({
        "choices": [{ "message": { "content": plan.to_string() } }],
        "usage": { "prompt_tokens": 40, "completion_tokens": 80 }
      })
      .to_string();
      write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
      )
      .expect("response should be writable");
    }
  });
  (format!("http://{address}"), server)
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
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

fn valid_account_plan(platform: &str, selected_fields: &[&str], budget_micros: i64) -> Value {
  let mut plan = crate::collection::generate_account_collection_plan(
    crate::collection::AccountFormCollectionPlanRequest {
      platform: platform.to_string(),
      account_source: "user_search".to_string(),
      selected_fields: selected_fields
        .iter()
        .map(|field| (*field).to_string())
        .collect(),
      enrichment_policy: "auto_costed".to_string(),
      params: serde_json::json!({ "keyword": "car" }),
      age_range: None,
      gender_filter: None,
      request_limit: Some(1),
      record_limit: Some(20),
      budget_limit_micros: Some(budget_micros),
    },
  )
  .expect("account plan fixture should generate")
  .plan_json;
  for step in plan["steps"]
    .as_array_mut()
    .expect("account plan steps should be an array")
  {
    let params = step["params"]
      .as_object_mut()
      .expect("account plan params should be an object");
    for key in [
      "keyword",
      "item_id",
      "account_id",
      "region",
      "time_range",
      "page_size",
    ] {
      params.entry(key.to_string()).or_insert(Value::Null);
    }
  }
  plan["confidence"] = serde_json::json!(0.98);
  plan["missing_fields"] = serde_json::json!([]);
  plan
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
