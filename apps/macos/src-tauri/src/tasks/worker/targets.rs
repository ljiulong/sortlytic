use std::collections::BTreeSet;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};

use super::mutations::with_fenced_write;
use super::WorkerFence;

#[derive(Debug, Clone)]
pub(super) struct TargetStepInput {
  pub task_run_id: String,
  pub step_key: String,
  pub platform: String,
  pub data_type: String,
  pub params: Value,
  pub target_limit: i64,
  pub output_selected: bool,
  pub depends_on_step_key: Option<String>,
  pub input_binding: Option<String>,
  pub dependency_data_type: Option<String>,
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
  fence: Option<&WorkerFence>,
) -> AppResult<Vec<PipelineTarget>> {
  with_fenced_write(connection, fence, |connection| {
    if fence.is_some() {
      validate_running_scope(connection, &input.task_run_id)?;
    }
    let target_field = target_field(&input.data_type)?;
    let target_values = if input.depends_on_step_key.is_some() {
      dependency_values(connection, input, target_field)?
    } else {
      BTreeSet::from([required_param(&input.params, target_field)?])
    };
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
      connection
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
    let targets = load_targets(connection, &input.task_run_id, &input.step_key)?;
    validate_materialized_targets(&targets)?;
    Ok(targets)
  })
}

fn validate_running_scope(connection: &Connection, task_run_id: &str) -> AppResult<()> {
  let running = connection
    .query_row(
      "SELECT task.status = 'running' AND run.status = 'running'
       FROM task_run AS run
       JOIN collection_task AS task ON task.id = run.task_id
       WHERE run.id = ?1 AND run.task_id = task.id",
      params![task_run_id],
      |row| row.get::<_, bool>(0),
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| validation_error("任务运行记录不存在或不属于任何任务"))?;
  if !running {
    return Err(validation_error("只允许为运行中的任务物化采集目标"));
  }
  Ok(())
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
  let target_limit = usize::try_from(input.target_limit)
    .ok()
    .filter(|limit| *limit > 0)
    .ok_or_else(|| validation_error("依赖步骤账号目标上限必须大于 0"))?;
  let binding = input.input_binding.as_deref().unwrap_or(target_field);
  let value_column = match binding {
    "item_id" => "raw.platform_record_id",
    "account_id" | "platform_user_id" => {
      "COALESCE(json_extract(normalized.account_fields_json, '$.platform_user_id'), normalized.author_id)"
    }
    "account_handle" => "json_extract(normalized.account_fields_json, '$.account_handle')",
    "secure_user_id" => "json_extract(normalized.account_fields_json, '$.secure_user_id')",
    _ => return Err(validation_error("依赖步骤使用了不受支持的账号目标绑定")),
  };
  let dependency_data_type = input
    .dependency_data_type
    .as_deref()
    .ok_or_else(|| validation_error("依赖步骤缺少来源数据类型"))?;
  let sql = format!(
    "SELECT {value_column}
     FROM raw_record AS raw
     LEFT JOIN normalized_record AS normalized ON normalized.raw_record_id = raw.id
     WHERE raw.task_run_id = ?1 AND raw.platform = ?2
       AND raw.data_type = ?3 AND {value_column} IS NOT NULL
     ORDER BY raw.collected_at, raw.id"
  );
  let mut statement = connection.prepare(&sql).map_err(database_error)?;
  let values = statement
    .query_map(
      params![input.task_run_id, input.platform, dependency_data_type],
      |row| row.get::<_, String>(0),
    )
    .map_err(database_error)?
    .collect::<rusqlite::Result<BTreeSet<_>>>()
    .map_err(database_error)?;
  Ok(
    values
      .into_iter()
      .map(|value| value.trim().to_string())
      .filter(|value| !value.is_empty())
      .take(target_limit)
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
    "keyword_search" | "user_search" => Ok("keyword"),
    "item_detail" | "comments" => Ok("item_id"),
    "account_profile"
    | "account_posts"
    | "followers"
    | "followings"
    | "similar_accounts"
    | "extended_demographics"
    | "account_country" => Ok("account_id"),
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
        target_limit: 10,
        output_selected: true,
        depends_on_step_key: Some("keyword_search".to_string()),
        input_binding: None,
        dependency_data_type: Some("keyword_search".to_string()),
      },
      TargetStepInput {
        task_run_id: "run-1".to_string(),
        step_key: "account_profile".to_string(),
        platform: "tiktok".to_string(),
        data_type: "account_profile".to_string(),
        params: json!({ "account_id": "$steps.keyword_search.items[].account_id" }),
        target_limit: 10,
        output_selected: true,
        depends_on_step_key: Some("keyword_search".to_string()),
        input_binding: None,
        dependency_data_type: Some("keyword_search".to_string()),
      },
    ] {
      materialize_targets(&connection, &input, None).expect("依赖目标应物化");
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

  #[test]
  fn version_four_enrichment_uses_declared_account_binding_from_discovery_records() {
    let connection = target_connection();
    connection
      .execute(
        "INSERT INTO raw_record (
           id, task_run_id, platform, data_type, platform_record_id
         ) VALUES ('raw-v4', 'run-v4', 'tiktok', 'user_search', 'search-result-1')",
        [],
      )
      .expect("v4 发现原始记录应插入");
    connection
      .execute(
        "INSERT INTO normalized_record (
           raw_record_id, author_id, account_fields_json
         ) VALUES (?1, ?2, ?3)",
        params![
          "raw-v4",
          "platform-user-1",
          json!({
            "platform_user_id": "platform-user-1",
            "account_handle": "account-handle-1",
            "secure_user_id": "secure-user-1"
          })
          .to_string()
        ],
      )
      .expect("v4 发现归一化记录应插入");

    let targets = materialize_targets(
      &connection,
      &TargetStepInput {
        task_run_id: "run-v4".to_string(),
        step_key: "enrich_country".to_string(),
        platform: "tiktok".to_string(),
        data_type: "account_country".to_string(),
        params: json!({ "account_id": "$steps.discover.accounts[].account_handle" }),
        target_limit: 10,
        output_selected: true,
        depends_on_step_key: Some("discover".to_string()),
        input_binding: Some("account_handle".to_string()),
        dependency_data_type: Some("user_search".to_string()),
      },
      None,
    )
    .expect("v4 账号名补全目标应物化");

    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].target_key, "account-handle-1");
    assert_eq!(targets[0].params["account_id"], "account-handle-1");
  }

  #[test]
  fn stale_worker_fence_rejects_target_materialization() {
    let connection = target_connection();
    connection
      .execute(
        "INSERT INTO task_worker_lease (
           id, owner_id, lease_expires_at, created_at, updated_at, generation
         ) VALUES (
           'task_worker', 'replacement-owner', 9223372036854775807, 'now', 'now', 2
         )",
        [],
      )
      .expect("replacement lease should be installed");
    let stale =
      WorkerFence::new("stale-owner".to_string(), 1).expect("stale fence should be valid");

    materialize_targets(
      &connection,
      &TargetStepInput {
        task_run_id: "run-stale".to_string(),
        step_key: "item_detail".to_string(),
        platform: "tiktok".to_string(),
        data_type: "item_detail".to_string(),
        params: json!({ "item_id": "stale-video" }),
        target_limit: 1,
        output_selected: true,
        depends_on_step_key: None,
        input_binding: None,
        dependency_data_type: None,
      },
      Some(&stale),
    )
    .expect_err("a stale generation must not materialize pipeline targets");

    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM collection_pipeline_target WHERE task_run_id = 'run-stale'",
          [],
          |row| row.get::<_, i64>(0),
        )
        .unwrap(),
      0
    );
  }

  #[test]
  fn cancelled_run_rejects_target_materialization_with_a_current_fence() {
    let connection = target_connection();
    connection
      .execute_batch(
        "CREATE TABLE collection_task (
           id TEXT PRIMARY KEY,
           status TEXT NOT NULL
         );
         CREATE TABLE task_run (
           id TEXT PRIMARY KEY,
           task_id TEXT NOT NULL,
           status TEXT NOT NULL
         );
         INSERT INTO collection_task (id, status)
           VALUES ('task-cancelled-target', 'cancelled');
         INSERT INTO task_run (id, task_id, status)
           VALUES ('run-cancelled-target', 'task-cancelled-target', 'cancelled');
         INSERT INTO task_worker_lease (
           id, owner_id, lease_expires_at, created_at, updated_at, generation
         ) VALUES (
           'task_worker', 'current-owner', 9223372036854775807, 'now', 'now', 1
         );",
      )
      .expect("cancelled target scope should install");
    let current =
      WorkerFence::new("current-owner".to_string(), 1).expect("current fence should be valid");

    materialize_targets(
      &connection,
      &TargetStepInput {
        task_run_id: "run-cancelled-target".to_string(),
        step_key: "item_detail".to_string(),
        platform: "tiktok".to_string(),
        data_type: "item_detail".to_string(),
        params: json!({ "item_id": "cancelled-video" }),
        target_limit: 1,
        output_selected: true,
        depends_on_step_key: None,
        input_binding: None,
        dependency_data_type: None,
      },
      Some(&current),
    )
    .expect_err("a cancelled run must not materialize pipeline targets");

    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM collection_pipeline_target
           WHERE task_run_id = 'run-cancelled-target'",
          [],
          |row| row.get::<_, i64>(0),
        )
        .expect("target count should query"),
      0
    );
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
         CREATE TABLE normalized_record (
           raw_record_id TEXT, author_id TEXT, account_fields_json TEXT NOT NULL DEFAULT '{}'
         );
         CREATE TABLE collection_pipeline_target (
           id TEXT PRIMARY KEY, task_run_id TEXT NOT NULL, step_key TEXT NOT NULL,
           data_type TEXT NOT NULL, target_key TEXT NOT NULL,
           resolved_params_json TEXT NOT NULL, cursor_json TEXT,
           status TEXT NOT NULL DEFAULT 'pending', request_count INTEGER NOT NULL DEFAULT 0,
           output_selected INTEGER NOT NULL, failure_json TEXT,
           created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
           UNIQUE (task_run_id, step_key, target_key)
         );
         CREATE TABLE task_worker_lease (
           id TEXT PRIMARY KEY,
           owner_id TEXT NOT NULL,
           lease_expires_at INTEGER NOT NULL,
           created_at TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           generation INTEGER NOT NULL
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
          "INSERT INTO normalized_record (raw_record_id, author_id) VALUES (?1, ?2)",
          params![raw_id, author_id],
        )
        .expect("搜索归一化记录应插入");
    }
    connection
  }
}
