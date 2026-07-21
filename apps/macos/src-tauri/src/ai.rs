use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api_profiles::{
  load_api_profile_registry, AiApiFormat, AiProviderType, ApiProfileStatus,
};
use crate::domain::{redact_sensitive_text, AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::tasks::CollectionPlanView;
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

mod attempts;
pub(crate) mod collection_intent_schema;
mod generation;
pub(crate) mod intent_plan;
pub(crate) mod provider_client;
mod provider_errors;
mod provider_policy;

pub(crate) use attempts::mark_interrupted_task_intents;
pub use attempts::{list_latest_task_intents, list_task_intents, NaturalParseAttemptView};
pub use collection_intent_schema::{CollectionIntentV1, IntentAgeRange};
pub use generation::generate_collection_plan_from_text;
pub use intent_plan::IntentPlanBuildResult;

use collection_intent_schema::parse_collection_intent;
use provider_client::{call_model_for_intent, collection_intent_request, ProviderConfig};

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
  pub parsed_intent: Option<CollectionIntentV1>,
  pub issues: Vec<String>,
  pub collection_plan: Option<CollectionPlanView>,
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
  let request = collection_intent_request(prompt_content, intent_text);
  let response = call_model_for_intent(&profile.config, &request)?;
  let parsed = parse_collection_intent(&response.output_json).map_err(|errors| {
    ai_error(format!(
      "真实模型回归未通过 collection_intent_v1 Schema：{}",
      errors.join("；")
    ))
  })?;

  Ok(PromptRegressionModelOutput {
    provider_id: profile.profile_id,
    model_id: profile.config.model_id,
    output_json: serde_json::to_value(parsed)
      .map_err(|_| ai_error("无法序列化真实模型回归意图"))?,
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

fn active_ai_profile(
  root_path: &Path,
  input: &GenerateCollectionPlanFromTextInput,
) -> AppResult<ResolvedAiProfile> {
  let registry = load_api_profile_registry(root_path)?;
  let profile_id = registry
    .active_profile_ids
    .ai
    .as_deref()
    .ok_or_else(|| ai_config_error("尚未设置当前 AI 配置，请先在设置中完成真实连通性测试"))?;
  let profile = registry
    .ai_profiles
    .get(profile_id)
    .ok_or_else(|| ai_config_error("当前 AI 配置不存在，请重新选择并测试"))?;
  if profile.status != ApiProfileStatus::Success {
    return Err(ai_config_error(
      "当前 AI 配置尚未通过真实连通性测试，请先在设置中测试",
    ));
  }
  if input
    .provider_id
    .as_deref()
    .is_some_and(|selected| selected != profile.id)
  {
    return Err(ai_config_error("请求指定的 AI 配置与当前配置不一致"));
  }
  if input
    .model_id
    .as_deref()
    .is_some_and(|selected| selected != profile.default_model_id)
  {
    return Err(ai_config_error("请求指定的模型与当前 AI 配置不一致"));
  }
  let api_key = profile
    .credential_ref_id
    .as_ref()
    .and_then(|credential_id| registry.credentials.get(credential_id))
    .map(|credential| credential.secret.clone());
  if profile.provider_type != AiProviderType::Ollama && api_key.is_none() {
    return Err(ai_config_error(
      "当前 AI 配置缺少 API Key，请重新输入并测试",
    ));
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
      ) VALUES (?1, ?2, ?3, 'collection_intent_generation', ?4, 0, 'failed', ?5, ?6, ?7, 0, '{}', ?8)",
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
  issues: &[String],
  missing_fields: &[String],
) -> AppResult<()> {
  let now = Utc::now().to_rfc3339();
  if parse_status == "valid" {
    return connection
      .execute(
        "UPDATE task_intent
         SET parse_status = 'valid', parse_phase = 'success', ai_run_id = ?1,
             error_code = NULL, error_message = NULL, retryable = NULL,
             error_safe_details_json = '{}', updated_at = ?2
         WHERE id = ?3",
        params![ai_run_id, now, attempt_id],
      )
      .map(|_| ())
      .map_err(database_error);
  }

  let safe_issues = issues
    .iter()
    .map(|issue| redact_sensitive_text(issue))
    .collect::<Vec<_>>();
  let safe_missing_fields = missing_fields
    .iter()
    .map(|field| redact_sensitive_text(field))
    .collect::<Vec<_>>();
  let mut message_parts = Vec::new();
  if !safe_issues.is_empty() {
    message_parts.push(safe_issues.join("；"));
  }
  if !safe_missing_fields.is_empty() {
    message_parts.push(format!("缺少字段：{}", safe_missing_fields.join("、")));
  }
  let error_message = if message_parts.is_empty() {
    "解析完成，但意图或计划需要补充信息".to_string()
  } else {
    format!("解析完成，需要修正：{}", message_parts.join("；"))
  };
  let safe_details = serde_json::json!({
    "issues": safe_issues,
    "missing_fields": safe_missing_fields,
  });
  connection
    .execute(
      "UPDATE task_intent
       SET parse_status = 'needs_review', parse_phase = 'needs_review', ai_run_id = ?1,
           error_code = 'VALIDATION_ERROR', error_message = ?2, retryable = 0,
           error_safe_details_json = ?3, updated_at = ?4
       WHERE id = ?5",
      params![
        ai_run_id,
        error_message,
        safe_details.to_string(),
        now,
        attempt_id
      ],
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
  result.inspect_err(|error| {
    let _ = update_task_intent_failure(connection, attempt_id, phase, error, None);
  })
}

fn error_code_name(error: &AppError) -> String {
  serde_json::to_value(&error.code)
    .ok()
    .and_then(|value| value.as_str().map(ToString::to_string))
    .unwrap_or_else(|| "MODEL_PROTOCOL_ERROR".to_string())
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

fn prepare_task_for_natural_parse(connection: &Connection, task_id: &str) -> AppResult<()> {
  let task = connection
    .query_row(
      "SELECT source_type, status FROM collection_task WHERE id = ?1",
      params![task_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| ai_error("任务不存在"))?;

  if task.0 != "natural_language" {
    return Err(ai_error("只有自然语言来源的任务可以调用 AI 重新解析"));
  }
  match task.1.as_str() {
    "draft" | "waiting_confirmation" => Ok(()),
    "failed" | "cancelled" => connection
      .execute(
        "UPDATE collection_task
         SET status = 'draft', confirmed_at = NULL, completed_at = NULL, cancelled_at = NULL,
             cost_estimate_json = '{}', actual_cost_json = '{}', updated_at = ?1
         WHERE id = ?2 AND status IN ('failed', 'cancelled')",
        params![Utc::now().to_rfc3339(), task_id],
      )
      .map(|_| ())
      .map_err(database_error),
    "queued" | "running" => Err(ai_error(
      "排队或运行中的任务必须先取消，才能重新解析自然语言需求",
    )),
    "success" | "partial_success" => Err(ai_error(
      "成功或部分成功任务不能原地重新解析，请使用“复制并编辑”创建新任务",
    )),
    _ => Err(ai_error("当前任务状态不支持重新解析自然语言需求")),
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

fn ai_config_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ModelConfigError,
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
