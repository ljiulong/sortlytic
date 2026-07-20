use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::api_profiles::{
  load_api_profile_registry, AiApiFormat, AiProviderType, ApiProfileStatus,
};
use crate::collection::validate_collection_plan_v4;
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::prompts::seed_builtin_prompts;
use crate::tasks::{
  save_collection_plan, update_collection_task, CollectionPlanView, SaveCollectionPlanInput,
  UpdateCollectionTaskInput,
};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

mod attempts;
#[allow(dead_code)]
pub(crate) mod collection_intent_schema;
pub(crate) mod collection_plan_schema;
pub(crate) mod provider_client;

pub(crate) use attempts::mark_interrupted_task_intents;
pub use attempts::{list_latest_task_intents, NaturalParseAttemptView};
pub use collection_intent_schema::{CollectionIntentV1, IntentAgeRange};

use collection_plan_schema::validate_collection_plan_schema;
use provider_client::{call_model, collection_plan_request, ProviderConfig};

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

#[derive(Debug, Clone, PartialEq)]
pub struct PromptRegressionModelOutput {
  pub provider_id: String,
  pub model_id: String,
  pub output_json: Value,
}

pub fn run_collection_prompt_regression(
  root_path: impl AsRef<Path>,
  prompt_content: &str,
  intent_text: &str,
) -> AppResult<PromptRegressionModelOutput> {
  let root_path = root_path.as_ref();
  let profile = active_ai_profile(
    root_path,
    &GenerateCollectionPlanFromTextInput {
      task_id: String::new(),
      intent_text: intent_text.to_string(),
      provider_id: None,
      model_id: None,
    },
  )?;
  let request = collection_plan_request(prompt_content, intent_text);
  let response = call_model(&profile.config, &request)?;
  let schema_errors = validate_collection_plan_schema(&response.output_json);
  if !schema_errors.is_empty() {
    return Err(ai_error(schema_errors.join("；")));
  }

  Ok(PromptRegressionModelOutput {
    provider_id: profile.profile_id,
    model_id: profile.config.model_id,
    output_json: normalize_model_plan(response.output_json),
  })
}

pub fn generate_collection_plan_from_text(
  root_path: impl AsRef<Path>,
  input: GenerateCollectionPlanFromTextInput,
) -> AppResult<GeneratedCollectionPlanView> {
  let root_path = root_path.as_ref().to_path_buf();
  let intent_text = input.intent_text.trim();
  if intent_text.is_empty() {
    return Err(ai_error("自然语言采集需求不能为空"));
  }
  let connection = open_workspace_connection(&root_path)?;
  ensure_task_exists(&connection, &input.task_id)?;
  let now = Utc::now().to_rfc3339();
  let attempt_id = Uuid::new_v4().to_string();
  create_task_intent_attempt(&connection, &attempt_id, &input.task_id, intent_text, &now)?;
  preserve_attempt_error(
    seed_builtin_prompts(&root_path),
    &connection,
    &attempt_id,
    "preparing",
  )?;
  let prompt = preserve_attempt_error(
    active_prompt_version(&connection, "collection_plan_from_text"),
    &connection,
    &attempt_id,
    "preparing",
  )?;
  let profile = preserve_attempt_error(
    active_ai_profile(&root_path, &input),
    &connection,
    &attempt_id,
    "preparing",
  )?;
  let runtime_snapshot_id = Uuid::new_v4().to_string();
  let ai_run_id = Uuid::new_v4().to_string();

  update_task_intent_phase(&connection, &attempt_id, "requesting_ai", None)?;
  preserve_attempt_error(
    connection
      .execute(
        "INSERT INTO runtime_snapshot (
        id, task_id, provider_id, model_id, api_format, base_url_type, prompt_version_id,
        output_schema_id, capabilities_json, config_source, created_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'collection_plan_v4', ?8, 'active_api_profile', ?9)",
        params![
          runtime_snapshot_id,
          input.task_id,
          profile.profile_id,
          profile.config.model_id,
          api_format_name(profile.config.api_format),
          base_url_type(profile.config.provider_type),
          prompt.id,
          serde_json::json!({
            "structured_output": true,
            "schema_enforced_locally": true,
            "prompt_content_hash": prompt.content_hash,
            "provider_type": provider_type_name(profile.config.provider_type)
          })
          .to_string(),
          now
        ],
      )
      .map(|_| ())
      .map_err(database_error),
    &connection,
    &attempt_id,
    "requesting_ai",
  )?;

  let request = collection_plan_request(&prompt.content, intent_text);
  let call_started_at = std::time::Instant::now();
  let response = match call_model(&profile.config, &request) {
    Ok(response) => response,
    Err(error) => {
      let latency_ms = i64::try_from(call_started_at.elapsed().as_millis()).unwrap_or(i64::MAX);
      persist_failed_ai_run(
        &connection,
        FailedAiRunInput {
          ai_run_id: &ai_run_id,
          attempt_id: &attempt_id,
          task_id: &input.task_id,
          runtime_snapshot_id: &runtime_snapshot_id,
          intent_text,
          error: &error,
          latency_ms,
          created_at: &now,
        },
      )?;
      return Err(error);
    }
  };
  update_task_intent_phase(&connection, &attempt_id, "validating_intent", None)?;
  let schema_errors = validate_collection_plan_schema(&response.output_json);
  let generated = normalize_model_plan(response.output_json);
  let mut plan_validation = validate_collection_plan_v4(&generated);
  plan_validation.errors.extend(schema_errors);
  plan_validation.errors.sort();
  plan_validation.errors.dedup();
  plan_validation.valid = plan_validation.errors.is_empty();
  let schema_valid = plan_validation.valid;
  let validation_status = if schema_valid {
    "valid"
  } else {
    "needs_review"
  };

  preserve_attempt_error(
    connection
      .execute(
      "INSERT INTO ai_run (
        id, task_id, runtime_snapshot_id, run_type, input_summary, output_json, schema_valid,
        validation_status, input_tokens, output_tokens, latency_ms, retry_count,
        cost_estimate_json, created_at
      ) VALUES (?1, ?2, ?3, 'collection_plan_generation', ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12)",
      params![
        ai_run_id,
        input.task_id,
        runtime_snapshot_id,
        intent_text,
        generated.to_string(),
        bool_to_i64(schema_valid),
        validation_status,
        response.input_tokens,
        response.output_tokens,
        response.latency_ms,
        generated
          .get("cost_estimate")
          .cloned()
          .unwrap_or_else(|| serde_json::json!({}))
          .to_string(),
        now
      ],
      )
      .map(|_| ())
      .map_err(database_error),
    &connection,
    &attempt_id,
    "validating_intent",
  )?;

  update_task_intent_phase(&connection, &attempt_id, "building_plan", Some(&ai_run_id))?;
  let generated_platforms = json_string_array(generated.get("platforms"));
  if schema_valid
    && !generated_platforms.is_empty()
    && generated.get("entity").and_then(Value::as_str) == Some("account")
  {
    preserve_attempt_error(
      update_collection_task(
        &root_path,
        &input.task_id,
        UpdateCollectionTaskInput {
          name: None,
          platforms: Some(generated_platforms),
          data_types: Some(vec!["account".to_string()]),
        },
      )
      .map(|_| ()),
      &connection,
      &attempt_id,
      "building_plan",
    )?;
  }

  let collection_plan = preserve_attempt_error(
    save_collection_plan(
      &root_path,
      SaveCollectionPlanInput {
        task_id: input.task_id.clone(),
        source: "ai_generated".to_string(),
        plan_json: generated.clone(),
        validation_status: validation_status.to_string(),
        validation_errors_json: Some(serde_json::json!(plan_validation.errors)),
        cost_estimate_json: generated.get("cost_estimate").cloned(),
      },
    ),
    &connection,
    &attempt_id,
    "building_plan",
  )?;
  update_task_intent_success(&connection, &attempt_id, validation_status, &ai_run_id)?;
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

fn active_ai_profile(
  root_path: &Path,
  input: &GenerateCollectionPlanFromTextInput,
) -> AppResult<ResolvedAiProfile> {
  let registry = load_api_profile_registry(root_path)?;
  let profile_id = registry
    .active_profile_ids
    .ai
    .as_deref()
    .ok_or_else(|| ai_error("尚未设置当前 AI 配置，请先在设置中完成真实连通性测试"))?;
  let profile = registry
    .ai_profiles
    .get(profile_id)
    .ok_or_else(|| ai_error("当前 AI 配置不存在，请重新选择并测试"))?;
  if profile.status != ApiProfileStatus::Success {
    return Err(ai_error(
      "当前 AI 配置尚未通过真实连通性测试，请先在设置中测试",
    ));
  }
  if input
    .provider_id
    .as_deref()
    .is_some_and(|selected| selected != profile.id)
  {
    return Err(ai_error("请求指定的 AI 配置与当前配置不一致"));
  }
  if input
    .model_id
    .as_deref()
    .is_some_and(|selected| selected != profile.default_model_id)
  {
    return Err(ai_error("请求指定的模型与当前 AI 配置不一致"));
  }
  let api_key = profile
    .credential_ref_id
    .as_ref()
    .and_then(|credential_id| registry.credentials.get(credential_id))
    .map(|credential| credential.secret.clone());
  if profile.provider_type != AiProviderType::Ollama && api_key.is_none() {
    return Err(ai_error("当前 AI 配置缺少 API Key，请重新输入并测试"));
  }
  Ok(ResolvedAiProfile {
    profile_id: profile.id.clone(),
    config: ProviderConfig {
      provider_type: profile.provider_type,
      api_format: profile.api_format,
      base_url: profile.base_url.clone(),
      model_id: profile.default_model_id.clone(),
      api_key,
    },
  })
}

fn persist_failed_ai_run(connection: &Connection, input: FailedAiRunInput<'_>) -> AppResult<()> {
  let error_code = serde_json::to_value(&input.error.code)
    .ok()
    .and_then(|value| value.as_str().map(ToString::to_string))
    .unwrap_or_else(|| "MODEL_PROTOCOL_ERROR".to_string());
  connection
    .execute(
      "INSERT INTO ai_run (
        id, task_id, runtime_snapshot_id, run_type, input_summary, schema_valid,
        validation_status, error_code, error_message, latency_ms, retry_count,
        cost_estimate_json, created_at
      ) VALUES (?1, ?2, ?3, 'collection_plan_generation', ?4, 0, 'failed', ?5, ?6, ?7, 0, '{}', ?8)",
      params![
        input.ai_run_id,
        input.task_id,
        input.runtime_snapshot_id,
        input.intent_text,
        error_code,
        input.error.message,
        input.latency_ms,
        input.created_at
      ],
    )
    .map_err(database_error)?;
  update_task_intent_failure(
    connection,
    input.attempt_id,
    "requesting_ai",
    input.error,
    Some(input.ai_run_id),
  )
}

fn create_task_intent_attempt(
  connection: &Connection,
  attempt_id: &str,
  task_id: &str,
  intent_text: &str,
  now: &str,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO task_intent (
        id, task_id, intent_text, language, parse_status, parse_phase,
        error_safe_details_json, created_at, updated_at
      ) VALUES (?1, ?2, ?3, 'zh-CN', 'running', 'preparing', '{}', ?4, ?4)",
      params![attempt_id, task_id, intent_text, now],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn update_task_intent_phase(
  connection: &Connection,
  attempt_id: &str,
  phase: &str,
  ai_run_id: Option<&str>,
) -> AppResult<()> {
  connection
    .execute(
      "UPDATE task_intent
       SET parse_phase = ?1, ai_run_id = COALESCE(?2, ai_run_id), updated_at = ?3
       WHERE id = ?4",
      params![phase, ai_run_id, Utc::now().to_rfc3339(), attempt_id],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn update_task_intent_success(
  connection: &Connection,
  attempt_id: &str,
  parse_status: &str,
  ai_run_id: &str,
) -> AppResult<()> {
  connection
    .execute(
      "UPDATE task_intent
       SET parse_status = ?1, parse_phase = 'success', ai_run_id = ?2,
           error_code = NULL, error_message = NULL, retryable = NULL,
           error_safe_details_json = '{}', updated_at = ?3
       WHERE id = ?4",
      params![parse_status, ai_run_id, Utc::now().to_rfc3339(), attempt_id],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn update_task_intent_failure(
  connection: &Connection,
  attempt_id: &str,
  phase: &str,
  error: &AppError,
  ai_run_id: Option<&str>,
) -> AppResult<()> {
  connection
    .execute(
      "UPDATE task_intent
       SET parse_status = 'failed', parse_phase = ?1, ai_run_id = COALESCE(?2, ai_run_id),
           error_code = ?3, error_message = ?4, retryable = ?5,
           error_safe_details_json = ?6, updated_at = ?7
       WHERE id = ?8",
      params![
        phase,
        ai_run_id,
        error_code_name(error),
        error.message,
        bool_to_i64(error.retryable),
        serde_json::to_string(&error.safe_details).unwrap_or_else(|_| "{}".to_string()),
        Utc::now().to_rfc3339(),
        attempt_id
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn preserve_attempt_error<T>(
  result: AppResult<T>,
  connection: &Connection,
  attempt_id: &str,
  phase: &str,
) -> AppResult<T> {
  result.map_err(|error| {
    let _ = update_task_intent_failure(connection, attempt_id, phase, &error, None);
    error
  })
}

fn error_code_name(error: &AppError) -> String {
  serde_json::to_value(&error.code)
    .ok()
    .and_then(|value| value.as_str().map(ToString::to_string))
    .unwrap_or_else(|| "MODEL_PROTOCOL_ERROR".to_string())
}

fn normalize_model_plan(mut output: Value) -> Value {
  if let Some(steps) = output.get_mut("steps").and_then(Value::as_array_mut) {
    for step in steps {
      if let Some(params) = step.get_mut("params").and_then(Value::as_object_mut) {
        params.retain(|_, value| !value.is_null());
      }
    }
  }
  output
}

fn api_format_name(format: AiApiFormat) -> &'static str {
  match format {
    AiApiFormat::OpenaiCompatible => "openai_compatible",
    AiApiFormat::AnthropicMessages => "anthropic_messages",
    AiApiFormat::Gemini => "gemini",
    AiApiFormat::Ollama => "ollama",
  }
}

fn provider_type_name(provider_type: AiProviderType) -> &'static str {
  match provider_type {
    AiProviderType::Openai => "openai",
    AiProviderType::Anthropic => "anthropic",
    AiProviderType::Gemini => "gemini",
    AiProviderType::CustomOpenaiCompatible => "custom_openai_compatible",
    AiProviderType::Ollama => "ollama",
  }
}

fn base_url_type(provider_type: AiProviderType) -> &'static str {
  match provider_type {
    AiProviderType::Openai | AiProviderType::Anthropic | AiProviderType::Gemini => "official",
    AiProviderType::CustomOpenaiCompatible => "custom",
    AiProviderType::Ollama => "local",
  }
}

fn active_prompt_version(
  connection: &Connection,
  template_key: &str,
) -> AppResult<ActivePromptVersion> {
  connection
    .query_row(
      "SELECT pv.id, pv.content, pv.content_hash
       FROM prompt_version pv
       JOIN prompt_template pt ON pt.id = pv.template_id
       WHERE pt.template_key = ?1 AND pv.status = 'active'
       ORDER BY pv.version DESC
       LIMIT 1",
      params![template_key],
      |row| {
        Ok(ActivePromptVersion {
          id: row.get(0)?,
          content: row.get(1)?,
          content_hash: row.get(2)?,
        })
      },
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
  content: String,
  content_hash: String,
}

#[derive(Debug)]
struct ResolvedAiProfile {
  profile_id: String,
  config: ProviderConfig,
}

struct FailedAiRunInput<'a> {
  ai_run_id: &'a str,
  attempt_id: &'a str,
  task_id: &'a str,
  runtime_snapshot_id: &'a str,
  intent_text: &'a str,
  error: &'a AppError,
  latency_ms: i64,
  created_at: &'a str,
}

#[cfg(test)]
mod tests;
