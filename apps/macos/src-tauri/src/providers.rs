use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProviderInput {
  pub provider_id: String,
  pub display_name: String,
  pub enabled: Option<bool>,
  pub auth_type: String,
  pub secret_ref_id: Option<String>,
  pub base_url: Option<String>,
  pub api_format: String,
  pub region: Option<String>,
  pub cost_policy_json: Option<Value>,
  pub rate_limit_policy_json: Option<Value>,
  pub health_check_json: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProviderView {
  pub id: String,
  pub provider_id: String,
  pub display_name: String,
  pub enabled: bool,
  pub auth_type: String,
  pub secret_ref_id: Option<String>,
  pub base_url: Option<String>,
  pub api_format: String,
  pub region: Option<String>,
  pub default_model_id: Option<String>,
  pub cost_policy_json: Value,
  pub rate_limit_policy_json: Value,
  pub health_check_json: Value,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProfileInput {
  pub provider_id: String,
  pub model_id: String,
  pub display_name: String,
  pub capabilities_json: Option<Value>,
  pub context_window: Option<i64>,
  pub supports_structured_output: Option<bool>,
  pub supports_streaming: Option<bool>,
  pub supports_tools: Option<bool>,
  pub supports_vision: Option<bool>,
  pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelProfileView {
  pub id: String,
  pub provider_id: String,
  pub model_id: String,
  pub display_name: String,
  pub capabilities_json: Value,
  pub context_window: Option<i64>,
  pub supports_structured_output: bool,
  pub supports_streaming: bool,
  pub supports_tools: bool,
  pub supports_vision: bool,
  pub enabled: bool,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderTestResult {
  pub provider_id: String,
  pub success: bool,
  pub message: String,
}

pub fn create_model_provider(
  root_path: impl AsRef<Path>,
  input: ModelProviderInput,
) -> AppResult<ModelProviderView> {
  let connection = open_workspace_connection(root_path)?;
  let input = normalize_provider_input(input)?;
  validate_model_provider_secret(
    &connection,
    &input.provider_id,
    input.secret_ref_id.as_deref(),
  )?;
  let now = Utc::now().to_rfc3339();
  let id = Uuid::new_v4().to_string();

  connection
    .execute(
      "INSERT INTO model_provider (
        id, provider_id, display_name, enabled, auth_type, secret_ref_id, base_url,
        api_format, region, default_model_id, cost_policy_json, rate_limit_policy_json,
        health_check_json, created_at, updated_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12, ?13, ?14)",
      params![
        id,
        input.provider_id,
        input.display_name,
        bool_to_i64(input.enabled.unwrap_or(true)),
        input.auth_type,
        input.secret_ref_id,
        input.base_url,
        input.api_format,
        input.region,
        json_to_string(input.cost_policy_json),
        json_to_string(input.rate_limit_policy_json),
        json_to_string(input.health_check_json),
        now,
        now
      ],
    )
    .map_err(database_error)?;

  write_provider_audit_log(
    &connection,
    "create_model_provider",
    Some(&input.provider_id),
    serde_json::json!({ "provider_id": input.provider_id }),
  )?;

  get_model_provider(&connection, &input.provider_id)
}

pub fn update_model_provider(
  root_path: impl AsRef<Path>,
  provider_id: &str,
  input: ModelProviderInput,
) -> AppResult<ModelProviderView> {
  let connection = open_workspace_connection(root_path)?;
  let provider_id = normalize_required("provider_id", provider_id, AppErrorStage::Provider)?;
  let input = normalize_provider_input(input)?;
  validate_model_provider_secret(
    &connection,
    &input.provider_id,
    input.secret_ref_id.as_deref(),
  )?;
  let now = Utc::now().to_rfc3339();

  connection
    .execute(
      "UPDATE model_provider
       SET provider_id = ?1, display_name = ?2, enabled = ?3, auth_type = ?4,
           secret_ref_id = ?5, base_url = ?6, api_format = ?7, region = ?8,
           cost_policy_json = ?9, rate_limit_policy_json = ?10, health_check_json = ?11,
           updated_at = ?12
       WHERE provider_id = ?13",
      params![
        input.provider_id,
        input.display_name,
        bool_to_i64(input.enabled.unwrap_or(true)),
        input.auth_type,
        input.secret_ref_id,
        input.base_url,
        input.api_format,
        input.region,
        json_to_string(input.cost_policy_json),
        json_to_string(input.rate_limit_policy_json),
        json_to_string(input.health_check_json),
        now,
        provider_id
      ],
    )
    .map_err(database_error)?;

  write_provider_audit_log(
    &connection,
    "update_model_provider",
    Some(&input.provider_id),
    serde_json::json!({ "previous_provider_id": provider_id }),
  )?;

  get_model_provider(&connection, &input.provider_id)
}

pub fn delete_model_provider(root_path: impl AsRef<Path>, provider_id: &str) -> AppResult<bool> {
  let connection = open_workspace_connection(root_path)?;
  let provider_id = normalize_required("provider_id", provider_id, AppErrorStage::Provider)?;

  let deleted = connection
    .execute(
      "DELETE FROM model_provider WHERE provider_id = ?1",
      params![provider_id],
    )
    .map_err(database_error)?;
  if deleted == 0 {
    return Ok(false);
  }

  write_provider_audit_log(
    &connection,
    "delete_model_provider",
    Some(&provider_id),
    serde_json::json!({ "provider_id": provider_id }),
  )?;

  Ok(true)
}

pub fn list_model_providers(
  root_path: impl AsRef<Path>,
  enabled: Option<bool>,
) -> AppResult<Vec<ModelProviderView>> {
  let connection = open_workspace_connection(root_path)?;

  if let Some(enabled) = enabled {
    let mut statement = connection
      .prepare(
        "SELECT id, provider_id, display_name, enabled, auth_type, secret_ref_id, base_url,
                api_format, region, default_model_id, cost_policy_json, rate_limit_policy_json,
                health_check_json, created_at, updated_at
         FROM model_provider
         WHERE enabled = ?1
         ORDER BY display_name",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map(params![bool_to_i64(enabled)], map_model_provider)
      .map_err(database_error)?;
    collect_rows(rows)
  } else {
    let mut statement = connection
      .prepare(
        "SELECT id, provider_id, display_name, enabled, auth_type, secret_ref_id, base_url,
                api_format, region, default_model_id, cost_policy_json, rate_limit_policy_json,
                health_check_json, created_at, updated_at
         FROM model_provider
         ORDER BY display_name",
      )
      .map_err(database_error)?;
    let rows = statement
      .query_map([], map_model_provider)
      .map_err(database_error)?;
    collect_rows(rows)
  }
}

pub fn upsert_model_profile(
  root_path: impl AsRef<Path>,
  input: ModelProfileInput,
) -> AppResult<ModelProfileView> {
  let connection = open_workspace_connection(root_path)?;
  let input = normalize_model_profile_input(input)?;
  let now = Utc::now().to_rfc3339();
  let existing_id = connection
    .query_row(
      "SELECT id FROM model_profile WHERE provider_id = ?1 AND model_id = ?2",
      params![input.provider_id, input.model_id],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?;
  let id = existing_id.unwrap_or_else(|| Uuid::new_v4().to_string());

  connection
    .execute(
      "INSERT INTO model_profile (
        id, provider_id, model_id, display_name, capabilities_json, context_window,
        supports_structured_output, supports_streaming, supports_tools, supports_vision,
        enabled, created_at, updated_at
      ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
      ON CONFLICT(provider_id, model_id) DO UPDATE SET
        display_name = excluded.display_name,
        capabilities_json = excluded.capabilities_json,
        context_window = excluded.context_window,
        supports_structured_output = excluded.supports_structured_output,
        supports_streaming = excluded.supports_streaming,
        supports_tools = excluded.supports_tools,
        supports_vision = excluded.supports_vision,
        enabled = excluded.enabled,
        updated_at = excluded.updated_at",
      params![
        id,
        input.provider_id,
        input.model_id,
        input.display_name,
        json_to_string(input.capabilities_json),
        input.context_window,
        bool_to_i64(input.supports_structured_output.unwrap_or(false)),
        bool_to_i64(input.supports_streaming.unwrap_or(false)),
        bool_to_i64(input.supports_tools.unwrap_or(false)),
        bool_to_i64(input.supports_vision.unwrap_or(false)),
        bool_to_i64(input.enabled.unwrap_or(true)),
        now,
        now
      ],
    )
    .map_err(database_error)?;

  write_provider_audit_log(
    &connection,
    "upsert_model_profile",
    Some(&input.provider_id),
    serde_json::json!({ "model_id": input.model_id }),
  )?;

  get_model_profile(&connection, &input.provider_id, &input.model_id)
}

pub fn list_model_profiles(
  root_path: impl AsRef<Path>,
  provider_id: &str,
) -> AppResult<Vec<ModelProfileView>> {
  let connection = open_workspace_connection(root_path)?;
  let provider_id = normalize_required("provider_id", provider_id, AppErrorStage::Provider)?;
  let mut statement = connection
    .prepare(
      "SELECT id, provider_id, model_id, display_name, capabilities_json, context_window,
              supports_structured_output, supports_streaming, supports_tools, supports_vision,
              enabled, created_at, updated_at
       FROM model_profile
       WHERE provider_id = ?1
       ORDER BY display_name",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![provider_id], map_model_profile)
    .map_err(database_error)?;
  collect_rows(rows)
}

pub fn set_default_model(
  root_path: impl AsRef<Path>,
  provider_id: &str,
  model_id: &str,
) -> AppResult<bool> {
  let connection = open_workspace_connection(root_path)?;
  let provider_id = normalize_required("provider_id", provider_id, AppErrorStage::Provider)?;
  let model_id = normalize_required("model_id", model_id, AppErrorStage::Provider)?;
  get_model_profile(&connection, &provider_id, &model_id)?;

  connection
    .execute(
      "UPDATE model_provider
       SET default_model_id = ?1, updated_at = ?2
       WHERE provider_id = ?3",
      params![model_id, Utc::now().to_rfc3339(), provider_id],
    )
    .map_err(database_error)?;

  Ok(true)
}

pub fn set_active_model_provider(
  root_path: impl AsRef<Path>,
  provider_id: &str,
) -> AppResult<bool> {
  let mut connection = open_workspace_connection(root_path)?;
  let provider_id = normalize_required("provider_id", provider_id, AppErrorStage::Provider)?;
  let transaction = connection
    .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
    .map_err(database_error)?;
  let exists = transaction
    .query_row(
      "SELECT 1 FROM model_provider WHERE provider_id = ?1",
      params![provider_id],
      |_| Ok(()),
    )
    .optional()
    .map_err(database_error)?
    .is_some();
  if !exists {
    return Err(provider_error("要激活的模型供应商不存在"));
  }

  let now = Utc::now().to_rfc3339();
  transaction
    .execute(
      "UPDATE model_provider SET enabled = 0, updated_at = ?1",
      params![now],
    )
    .map_err(database_error)?;
  transaction
    .execute(
      "UPDATE model_provider SET enabled = 1, updated_at = ?1 WHERE provider_id = ?2",
      params![now, provider_id],
    )
    .map_err(database_error)?;
  write_provider_audit_log(
    &transaction,
    "set_active_model_provider",
    Some(&provider_id),
    serde_json::json!({ "provider_id": provider_id }),
  )?;
  transaction.commit().map_err(database_error)?;
  Ok(true)
}

pub fn test_model_provider(
  root_path: impl AsRef<Path>,
  provider_id: &str,
  model_id: Option<String>,
) -> AppResult<ProviderTestResult> {
  let connection = open_workspace_connection(root_path)?;
  let provider_id = normalize_required("provider_id", provider_id, AppErrorStage::Provider)?;
  let provider = get_model_provider(&connection, &provider_id)?;

  if provider.auth_type != "none" && provider.secret_ref_id.is_none() {
    return Ok(ProviderTestResult {
      provider_id,
      success: false,
      message: "供应商需要密钥引用后才能测试连接".to_string(),
    });
  }
  if let Some(secret_ref_id) = provider.secret_ref_id.as_deref() {
    validate_model_provider_secret(&connection, &provider.provider_id, Some(secret_ref_id))?;
  }

  if let Some(model_id) = model_id {
    get_model_profile(&connection, &provider.provider_id, &model_id)?;
  }

  Ok(ProviderTestResult {
    provider_id: provider.provider_id,
    success: true,
    message: "供应商配置完整，真实网络连接测试将在模型适配层执行".to_string(),
  })
}

fn open_workspace_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
}

fn normalize_provider_input(input: ModelProviderInput) -> AppResult<ModelProviderInput> {
  let provider_id = normalize_required("provider_id", &input.provider_id, AppErrorStage::Provider)?;
  let display_name =
    normalize_required("display_name", &input.display_name, AppErrorStage::Provider)?;
  let auth_type = normalize_auth_type(&input.auth_type)?;
  let api_format = normalize_api_format(&input.api_format)?;
  let base_url = normalize_optional(input.base_url);
  let region = normalize_optional(input.region);
  let secret_ref_id = normalize_optional(input.secret_ref_id);

  if api_format == "openai_compatible" && base_url.is_none() {
    return Err(AppError::validation(
      "自定义 OpenAI-compatible endpoint 必须配置 Base URL",
      AppErrorStage::Provider,
    ));
  }

  Ok(ModelProviderInput {
    provider_id,
    display_name,
    enabled: input.enabled,
    auth_type,
    secret_ref_id,
    base_url,
    api_format,
    region,
    cost_policy_json: input.cost_policy_json,
    rate_limit_policy_json: input.rate_limit_policy_json,
    health_check_json: input.health_check_json,
  })
}

fn validate_model_provider_secret(
  connection: &Connection,
  provider_id: &str,
  secret_ref_id: Option<&str>,
) -> AppResult<()> {
  let Some(secret_ref_id) = secret_ref_id else {
    return Ok(());
  };
  let metadata = connection
    .query_row(
      "SELECT provider_type, provider_id FROM secret_ref WHERE id = ?1",
      params![secret_ref_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| provider_error("模型供应商密钥引用不存在"))?;
  if metadata.0 != "model_provider" || metadata.1 != provider_id {
    return Err(provider_error(
      "模型供应商只能绑定同类型且同 provider_id 的密钥",
    ));
  }
  Ok(())
}

fn normalize_model_profile_input(input: ModelProfileInput) -> AppResult<ModelProfileInput> {
  Ok(ModelProfileInput {
    provider_id: normalize_required("provider_id", &input.provider_id, AppErrorStage::Provider)?,
    model_id: normalize_required("model_id", &input.model_id, AppErrorStage::Provider)?,
    display_name: normalize_required("display_name", &input.display_name, AppErrorStage::Provider)?,
    capabilities_json: input.capabilities_json,
    context_window: input.context_window,
    supports_structured_output: input.supports_structured_output,
    supports_streaming: input.supports_streaming,
    supports_tools: input.supports_tools,
    supports_vision: input.supports_vision,
    enabled: input.enabled,
  })
}

fn normalize_required(field: &str, value: &str, stage: AppErrorStage) -> AppResult<String> {
  let value = value.trim();

  if value.is_empty() {
    return Err(AppError::validation(format!("{field} 不能为空"), stage));
  }

  Ok(value.to_string())
}

fn normalize_optional(value: Option<String>) -> Option<String> {
  value.and_then(|value| {
    let value = value.trim().to_string();
    if value.is_empty() {
      None
    } else {
      Some(value)
    }
  })
}

fn normalize_auth_type(auth_type: &str) -> AppResult<String> {
  match auth_type.trim() {
    "api_key" | "none" => Ok(auth_type.trim().to_string()),
    _ => Err(AppError::validation(
      "auth_type 只支持 api_key 或 none",
      AppErrorStage::Provider,
    )),
  }
}

fn normalize_api_format(api_format: &str) -> AppResult<String> {
  match api_format.trim() {
    "openai_compatible" | "anthropic_messages" | "gemini" | "ollama" => {
      Ok(api_format.trim().to_string())
    }
    _ => Err(AppError::validation(
      "api_format 不受支持",
      AppErrorStage::Provider,
    )),
  }
}

fn get_model_provider(connection: &Connection, provider_id: &str) -> AppResult<ModelProviderView> {
  connection
    .query_row(
      "SELECT id, provider_id, display_name, enabled, auth_type, secret_ref_id, base_url,
              api_format, region, default_model_id, cost_policy_json, rate_limit_policy_json,
              health_check_json, created_at, updated_at
       FROM model_provider
       WHERE provider_id = ?1",
      params![provider_id],
      map_model_provider,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| provider_error("模型供应商不存在"))
}

fn get_model_profile(
  connection: &Connection,
  provider_id: &str,
  model_id: &str,
) -> AppResult<ModelProfileView> {
  connection
    .query_row(
      "SELECT id, provider_id, model_id, display_name, capabilities_json, context_window,
              supports_structured_output, supports_streaming, supports_tools, supports_vision,
              enabled, created_at, updated_at
       FROM model_profile
       WHERE provider_id = ?1 AND model_id = ?2",
      params![provider_id, model_id],
      map_model_profile,
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| provider_error("模型不存在"))
}

fn map_model_provider(row: &Row<'_>) -> rusqlite::Result<ModelProviderView> {
  Ok(ModelProviderView {
    id: row.get(0)?,
    provider_id: row.get(1)?,
    display_name: row.get(2)?,
    enabled: i64_to_bool(row.get(3)?),
    auth_type: row.get(4)?,
    secret_ref_id: row.get(5)?,
    base_url: row.get(6)?,
    api_format: row.get(7)?,
    region: row.get(8)?,
    default_model_id: row.get(9)?,
    cost_policy_json: string_to_json(row.get(10)?)?,
    rate_limit_policy_json: string_to_json(row.get(11)?)?,
    health_check_json: string_to_json(row.get(12)?)?,
    created_at: row.get(13)?,
    updated_at: row.get(14)?,
  })
}

fn map_model_profile(row: &Row<'_>) -> rusqlite::Result<ModelProfileView> {
  Ok(ModelProfileView {
    id: row.get(0)?,
    provider_id: row.get(1)?,
    model_id: row.get(2)?,
    display_name: row.get(3)?,
    capabilities_json: string_to_json(row.get(4)?)?,
    context_window: row.get(5)?,
    supports_structured_output: i64_to_bool(row.get(6)?),
    supports_streaming: i64_to_bool(row.get(7)?),
    supports_tools: i64_to_bool(row.get(8)?),
    supports_vision: i64_to_bool(row.get(9)?),
    enabled: i64_to_bool(row.get(10)?),
    created_at: row.get(11)?,
    updated_at: row.get(12)?,
  })
}

fn collect_rows<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> AppResult<Vec<T>> {
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn json_to_string(value: Option<Value>) -> String {
  value.unwrap_or_else(|| serde_json::json!({})).to_string()
}

fn string_to_json(value: String) -> rusqlite::Result<Value> {
  serde_json::from_str(&value).map_err(|error| {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
  })
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

fn write_provider_audit_log(
  connection: &Connection,
  action: &str,
  entity_id: Option<&str>,
  safe_details: Value,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO audit_log (id, entity_type, entity_id, action, safe_details_json, created_at)
       VALUES (?1, 'model_provider', ?2, ?3, ?4, ?5)",
      params![
        Uuid::new_v4().to_string(),
        entity_id,
        action,
        safe_details.to_string(),
        Utc::now().to_rfc3339()
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn provider_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::Provider,
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

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
