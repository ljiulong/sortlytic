use std::path::Path;

use chrono::Utc;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NaturalParseAttemptView {
  pub id: String,
  pub task_id: String,
  pub intent_text: String,
  pub language: Option<String>,
  pub parse_status: String,
  pub parse_phase: Option<String>,
  pub ai_run_id: Option<String>,
  pub error_code: Option<String>,
  pub error_message: Option<String>,
  pub retryable: Option<bool>,
  pub error_safe_details_json: Value,
  pub provider_id: Option<String>,
  pub model_id: Option<String>,
  pub prompt_version_id: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

pub fn list_latest_task_intents(
  root_path: impl AsRef<Path>,
) -> AppResult<Vec<NaturalParseAttemptView>> {
  let connection = open_connection(root_path)?;
  let mut statement = connection
    .prepare(
      "WITH ranked AS (
         SELECT intent.*,
                ROW_NUMBER() OVER (
                  PARTITION BY intent.task_id
                  ORDER BY intent.updated_at DESC, intent.created_at DESC, intent.id DESC
                ) AS attempt_rank
         FROM task_intent AS intent
       )
       SELECT ranked.id, ranked.task_id, ranked.intent_text, ranked.language,
              ranked.parse_status, ranked.parse_phase, ranked.ai_run_id,
              ranked.error_code, ranked.error_message, ranked.retryable,
              ranked.error_safe_details_json, snapshot.provider_id, snapshot.model_id,
              snapshot.prompt_version_id, ranked.created_at, ranked.updated_at
       FROM ranked
       LEFT JOIN ai_run AS run ON run.id = ranked.ai_run_id
       LEFT JOIN runtime_snapshot AS snapshot ON snapshot.id = run.runtime_snapshot_id
       WHERE ranked.attempt_rank = 1
       ORDER BY ranked.updated_at DESC, ranked.created_at DESC, ranked.id DESC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map([], map_attempt)
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

pub fn list_task_intents(
  root_path: impl AsRef<Path>,
  task_id: &str,
) -> AppResult<Vec<NaturalParseAttemptView>> {
  let connection = open_connection(root_path)?;
  let mut statement = connection
    .prepare(
      "SELECT intent.id, intent.task_id, intent.intent_text, intent.language,
              intent.parse_status, intent.parse_phase, intent.ai_run_id,
              intent.error_code, intent.error_message, intent.retryable,
              intent.error_safe_details_json, snapshot.provider_id, snapshot.model_id,
              snapshot.prompt_version_id, intent.created_at, intent.updated_at
       FROM task_intent AS intent
       LEFT JOIN ai_run AS run ON run.id = intent.ai_run_id
       LEFT JOIN runtime_snapshot AS snapshot ON snapshot.id = run.runtime_snapshot_id
       WHERE intent.task_id = ?1
       ORDER BY intent.updated_at DESC, intent.created_at DESC, intent.id DESC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_id], map_attempt)
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

pub(crate) fn mark_interrupted_task_intents(root_path: impl AsRef<Path>) -> AppResult<usize> {
  let connection = open_connection(root_path)?;
  connection
    .execute(
      "UPDATE task_intent
       SET parse_status = 'interrupted',
           error_code = 'AI_PARSE_INTERRUPTED',
           error_message = '上次自然语言解析在应用关闭前未完成，请重新解析',
           retryable = 1,
           updated_at = ?1
       WHERE parse_status = 'running'",
      params![Utc::now().to_rfc3339()],
    )
    .map_err(database_error)
}

fn map_attempt(row: &Row<'_>) -> rusqlite::Result<NaturalParseAttemptView> {
  Ok(NaturalParseAttemptView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    intent_text: row.get(2)?,
    language: row.get(3)?,
    parse_status: row.get(4)?,
    parse_phase: row.get(5)?,
    ai_run_id: row.get(6)?,
    error_code: row.get(7)?,
    error_message: row.get(8)?,
    retryable: row.get::<_, Option<i64>>(9)?.map(|value| value != 0),
    error_safe_details_json: row
      .get::<_, String>(10)
      .ok()
      .and_then(|value| serde_json::from_str(&value).ok())
      .unwrap_or_else(|| serde_json::json!({})),
    provider_id: row.get(11)?,
    model_id: row.get(12)?,
    prompt_version_id: row.get(13)?,
    created_at: row.get(14)?,
    updated_at: row.get(15)?,
  })
}

fn open_connection(root_path: impl AsRef<Path>) -> AppResult<Connection> {
  open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))
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
mod tests {
  use std::fs;

  use uuid::Uuid;

  use super::*;
  use crate::tasks::{create_collection_task, CreateCollectionTaskInput};
  use crate::workspace::create_workspace;

  #[test]
  fn lists_only_latest_attempt_and_recovers_running_state_once() {
    let root = std::env::temp_dir().join(format!("latest-intent-{}", Uuid::new_v4()));
    create_workspace("最新解析尝试", &root).unwrap();
    let task = create_collection_task(
      &root,
      CreateCollectionTaskInput {
        name: "自然语言任务".to_string(),
        source_type: "natural_language".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["account".to_string()],
      },
    )
    .unwrap();
    let connection = open_connection(&root).unwrap();
    for (id, text, status, phase, created_at) in [
      (
        "intent-old",
        "旧输入",
        "failed",
        "requesting_ai",
        "2026-07-20T00:00:00Z",
      ),
      (
        "intent-new",
        "新输入",
        "running",
        "requesting_ai",
        "2026-07-20T00:01:00Z",
      ),
    ] {
      connection
        .execute(
          "INSERT INTO task_intent (
          id, task_id, intent_text, language, parse_status, parse_phase,
          error_safe_details_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, 'zh-CN', ?4, ?5, '{}', ?6, ?6)",
          params![id, task.id, text, status, phase, created_at],
        )
        .unwrap();
    }
    drop(connection);

    assert_eq!(mark_interrupted_task_intents(&root).unwrap(), 1);
    assert_eq!(mark_interrupted_task_intents(&root).unwrap(), 0);
    let attempts = list_latest_task_intents(&root).unwrap();

    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].id, "intent-new");
    assert_eq!(attempts[0].intent_text, "新输入");
    assert_eq!(attempts[0].parse_status, "interrupted");
    assert_eq!(attempts[0].parse_phase.as_deref(), Some("requesting_ai"));
    assert_eq!(
      attempts[0].error_code.as_deref(),
      Some("AI_PARSE_INTERRUPTED")
    );
    assert_eq!(attempts[0].retryable, Some(true));

    let history = list_task_intents(&root, &task.id).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].id, "intent-new");
    assert_eq!(history[1].id, "intent-old");
    assert_eq!(history[1].parse_status, "failed");

    fs::remove_dir_all(root).ok();
  }
}
