use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::collection::validate_collection_plan_v2;
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::planning::generate_plan_json;
use crate::prompts::seed_builtin_prompts;
use crate::tasks::{
  save_collection_plan, update_collection_task, CollectionPlanView, SaveCollectionPlanInput,
  UpdateCollectionTaskInput,
};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GenerateCollectionPlanFromTextInput {
  pub task_id: String,
  pub intent_text: String,
  pub provider_id: Option<String>,
  pub model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeSnapshotView {
  pub id: String,
  pub task_id: String,
  pub agent_profile_id: Option<String>,
  pub provider_id: String,
  pub model_id: String,
  pub api_format: String,
  pub base_url_type: String,
  pub prompt_version_id: String,
  pub output_schema_id: String,
  pub capabilities_json: Value,
  pub config_source: String,
  pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiRunView {
  pub id: String,
  pub task_id: String,
  pub runtime_snapshot_id: String,
  pub run_type: String,
  pub input_record_set_id: Option<String>,
  pub input_summary: Option<String>,
  pub output_json: Option<Value>,
  pub raw_output_path: Option<String>,
  pub schema_valid: bool,
  pub validation_status: String,
  pub error_code: Option<String>,
  pub error_message: Option<String>,
  pub input_tokens: Option<i64>,
  pub output_tokens: Option<i64>,
  pub latency_ms: Option<i64>,
  pub first_token_latency_ms: Option<i64>,
  pub retry_count: i64,
  pub cost_estimate_json: Value,
  pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneratedCollectionPlanView {
  pub ai_run: AiRunView,
  pub runtime_snapshot: RuntimeSnapshotView,
  pub collection_plan: CollectionPlanView,
}

pub fn generate_collection_plan_from_text(
  root_path: impl AsRef<Path>,
  input: GenerateCollectionPlanFromTextInput,
) -> AppResult<GeneratedCollectionPlanView> {
  if input.provider_id.is_some() || input.model_id.is_some() {
    return Err(ai_error(
      "当前自然语言计划仅支持本地规则解析，真实模型调用尚未接入",
    ));
  }

  let root_path = root_path.as_ref().to_path_buf();
  seed_builtin_prompts(&root_path)?;
  let connection = open_workspace_connection(&root_path)?;
  ensure_task_exists(&connection, &input.task_id)?;
  let prompt = active_prompt_version(&connection, "collection_plan_from_text")?;
  let generated = generate_plan_json(&input.intent_text);
  let generated_platforms = json_string_array(generated.get("platforms"));
  let generated_data_types = json_string_array(generated.get("data_types"));
  if !generated_platforms.is_empty() && !generated_data_types.is_empty() {
    update_collection_task(
      &root_path,
      &input.task_id,
      UpdateCollectionTaskInput {
        name: None,
        platforms: Some(generated_platforms),
        data_types: Some(generated_data_types),
      },
    )?;
  }
  let plan_validation = validate_collection_plan_v2(&generated);
  let schema_valid = plan_validation.valid;
  let validation_status = if schema_valid {
    "valid"
  } else {
    "needs_review"
  };
  let now = Utc::now().to_rfc3339();
  let runtime_snapshot_id = Uuid::new_v4().to_string();
  let ai_run_id = Uuid::new_v4().to_string();
  let provider_id = "local-rule-engine";
  let model_id = "rule-parser-v1";

  connection
    .execute(
      "INSERT INTO runtime_snapshot (
        id, task_id, provider_id, model_id, api_format, base_url_type, prompt_version_id,
        output_schema_id, capabilities_json, config_source, created_at
      ) VALUES (?1, ?2, ?3, ?4, 'local_rule', 'none', ?5, 'collection_plan_v2', ?6, 'local', ?7)",
      params![
        runtime_snapshot_id,
        input.task_id,
        provider_id,
        model_id,
        prompt.id,
        serde_json::json!({ "structured_output": true }).to_string(),
        now
      ],
    )
    .map_err(database_error)?;

  connection
    .execute(
      "INSERT INTO ai_run (
        id, task_id, runtime_snapshot_id, run_type, input_summary, output_json, schema_valid,
        validation_status, input_tokens, output_tokens, latency_ms, retry_count,
        cost_estimate_json, created_at
      ) VALUES (?1, ?2, ?3, 'collection_plan_generation', ?4, ?5, ?6, ?7, ?8, ?9, 0, 0, ?10, ?11)",
      params![
        ai_run_id,
        input.task_id,
        runtime_snapshot_id,
        input.intent_text,
        generated.to_string(),
        bool_to_i64(schema_valid),
        validation_status,
        estimate_tokens(&input.intent_text),
        estimate_tokens(&generated.to_string()),
        generated
          .get("cost_estimate")
          .cloned()
          .unwrap_or_else(|| serde_json::json!({}))
          .to_string(),
        now
      ],
    )
    .map_err(database_error)?;

  connection
    .execute(
      "INSERT INTO task_intent (id, task_id, intent_text, language, parse_status, ai_run_id, created_at)
       VALUES (?1, ?2, ?3, 'zh-CN', ?4, ?5, ?6)",
      params![
        Uuid::new_v4().to_string(),
        input.task_id,
        input.intent_text,
        validation_status,
        ai_run_id,
        now
      ],
    )
    .map_err(database_error)?;

  let collection_plan = save_collection_plan(
    &root_path,
    SaveCollectionPlanInput {
      task_id: input.task_id.clone(),
      source: "ai_generated".to_string(),
      plan_json: generated.clone(),
      validation_status: validation_status.to_string(),
      validation_errors_json: Some(serde_json::json!(plan_validation.errors)),
      cost_estimate_json: generated.get("cost_estimate").cloned(),
    },
  )?;
  let ai_run = get_ai_run(&root_path, &ai_run_id)?;
  let runtime_snapshot = get_runtime_snapshot(&connection, &runtime_snapshot_id)?;

  Ok(GeneratedCollectionPlanView {
    ai_run,
    runtime_snapshot,
    collection_plan,
  })
}

pub fn get_ai_run(root_path: impl AsRef<Path>, ai_run_id: &str) -> AppResult<AiRunView> {
  let connection = open_workspace_connection(root_path)?;
  connection
    .query_row(
      "SELECT id, task_id, runtime_snapshot_id, run_type, input_record_set_id, input_summary,
              output_json, raw_output_path, schema_valid, validation_status, error_code,
              error_message, input_tokens, output_tokens, latency_ms, first_token_latency_ms,
              retry_count, cost_estimate_json, created_at
       FROM ai_run
       WHERE id = ?1",
      params![ai_run_id],
      map_ai_run,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| ai_error("AI 运行记录不存在"))
}

pub fn list_ai_runs(
  root_path: impl AsRef<Path>,
  task_id: String,
  run_type: Option<String>,
) -> AppResult<Vec<AiRunView>> {
  let connection = open_workspace_connection(root_path)?;

  if let Some(run_type) = run_type {
    let mut statement = connection
      .prepare(
        "SELECT id, task_id, runtime_snapshot_id, run_type, input_record_set_id, input_summary,
                output_json, raw_output_path, schema_valid, validation_status, error_code,
                error_message, input_tokens, output_tokens, latency_ms, first_token_latency_ms,
                retry_count, cost_estimate_json, created_at
         FROM ai_run
         WHERE task_id = ?1 AND run_type = ?2
         ORDER BY created_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![task_id, run_type], map_ai_run)
      .map_err(database_error)?;
    collect_rows(rows)
  } else {
    let mut statement = connection
      .prepare(
        "SELECT id, task_id, runtime_snapshot_id, run_type, input_record_set_id, input_summary,
                output_json, raw_output_path, schema_valid, validation_status, error_code,
                error_message, input_tokens, output_tokens, latency_ms, first_token_latency_ms,
                retry_count, cost_estimate_json, created_at
         FROM ai_run
         WHERE task_id = ?1
         ORDER BY created_at DESC",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![task_id], map_ai_run)
      .map_err(database_error)?;
    collect_rows(rows)
  }
}

fn json_string_array(value: Option<&Value>) -> Vec<String> {
  value
    .and_then(Value::as_array)
    .map(|values| {
      values
        .iter()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect()
    })
    .unwrap_or_default()
}

fn active_prompt_version(
  connection: &Connection,
  template_key: &str,
) -> AppResult<ActivePromptVersion> {
  connection
    .query_row(
      "SELECT pv.id
       FROM prompt_version pv
       JOIN prompt_template pt ON pt.id = pv.template_id
       WHERE pt.template_key = ?1 AND pv.status = 'active'
       ORDER BY pv.version DESC
       LIMIT 1",
      params![template_key],
      |row| Ok(ActivePromptVersion { id: row.get(0)? }),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| ai_error("缺少可用的 active 提示词版本"))
}

fn ensure_task_exists(connection: &Connection, task_id: &str) -> AppResult<()> {
  let exists = connection
    .query_row(
      "SELECT COUNT(*) FROM collection_task WHERE id = ?1",
      params![task_id],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?
    > 0;

  if exists {
    Ok(())
  } else {
    Err(ai_error("任务不存在"))
  }
}

fn get_runtime_snapshot(
  connection: &Connection,
  snapshot_id: &str,
) -> AppResult<RuntimeSnapshotView> {
  connection
    .query_row(
      "SELECT id, task_id, agent_profile_id, provider_id, model_id, api_format, base_url_type,
              prompt_version_id, output_schema_id, capabilities_json, config_source, created_at
       FROM runtime_snapshot
       WHERE id = ?1",
      params![snapshot_id],
      map_runtime_snapshot,
    )
    .map_err(database_error)
}

fn map_runtime_snapshot(row: &Row<'_>) -> rusqlite::Result<RuntimeSnapshotView> {
  Ok(RuntimeSnapshotView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    agent_profile_id: row.get(2)?,
    provider_id: row.get(3)?,
    model_id: row.get(4)?,
    api_format: row.get(5)?,
    base_url_type: row.get(6)?,
    prompt_version_id: row.get(7)?,
    output_schema_id: row.get(8)?,
    capabilities_json: string_to_json(row.get(9)?),
    config_source: row.get(10)?,
    created_at: row.get(11)?,
  })
}

fn map_ai_run(row: &Row<'_>) -> rusqlite::Result<AiRunView> {
  let output_json = row.get::<_, Option<String>>(6)?.map(string_to_json);

  Ok(AiRunView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    runtime_snapshot_id: row.get(2)?,
    run_type: row.get(3)?,
    input_record_set_id: row.get(4)?,
    input_summary: row.get(5)?,
    output_json,
    raw_output_path: row.get(7)?,
    schema_valid: i64_to_bool(row.get(8)?),
    validation_status: row.get(9)?,
    error_code: row.get(10)?,
    error_message: row.get(11)?,
    input_tokens: row.get(12)?,
    output_tokens: row.get(13)?,
    latency_ms: row.get(14)?,
    first_token_latency_ms: row.get(15)?,
    retry_count: row.get(16)?,
    cost_estimate_json: string_to_json(row.get(17)?),
    created_at: row.get(18)?,
  })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> AppResult<Vec<T>> {
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn open_workspace_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
}

fn string_to_json(value: String) -> Value {
  serde_json::from_str(&value).unwrap_or_else(|_| serde_json::json!({}))
}

fn bool_to_i64(value: bool) -> i64 {
  if value {
    1
  } else {
    0
  }
}

fn i64_to_bool(value: i64) -> bool {
  value != 0
}

fn estimate_tokens(text: &str) -> i64 {
  (text.chars().count() as i64 / 2).max(1)
}

fn ai_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::Ai,
    false,
  )
}

fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
}

#[derive(Debug)]
struct ActivePromptVersion {
  id: String,
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::tasks::{create_collection_task, get_task, CreateCollectionTaskInput};
  use crate::workspace::create_workspace;

  #[test]
  fn text_generation_saves_ai_run_snapshot_and_plan() {
    let root_path = unique_temp_workspace("ai-plan");
    create_workspace("AI 测试", &root_path).expect("workspace should be created");
    let task = create_collection_task(
      &root_path,
      CreateCollectionTaskInput {
        name: "自然语言任务".to_string(),
        source_type: "natural_language".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["comments".to_string()],
      },
    )
    .expect("task should be created");

    let result = generate_collection_plan_from_text(
      &root_path,
      GenerateCollectionPlanFromTextInput {
        task_id: task.id,
        intent_text: "采集美国 TikTok 汽车评论".to_string(),
        provider_id: None,
        model_id: None,
      },
    )
    .expect("plan should generate");
    let runs = list_ai_runs(
      &root_path,
      result.ai_run.task_id.clone(),
      Some("collection_plan_generation".to_string()),
    )
    .expect("runs should list");

    assert!(!result.ai_run.schema_valid);
    assert_eq!(result.ai_run.validation_status, "needs_review");
    assert_eq!(
      result.runtime_snapshot.output_schema_id,
      "collection_plan_v2"
    );
    assert_eq!(result.collection_plan.validation_status, "needs_review");
    assert!(result
      .collection_plan
      .validation_errors_json
      .as_array()
      .is_some_and(|errors| errors
        .iter()
        .filter_map(Value::as_str)
        .any(|error| error.contains("item_id"))));
    assert_eq!(runs.len(), 1);

    std::fs::remove_dir_all(root_path).ok();
  }

  #[test]
  fn local_rule_generation_rejects_unimplemented_model_selection() {
    let root_path = unique_temp_workspace("ai-model-selection");
    create_workspace("AI 模型边界测试", &root_path).expect("workspace should be created");
    let task = create_collection_task(
      &root_path,
      CreateCollectionTaskInput {
        name: "不能伪装成真实模型调用".to_string(),
        source_type: "natural_language".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["comments".to_string()],
      },
    )
    .expect("task should be created");

    let error = generate_collection_plan_from_text(
      &root_path,
      GenerateCollectionPlanFromTextInput {
        task_id: task.id,
        intent_text: "采集美国 TikTok 汽车评论".to_string(),
        provider_id: Some("provider-openai".to_string()),
        model_id: Some("gpt-test".to_string()),
      },
    )
    .expect_err("local rule path must reject unimplemented model selection");

    assert_eq!(error.code, AppErrorCode::ValidationError);
    assert!(error.message.contains("本地规则"));

    std::fs::remove_dir_all(root_path).ok();
  }

  #[test]
  fn domestic_platform_text_infers_cn_region() {
    let generated = generate_plan_json("采集小红书汽车评论");

    assert_eq!(generated["region"]["value"], "CN");
    assert_eq!(generated["missing_fields"], serde_json::json!([]));
  }

  #[test]
  fn multi_platform_plan_updates_task_scope_and_builds_every_step() {
    let root_path = unique_temp_workspace("ai-multi-plan");
    create_workspace("AI 测试", &root_path).expect("workspace should be created");
    let task = create_collection_task(
      &root_path,
      CreateCollectionTaskInput {
        name: "多平台自然语言任务".to_string(),
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
        intent_text: "同时采集美国 TikTok 和抖音的评论与关键词".to_string(),
        provider_id: None,
        model_id: None,
      },
    )
    .expect("multi-platform plan should generate");
    let updated_task = get_task(&root_path, &task.id).expect("task should reload");

    assert_eq!(
      updated_task.platforms_json,
      serde_json::json!(["tiktok", "douyin"])
    );
    assert_eq!(
      updated_task.data_types_json,
      serde_json::json!(["comments", "keyword_search"])
    );
    assert_eq!(
      result.collection_plan.plan_json["steps"]
        .as_array()
        .map(Vec::len),
      Some(4)
    );
    assert_eq!(result.collection_plan.validation_status, "needs_review");

    std::fs::remove_dir_all(root_path).ok();
  }

  fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
  }
}
