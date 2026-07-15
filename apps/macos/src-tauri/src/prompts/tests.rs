use super::*;
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
  assert_eq!(versions[0].status, "active");
  assert_eq!(first_cases.len(), 3);
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
  let version = PromptVersionView {
    id: "version-1".to_string(),
    template_id: "template-1".to_string(),
    version: 1,
    content: "读取 input_json.text，只输出 JSON 采集计划，包含 platforms、data_types、region、steps、missing_fields 和 requires_user_confirmation，不得猜测缺失信息。".to_string(),
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
    expected_schema_id: "collection_plan_v1".to_string(),
    expected_rules_json: serde_json::json!({
      "expected_platforms": ["tiktok"],
      "expected_data_types": ["comments"],
      "expected_missing_fields": [],
      "expected_plan_valid": false
    }),
    enabled: true,
    created_at: "2026-01-01T00:00:00Z".to_string(),
    updated_at: "2026-01-01T00:00:00Z".to_string(),
  };

  let (schema_valid, rules_valid, _) = evaluate_prompt_case(&version, &case);

  assert!(schema_valid);
  assert!(!rules_valid);
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

  let activated =
    activate_prompt_version(&root_path, &version.id).expect("complete contract should activate");
  let runs = list_prompt_regression_runs(&root_path, &version.id).expect("runs should list");

  assert_eq!(activated.status, "active");
  assert_eq!(runs.len(), 3);
  assert!(runs.iter().all(|run| run.schema_valid && run.rules_valid));

  std::fs::remove_dir_all(root_path).ok();
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
