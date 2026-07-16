use std::collections::BTreeSet;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

#[derive(Debug, Clone)]
pub(super) struct TargetStepInput {
  pub task_run_id: String,
  pub step_key: String,
  pub platform: String,
  pub data_type: String,
  pub params: Value,
  pub output_selected: bool,
  pub depends_on_step_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PipelineTarget {
  pub id: String,
  pub target_key: String,
  pub params: Value,
  pub cursor: Option<Value>,
  pub status: String,
  pub request_count: i64,
}

pub(super) fn materialize_targets(
  connection: &Connection,
  input: &TargetStepInput,
) -> AppResult<Vec<PipelineTarget>> {
  let target_field = target_field(&input.data_type)?;
  let target_values = if input.depends_on_step_key.is_some() {
    dependency_values(connection, input, target_field)?
  } else {
    BTreeSet::from([required_param(&input.params, target_field)?])
  };
  let transaction = connection.unchecked_transaction().map_err(database_error)?;
  let now = Utc::now().to_rfc3339();
  for target_value in target_values {
    let mut resolved_params = input
      .params
      .as_object()
      .cloned()
      .ok_or_else(|| validation_error("采集步骤 params 必须是对象"))?;
    resolved_params.insert(
      target_field.to_string(),
      Value::String(target_value.clone()),
    );
    transaction
      .execute(
        "INSERT INTO collection_pipeline_target (
           id, task_run_id, step_key, data_type, target_key, resolved_params_json,
           status, request_count, output_selected, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', 0, ?7, ?8, ?8)
         ON CONFLICT(task_run_id, step_key, target_key) DO UPDATE SET
           resolved_params_json = excluded.resolved_params_json,
           output_selected = excluded.output_selected,
           updated_at = excluded.updated_at",
        params![
          Uuid::new_v4().to_string(),
          input.task_run_id,
          input.step_key,
          input.data_type,
          target_value,
          Value::Object(resolved_params).to_string(),
          i64::from(input.output_selected),
          now
        ],
      )
      .map_err(database_error)?;
  }
  transaction.commit().map_err(database_error)?;
  let targets = load_targets(connection, &input.task_run_id, &input.step_key)?;
  validate_materialized_targets(&targets)?;
  Ok(targets)
}

pub(super) fn load_targets(
  connection: &Connection,
  task_run_id: &str,
  step_key: &str,
) -> AppResult<Vec<PipelineTarget>> {
  let mut statement = connection
    .prepare(
      "SELECT id, target_key, resolved_params_json, cursor_json, status, request_count
       FROM collection_pipeline_target
       WHERE task_run_id = ?1 AND step_key = ?2
       ORDER BY created_at, target_key",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![task_run_id, step_key], |row| {
      let params_json: String = row.get(2)?;
      let cursor_json: Option<String> = row.get(3)?;
      Ok(PipelineTarget {
        id: row.get(0)?,
        target_key: row.get(1)?,
        params: serde_json::from_str(&params_json).map_err(|error| {
          rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
        })?,
        cursor: cursor_json
          .map(|value| serde_json::from_str(&value))
          .transpose()
          .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
              3,
              rusqlite::types::Type::Text,
              Box::new(error),
            )
          })?,
        status: row.get(4)?,
        request_count: row.get(5)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

fn dependency_values(
  connection: &Connection,
  input: &TargetStepInput,
  target_field: &str,
) -> AppResult<BTreeSet<String>> {
  let value_column = match target_field {
    "item_id" => "raw.platform_record_id",
    "account_id" => "normalized.author_id",
    _ => return Err(validation_error("关键词入口不支持当前目标绑定")),
  };
  let sql = format!(
    "SELECT {value_column}
     FROM raw_record AS raw
     LEFT JOIN normalized_record AS normalized ON normalized.raw_record_id = raw.id
     WHERE raw.task_run_id = ?1 AND raw.platform = ?2
       AND raw.data_type = 'keyword_search' AND {value_column} IS NOT NULL
     ORDER BY raw.collected_at, raw.id"
  );
  let mut statement = connection.prepare(&sql).map_err(database_error)?;
  let values = statement
    .query_map(params![input.task_run_id, input.platform], |row| {
      row.get::<_, String>(0)
    })
    .map_err(database_error)?
    .collect::<rusqlite::Result<BTreeSet<_>>>()
    .map_err(database_error)?;
  Ok(
    values
      .into_iter()
      .map(|value| value.trim().to_string())
      .filter(|value| !value.is_empty())
      .collect(),
  )
}

fn validate_materialized_targets(targets: &[PipelineTarget]) -> AppResult<()> {
  for target in targets {
    if target.id.trim().is_empty()
      || target.target_key.trim().is_empty()
      || !target.params.is_object()
      || !matches!(
        target.status.as_str(),
        "pending" | "running" | "success" | "failed" | "exhausted" | "budget_stopped"
      )
      || target.request_count < 0
      || target
        .cursor
        .as_ref()
        .is_some_and(|cursor| cursor.is_null())
    {
      return Err(validation_error("采集流水线目标记录已损坏"));
    }
  }
  Ok(())
}

fn target_field(data_type: &str) -> AppResult<&'static str> {
  match data_type {
    "keyword_search" => Ok("keyword"),
    "item_detail" | "comments" => Ok("item_id"),
    "account_profile" | "account_posts" => Ok("account_id"),
    _ => Err(validation_error("采集目标数据类型不受支持")),
  }
}

fn required_param(params: &Value, field: &str) -> AppResult<String> {
  params
    .get(field)
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|value| !value.is_empty() && !value.starts_with("$steps."))
    .map(ToString::to_string)
    .ok_or_else(|| validation_error(format!("采集目标缺少有效 {field}")))
}

fn validation_error(message: impl Into<String>) -> AppError {
  AppError::validation(message, AppErrorStage::Collection)
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
  use rusqlite::{params, Connection};
  use serde_json::json;

  use super::*;

  #[test]
  fn dependency_targets_resolve_item_and_account_ids_from_search_records() {
    let connection = target_connection();
    for input in [
      TargetStepInput {
        task_run_id: "run-1".to_string(),
        step_key: "comments".to_string(),
        platform: "tiktok".to_string(),
        data_type: "comments".to_string(),
        params: json!({ "item_id": "$steps.keyword_search.items[].item_id" }),
        output_selected: true,
        depends_on_step_key: Some("keyword_search".to_string()),
      },
      TargetStepInput {
        task_run_id: "run-1".to_string(),
        step_key: "account_profile".to_string(),
        platform: "tiktok".to_string(),
        data_type: "account_profile".to_string(),
        params: json!({ "account_id": "$steps.keyword_search.items[].account_id" }),
        output_selected: true,
        depends_on_step_key: Some("keyword_search".to_string()),
      },
    ] {
      materialize_targets(&connection, &input).expect("依赖目标应物化");
    }

    let targets = connection
      .prepare(
        "SELECT step_key, target_key, resolved_params_json
         FROM collection_pipeline_target ORDER BY step_key, target_key",
      )
      .expect("目标查询应准备")
      .query_map([], |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
        ))
      })
      .expect("目标应查询")
      .collect::<Result<Vec<_>, _>>()
      .expect("目标应解析");
    assert_eq!(targets.len(), 4);
    assert_eq!(targets[0].0, "account_profile");
    assert!(targets[0].2.contains("account-a") || targets[1].2.contains("account-a"));
    assert!(targets[2].2.contains("video-a") || targets[3].2.contains("video-a"));
  }

  fn target_connection() -> Connection {
    let connection = Connection::open_in_memory().expect("内存数据库应创建");
    connection
      .execute_batch(
        "CREATE TABLE raw_record (
           id TEXT PRIMARY KEY, task_run_id TEXT, platform TEXT,
           data_type TEXT, platform_record_id TEXT,
           collected_at TEXT DEFAULT '2026-07-16T08:00:00+00:00'
         );
         CREATE TABLE normalized_record (raw_record_id TEXT, author_id TEXT);
         CREATE TABLE collection_pipeline_target (
           id TEXT PRIMARY KEY, task_run_id TEXT NOT NULL, step_key TEXT NOT NULL,
           data_type TEXT NOT NULL, target_key TEXT NOT NULL,
           resolved_params_json TEXT NOT NULL, cursor_json TEXT,
           status TEXT NOT NULL DEFAULT 'pending', request_count INTEGER NOT NULL DEFAULT 0,
           output_selected INTEGER NOT NULL, failure_json TEXT,
           created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
           UNIQUE (task_run_id, step_key, target_key)
         );",
      )
      .expect("目标测试表应创建");
    for (raw_id, item_id, author_id) in [
      ("raw-a", "video-a", "account-a"),
      ("raw-b", "video-b", "account-b"),
    ] {
      connection
        .execute(
          "INSERT INTO raw_record (
             id, task_run_id, platform, data_type, platform_record_id
           ) VALUES (?1, 'run-1', 'tiktok', 'keyword_search', ?2)",
          params![raw_id, item_id],
        )
        .expect("搜索原始记录应插入");
      connection
        .execute(
          "INSERT INTO normalized_record VALUES (?1, ?2)",
          params![raw_id, author_id],
        )
        .expect("搜索归一化记录应插入");
    }
    connection
  }
}
