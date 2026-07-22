use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::Path;

use chrono::{TimeZone, Utc};
use rusqlite::{params, types::Type, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

mod account_fields;
mod storage;

const NORMALIZED_SCHEMA_VERSION: i64 = 2;
const MAX_PAGE_RECORDS: usize = 100;
pub(super) const MAX_RAW_RECORD_BYTES: usize = 16 * 1024 * 1024;
const MAX_RAW_PAGE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistCollectionPageInput {
  pub task_id: String,
  pub task_run_id: String,
  pub platform: String,
  pub data_type: String,
  pub records: Vec<Value>,
  pub collected_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawRecordView {
  pub id: String,
  pub task_id: String,
  pub task_run_id: Option<String>,
  pub platform: String,
  pub data_type: String,
  pub platform_record_id: String,
  pub raw_url: Option<String>,
  pub raw_file_path: String,
  pub raw_hash: String,
  pub summary_json: Value,
  pub collected_at: String,
  pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NormalizedRecordView {
  pub id: String,
  pub raw_record_id: String,
  pub task_id: String,
  pub platform: String,
  pub author_id: Option<String>,
  pub author_name: Option<String>,
  pub content_text: Option<String>,
  pub content_url: Option<String>,
  pub published_at: Option<String>,
  pub region: Option<String>,
  pub metrics_json: Value,
  pub tags_json: Value,
  pub account_fields_json: Value,
  pub field_evidence_json: Value,
  pub normalized_schema_version: i64,
  pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskRecordCountView {
  pub task_id: String,
  pub record_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskResultRecordView {
  pub id: String,
  pub platform: String,
  pub username: Option<String>,
  pub account: Option<String>,
  pub platform_user_id: Option<String>,
  pub profile_text: Option<String>,
  pub country_region: Option<String>,
  pub region_source: Option<String>,
  pub region_confidence: Option<String>,
  pub gender: Option<String>,
  pub age: Option<i64>,
  pub followers_count: Option<i64>,
  pub posts_count: Option<i64>,
  pub last_posted_at: Option<String>,
  pub profile_url: Option<String>,
  pub data_source: String,
  pub collected_at: String,
  pub notes: Option<String>,
  pub account_fields_json: Value,
  pub field_evidence_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskResultsPageView {
  pub task_id: String,
  pub task_run_id: String,
  pub run_status: String,
  pub age_filter_configured: bool,
  pub gender_filter_configured: bool,
  pub selected_fields: Vec<String>,
  pub total_count: i64,
  pub offset: i64,
  pub limit: i64,
  pub items: Vec<TaskResultRecordView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PersistCollectionPageResult {
  pub inserted_count: usize,
  pub existing_count: usize,
  pub raw_records: Vec<RawRecordView>,
  pub normalized_records: Vec<NormalizedRecordView>,
}

pub fn persist_collection_page(
  root_path: impl AsRef<Path>,
  input: PersistCollectionPageInput,
) -> AppResult<PersistCollectionPageResult> {
  let input = normalize_input(input)?;
  let prepared = prepare_records(&input)?;
  storage::persist_prepared_records(root_path.as_ref(), &input, prepared)
}

pub fn list_task_record_counts(root_path: impl AsRef<Path>) -> AppResult<Vec<TaskRecordCountView>> {
  let connection = open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))?;
  let mut statement = connection
    .prepare(
      "SELECT task.id, COUNT(account.id)
       FROM collection_task AS task
       LEFT JOIN task_run AS latest_run ON latest_run.id = (
         SELECT candidate.id
         FROM task_run AS candidate
         WHERE candidate.task_id = task.id
           AND candidate.status IN ('success', 'partial_success', 'failed', 'cancelled')
         ORDER BY candidate.run_sequence DESC
         LIMIT 1
       )
       LEFT JOIN collected_account AS account
         ON account.task_run_id = latest_run.id AND account.output_included = 1
       GROUP BY task.id
       ORDER BY task.created_at DESC, task.id ASC",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map([], |row| {
      Ok(TaskRecordCountView {
        task_id: row.get(0)?,
        record_count: row.get(1)?,
      })
    })
    .map_err(database_error)?;
  rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)
}

pub fn list_task_results(
  root_path: impl AsRef<Path>,
  task_id: &str,
  limit: i64,
  offset: i64,
) -> AppResult<TaskResultsPageView> {
  let task_id = required_text("task_id", task_id, 512)?;
  if !(1..=200).contains(&limit) {
    return Err(validation_error("limit 必须在 1 到 200 之间"));
  }
  if offset < 0 {
    return Err(validation_error("offset 不能小于 0"));
  }

  let connection = open_workspace_database(root_path.as_ref().join(DATABASE_FILE_NAME))?;
  let latest_run = connection
    .query_row(
      "SELECT run.id, run.status,
              CASE WHEN json_type(plan.plan_json, '$.age_range') = 'object' THEN 1 ELSE 0 END,
              CASE WHEN json_type(plan.plan_json, '$.gender_filter') = 'array'
                     AND json_array_length(plan.plan_json, '$.gender_filter') > 0
                   THEN 1 ELSE 0 END,
              task.selected_fields_json
       FROM task_run AS run
       JOIN collection_task AS task ON task.id = run.task_id
       LEFT JOIN collection_plan AS plan ON plan.id = run.plan_id
       WHERE run.task_id = ?1 AND run.status IN ('success', 'partial_success')
       ORDER BY run.run_sequence DESC
       LIMIT 1",
      params![task_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, i64>(2)? != 0,
          row.get::<_, i64>(3)? != 0,
          string_array_column(row, 4)?,
        ))
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| validation_error("任务没有可查看的成功运行"))?;
  let total_count = connection
    .query_row(
      "SELECT COUNT(*) FROM collected_account
       WHERE task_run_id = ?1 AND output_included = 1",
      params![latest_run.0],
      |row| row.get::<_, i64>(0),
    )
    .map_err(database_error)?;
  let mut statement = connection
    .prepare(
      "SELECT id, platform, username, account, platform_user_id, profile_text,
              country_region, region_source, region_confidence, gender, age,
              followers_count, posts_count, last_posted_at, profile_url,
              data_source, collected_at, notes, account_fields_json, field_evidence_json
       FROM collected_account
       WHERE task_run_id = ?1 AND output_included = 1
       ORDER BY created_at, id
       LIMIT ?2 OFFSET ?3",
    )
    .map_err(database_error)?;
  let rows = statement
    .query_map(params![latest_run.0, limit, offset], |row| {
      Ok(TaskResultRecordView {
        id: row.get(0)?,
        platform: row.get(1)?,
        username: row.get(2)?,
        account: row.get(3)?,
        platform_user_id: row.get(4)?,
        profile_text: row.get(5)?,
        country_region: row.get(6)?,
        region_source: row.get(7)?,
        region_confidence: row.get(8)?,
        gender: row.get(9)?,
        age: row.get(10)?,
        followers_count: row.get(11)?,
        posts_count: row.get(12)?,
        last_posted_at: row.get(13)?,
        profile_url: row.get(14)?,
        data_source: row.get(15)?,
        collected_at: row.get(16)?,
        notes: row.get(17)?,
        account_fields_json: json_value_column(row, 18)?,
        field_evidence_json: json_value_column(row, 19)?,
      })
    })
    .map_err(database_error)?;
  let items = rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;

  Ok(TaskResultsPageView {
    task_id,
    task_run_id: latest_run.0,
    run_status: latest_run.1,
    age_filter_configured: latest_run.2,
    gender_filter_configured: latest_run.3,
    selected_fields: latest_run.4,
    total_count,
    offset,
    limit,
    items,
  })
}

fn string_array_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Vec<String>> {
  let text = row.get::<_, String>(index)?;
  serde_json::from_str(&text)
    .map_err(|error| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error)))
}

fn json_value_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Value> {
  let text = row.get::<_, String>(index)?;
  serde_json::from_str(&text)
    .map_err(|error| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error)))
}

#[derive(Debug)]
pub(super) struct NormalizedInput {
  pub(super) task_id: String,
  pub(super) task_run_id: String,
  pub(super) platform: String,
  pub(super) data_type: String,
  pub(super) collected_at: String,
  records: Vec<Value>,
}

#[derive(Debug, Clone)]
pub(super) struct PreparedRecord {
  pub(super) platform_record_id: String,
  pub(super) raw_bytes: Vec<u8>,
  pub(super) raw_hash: String,
  pub(super) identity_hash: String,
  pub(super) normalized: NormalizedFields,
}

#[derive(Debug, Clone)]
pub(super) struct NormalizedFields {
  pub(super) author_id: Option<String>,
  pub(super) author_name: Option<String>,
  pub(super) content_text: Option<String>,
  pub(super) content_url: Option<String>,
  pub(super) published_at: Option<String>,
  pub(super) region: Option<String>,
  pub(super) metrics_json: Value,
  pub(super) tags_json: Value,
  pub(super) account_fields_json: Value,
  pub(super) field_evidence_json: Value,
}

fn normalize_input(input: PersistCollectionPageInput) -> AppResult<NormalizedInput> {
  let task_id = required_text("task_id", &input.task_id, 512)?;
  let task_run_id = required_text("task_run_id", &input.task_run_id, 512)?;
  let platform = required_text("platform", &input.platform, 64)?;
  let data_type = required_text("data_type", &input.data_type, 64)?;
  let supported = crate::collection::list_platform_data_types(&platform).is_ok_and(|items| {
    items
      .iter()
      .any(|capability| capability.data_type == data_type)
  }) || account_fields::is_supported_account_data_type(&platform, &data_type);
  if !supported {
    return Err(validation_error("平台与数据类型组合不受支持"));
  }
  if input.records.len() > MAX_PAGE_RECORDS {
    return Err(validation_error(format!(
      "单页记录数不能超过 {MAX_PAGE_RECORDS}"
    )));
  }
  let collected_at = match input.collected_at {
    Some(value) => chrono::DateTime::parse_from_rfc3339(value.trim())
      .map_err(|_| validation_error("collected_at 必须是 RFC 3339 时间"))?
      .to_rfc3339(),
    None => Utc::now().to_rfc3339(),
  };

  Ok(NormalizedInput {
    task_id,
    task_run_id,
    platform,
    data_type,
    collected_at,
    records: input.records,
  })
}

fn prepare_records(input: &NormalizedInput) -> AppResult<Vec<PreparedRecord>> {
  let mut total_bytes = 0usize;
  let mut unique = BTreeMap::<String, PreparedRecord>::new();

  for raw_value in &input.records {
    let prepared = prepare_record(input, raw_value)?;
    total_bytes = total_bytes
      .checked_add(prepared.raw_bytes.len())
      .ok_or_else(|| validation_error("整页原始记录体积溢出"))?;
    if total_bytes > MAX_RAW_PAGE_BYTES {
      return Err(validation_error("整页原始记录超过 16 MiB 安全上限"));
    }

    if let Some(existing) = unique.get(&prepared.platform_record_id) {
      if existing.raw_hash != prepared.raw_hash {
        return Err(validation_error(format!(
          "同一页包含 ID {} 的冲突记录",
          prepared.platform_record_id
        )));
      }
      continue;
    }
    unique.insert(prepared.platform_record_id.clone(), prepared);
  }

  Ok(unique.into_values().collect())
}

pub(super) fn prepare_record(
  input: &NormalizedInput,
  raw_value: &Value,
) -> AppResult<PreparedRecord> {
  if !raw_value.is_object() {
    return Err(validation_error("采集记录必须是 JSON 对象"));
  }
  let payload = normalization_payload(&input.platform, &input.data_type, raw_value);
  let platform_record_id =
    platform_record_id(&input.platform, &input.data_type, payload, raw_value)?;
  let raw_bytes = serialize_raw_record(raw_value)?;
  let raw_hash = sha256_hex(&raw_bytes);
  let identity_hash = sha256_hex(
    format!(
      "{}\0{}\0{}\0{}\0{}",
      input.task_id, input.task_run_id, input.platform, input.data_type, platform_record_id
    )
    .as_bytes(),
  );

  Ok(PreparedRecord {
    platform_record_id,
    raw_bytes,
    raw_hash,
    identity_hash,
    normalized: normalize_record(input, payload, raw_value),
  })
}

fn serialize_raw_record(value: &Value) -> AppResult<Vec<u8>> {
  let mut writer = LimitedJsonWriter::default();
  match serde_json::to_writer(&mut writer, value) {
    Ok(()) => Ok(writer.bytes),
    Err(_) if writer.exceeded => Err(validation_error("单条原始记录超过 16 MiB 安全上限")),
    Err(error) => Err(json_error(error)),
  }
}

#[derive(Default)]
struct LimitedJsonWriter {
  bytes: Vec<u8>,
  exceeded: bool,
}

impl Write for LimitedJsonWriter {
  fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
    let exceeds_limit = match self.bytes.len().checked_add(buffer.len()) {
      Some(length) => length > MAX_RAW_RECORD_BYTES,
      None => true,
    };
    if exceeds_limit {
      self.exceeded = true;
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "raw record size limit exceeded",
      ));
    }
    self.bytes.extend_from_slice(buffer);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

fn normalization_payload<'a>(platform: &str, data_type: &str, record: &'a Value) -> &'a Value {
  let pointers: &[&str] = match (platform, data_type) {
    ("tiktok" | "douyin", "item_detail") => &["/aweme_detail", "/aweme", "/aweme_info"],
    ("xiaohongshu", "item_detail") => &["/note", "/note_card", "/data/note"],
    (_, "account_profile") => &["/user", "/user_info", "/data/user"],
    ("xiaohongshu", "keyword_search") => &["/note_card"],
    _ => &[],
  };
  pointers
    .iter()
    .find_map(|pointer| {
      record
        .pointer(pointer)
        .filter(|value| value.as_object().is_some_and(|object| !object.is_empty()))
    })
    .unwrap_or(record)
}

fn platform_record_id(
  platform: &str,
  data_type: &str,
  payload: &Value,
  raw: &Value,
) -> AppResult<String> {
  let pointers: &[&str] = match (platform, data_type) {
    (_, "comments") => &["/cid", "/comment_id", "/id"],
    ("xiaohongshu", "account_profile") => &["/user_id", "/red_id", "/id"],
    (
      _,
      "account_profile"
      | "user_search"
      | "followers"
      | "followings"
      | "similar_accounts"
      | "extended_demographics"
      | "account_country",
    ) => &[
      "/uid",
      "/user_id",
      "/userid",
      "/sec_uid",
      "/sec_user_id",
      "/unique_id",
      "/username",
      "/id",
      "/user/uid",
      "/user/user_id",
    ],
    ("xiaohongshu", _) => &["/note_id", "/id"],
    _ => &["/aweme_id", "/id"],
  };
  let id = first_text_from_sources(&[payload, raw], pointers)
    .ok_or_else(|| validation_error("采集记录缺少平台记录 ID"))?;
  if id.chars().count() > 512 {
    return Err(validation_error("平台记录 ID 长度不能超过 512 个字符"));
  }
  Ok(id)
}

fn normalize_record(input: &NormalizedInput, payload: &Value, raw: &Value) -> NormalizedFields {
  let sources = [payload, raw];
  let (account_fields_json, field_evidence_json) =
    account_fields::normalize_account_fields(input, raw);
  NormalizedFields {
    author_id: first_text_from_sources(
      &sources,
      &[
        "/author/uid",
        "/author/user_id",
        "/author/userid",
        "/author/sec_uid",
        "/user/uid",
        "/user/user_id",
        "/user/userid",
        "/user/red_id",
        "/uid",
        "/user_id",
        "/userid",
        "/red_id",
        "/author_id",
      ],
    ),
    author_name: first_text_from_sources(
      &sources,
      &[
        "/author/nickname",
        "/author/name",
        "/user/nickname",
        "/user/name",
        "/nickname",
        "/name",
      ],
    ),
    content_text: first_text_from_sources(
      &sources,
      &[
        "/desc",
        "/text",
        "/content",
        "/title",
        "/display_title",
        "/signature",
      ],
    ),
    content_url: first_text_from_sources(
      &sources,
      &[
        "/share_url",
        "/url",
        "/note_url",
        "/video/share_url",
        "/share_info/link",
      ],
    ),
    published_at: first_value_from_sources(
      &sources,
      &[
        "/create_time",
        "/create_timestamp",
        "/publish_time",
        "/published_at",
      ],
    )
    .and_then(normalize_timestamp),
    region: first_text_from_sources(
      &sources,
      &["/region", "/region_code", "/author/region", "/ip_label"],
    ),
    metrics_json: collect_metrics(&sources),
    tags_json: collect_tags(&sources),
    account_fields_json,
    field_evidence_json,
  }
}

fn collect_metrics(sources: &[&Value]) -> Value {
  let keys = [
    "digg_count",
    "liked_count",
    "comment_count",
    "share_count",
    "collect_count",
    "play_count",
    "view_count",
    "follower_count",
    "following_count",
  ];
  let mut metrics = Map::new();
  for source in sources {
    for container in [source.get("statistics"), source.get("stats"), Some(*source)] {
      if let Some(object) = container.and_then(Value::as_object) {
        for key in keys {
          if let Some(value) = object.get(key).filter(|value| value.is_number()) {
            metrics
              .entry(key.to_string())
              .or_insert_with(|| value.clone());
          }
        }
      }
    }
  }
  Value::Object(metrics)
}

fn collect_tags(sources: &[&Value]) -> Value {
  let mut tags = BTreeSet::new();
  for source in sources {
    for pointer in ["/hashtags", "/tags", "/cha_list", "/tag_list"] {
      if let Some(items) = source.pointer(pointer).and_then(Value::as_array) {
        for item in items {
          let tag = value_text(item)
            .or_else(|| first_text(item, &["/title", "/name", "/hashtag_name", "/cha_name"]));
          if let Some(tag) = tag {
            tags.insert(tag);
          }
        }
      }
    }
  }
  serde_json::json!(tags.into_iter().collect::<Vec<_>>())
}

fn normalize_timestamp(value: &Value) -> Option<String> {
  if let Some(text) = value.as_str() {
    return chrono::DateTime::parse_from_rfc3339(text.trim())
      .ok()
      .map(|time| time.to_rfc3339());
  }
  let mut seconds = value.as_i64()?;
  if seconds >= 1_000_000_000_000 || seconds <= -1_000_000_000_000 {
    seconds /= 1_000;
  }
  Utc
    .timestamp_opt(seconds, 0)
    .single()
    .map(|time| time.to_rfc3339())
}

fn first_text_from_sources(sources: &[&Value], pointers: &[&str]) -> Option<String> {
  sources
    .iter()
    .find_map(|source| first_text(source, pointers))
}

fn first_value_from_sources<'a>(sources: &[&'a Value], pointers: &[&str]) -> Option<&'a Value> {
  sources
    .iter()
    .find_map(|source| pointers.iter().find_map(|pointer| source.pointer(pointer)))
}

fn first_text(value: &Value, pointers: &[&str]) -> Option<String> {
  pointers
    .iter()
    .find_map(|pointer| value.pointer(pointer).and_then(value_text))
}

fn value_text(value: &Value) -> Option<String> {
  match value {
    Value::String(text) => {
      let text = text.trim();
      (!text.is_empty()).then(|| text.to_string())
    }
    Value::Number(number) => Some(number.to_string()),
    _ => None,
  }
}

fn required_text(field: &str, value: &str, max_chars: usize) -> AppResult<String> {
  let value = value.trim();
  if value.is_empty() {
    return Err(validation_error(format!("{field} 不能为空")));
  }
  if value.chars().count() > max_chars {
    return Err(validation_error(format!(
      "{field} 长度不能超过 {max_chars} 个字符"
    )));
  }
  Ok(value.to_string())
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
  format!("{:x}", Sha256::digest(bytes))
}

pub(super) fn validation_error(message: impl Into<String>) -> AppError {
  AppError::validation(message, AppErrorStage::Collection)
}

pub(super) fn database_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    error.to_string(),
    AppErrorStage::Database,
    false,
  )
}

pub(super) fn record_file_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::WorkspaceError,
    error.to_string(),
    AppErrorStage::Collection,
    false,
  )
}

pub(super) fn permission_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::PermissionError,
    message,
    AppErrorStage::Collection,
    false,
  )
}

pub(super) fn integrity_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ExportIntegrityError,
    message,
    AppErrorStage::Collection,
    false,
  )
}

fn json_error(error: impl ToString) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    error.to_string(),
    AppErrorStage::Collection,
    false,
  )
}

#[cfg(test)]
mod summary_tests;
