use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, types::Type, Connection, OptionalExtension, Row, TransactionBehavior};
use serde_json::Value;
use uuid::Uuid;

use crate::domain::AppResult;
use crate::workspace::{open_workspace_database, CURRENT_SCHEMA_VERSION, DATABASE_FILE_NAME};

use super::{
  database_error, integrity_error, permission_error, record_file_error, sha256_hex,
  validation_error, NormalizedFields, NormalizedInput, NormalizedRecordView,
  PersistCollectionPageResult, PreparedRecord, RawRecordView, MAX_RAW_RECORD_BYTES,
  NORMALIZED_SCHEMA_VERSION,
};

const RAW_PARENT_DIRECTORY: &str = "raw";
const RAW_DIRECTORY: &str = "raw/tikhub";
const TEMP_DIRECTORY: &str = "temp";

pub(super) fn persist_prepared_records(
  root_path: &Path,
  input: &NormalizedInput,
  prepared: Vec<PreparedRecord>,
) -> AppResult<PersistCollectionPageResult> {
  let paths = WorkspacePaths::validate(root_path)?;
  let mut connection = open_workspace_database(&paths.database)?;
  validate_registered_workspace_root(&connection, &paths.root)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  validate_running_scope(&transaction, input)?;
  let mut created_files = Vec::new();
  let persisted = persist_records(&transaction, &paths, input, prepared, &mut created_files);

  match persisted {
    Ok(result) => match transaction.commit().map_err(database_error) {
      Ok(()) => Ok(result),
      Err(error) => {
        cleanup_files(&created_files, &paths.raw);
        Err(error)
      }
    },
    Err(error) => {
      drop(transaction);
      cleanup_files(&created_files, &paths.raw);
      Err(error)
    }
  }
}

fn persist_records(
  connection: &Connection,
  paths: &WorkspacePaths,
  input: &NormalizedInput,
  prepared: Vec<PreparedRecord>,
  created_files: &mut Vec<PathBuf>,
) -> AppResult<PersistCollectionPageResult> {
  let mut result = PersistCollectionPageResult {
    inserted_count: 0,
    existing_count: 0,
    raw_records: Vec::new(),
    normalized_records: Vec::new(),
  };

  for record in prepared {
    if let Some(existing) = find_raw_record(connection, input, &record.platform_record_id)? {
      verify_existing_raw_file(paths, input, &existing, &record.identity_hash)?;
      let normalized = find_normalized_record(connection, &existing.id)?
        .ok_or_else(|| integrity_error("raw_record 缺少对应 normalized_record"))?;
      result.existing_count += 1;
      result.raw_records.push(existing);
      result.normalized_records.push(normalized);
      continue;
    }

    let (record, created_file) = materialize_raw_snapshot(paths, record)?;
    let relative_path = raw_relative_path(&record.identity_hash)?;
    let absolute_path = paths.raw.join(format!("{}.json", record.identity_hash));
    if created_file {
      created_files.push(absolute_path);
    }
    let raw_id = Uuid::new_v4().to_string();
    let normalized_id = format!("normalized-{raw_id}");
    let now = Utc::now().to_rfc3339();
    let unmapped_field_paths =
      audit_unmapped_field_paths(&record.raw_bytes, &record.normalized.field_evidence_json)?;
    let summary_json = serde_json::json!({
      "source": "tikhub",
      "normalized_schema_version": NORMALIZED_SCHEMA_VERSION,
      "unmapped_field_paths": unmapped_field_paths
    });
    connection
      .execute(
        "INSERT INTO raw_record (
          id, task_id, task_run_id, platform, data_type, platform_record_id, raw_url,
          raw_file_path, raw_hash, summary_json, collected_at, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
          raw_id,
          input.task_id,
          input.task_run_id,
          input.platform,
          input.data_type,
          record.platform_record_id,
          record.normalized.content_url,
          relative_path,
          record.raw_hash,
          summary_json.to_string(),
          input.collected_at,
          now
        ],
      )
      .map_err(database_error)?;
    insert_normalized_record(
      connection,
      &normalized_id,
      &raw_id,
      &input.task_id,
      &input.platform,
      &record.normalized,
      &now,
    )?;
    result.inserted_count += 1;
    result
      .raw_records
      .push(get_raw_record(connection, &raw_id)?);
    result
      .normalized_records
      .push(get_normalized_record(connection, &normalized_id)?);
  }

  Ok(result)
}

fn audit_unmapped_field_paths(raw_bytes: &[u8], field_evidence: &Value) -> AppResult<Vec<String>> {
  let raw: Value = serde_json::from_slice(raw_bytes).map_err(super::json_error)?;
  let mapped_evidence = field_evidence
    .as_object()
    .into_iter()
    .flatten()
    .filter_map(|(_, evidence)| evidence.get("raw_path").and_then(Value::as_str))
    .filter(|path| path.starts_with('/'))
    .collect::<BTreeSet<_>>();
  let mut leaves = BTreeSet::new();
  collect_json_leaf_paths(&raw, "", &mut leaves);
  Ok(
    leaves
      .into_iter()
      .filter(|path| !is_mapped_raw_path(path, &mapped_evidence))
      .collect(),
  )
}

fn collect_json_leaf_paths(value: &Value, prefix: &str, paths: &mut BTreeSet<String>) {
  match value {
    Value::Object(object) => {
      for (key, child) in object {
        let key = key.replace('~', "~0").replace('/', "~1");
        collect_json_leaf_paths(child, &format!("{prefix}/{key}"), paths);
      }
    }
    Value::Array(items) => {
      for (index, child) in items.iter().enumerate() {
        collect_json_leaf_paths(child, &format!("{prefix}/{index}"), paths);
      }
    }
    _ if !prefix.is_empty() => {
      paths.insert(prefix.to_string());
    }
    _ => {}
  }
}

fn is_mapped_raw_path(path: &str, evidence: &BTreeSet<&str>) -> bool {
  if evidence.contains(path) {
    return true;
  }
  const KNOWN_LEAF_SUFFIXES: &[&str] = &[
    "/aweme_id",
    "/cid",
    "/comment_id",
    "/id",
    "/note_id",
    "/uid",
    "/user_id",
    "/userid",
    "/sec_uid",
    "/sec_user_id",
    "/unique_id",
    "/username",
    "/red_id",
    "/author_id",
    "/nickname",
    "/name",
    "/desc",
    "/text",
    "/content",
    "/title",
    "/display_title",
    "/signature",
    "/share_url",
    "/url",
    "/note_url",
    "/link",
    "/create_time",
    "/create_timestamp",
    "/publish_time",
    "/published_at",
    "/region",
    "/region_code",
    "/ip_label",
    "/digg_count",
    "/liked_count",
    "/comment_count",
    "/share_count",
    "/collect_count",
    "/play_count",
    "/view_count",
    "/follower_count",
    "/following_count",
  ];
  KNOWN_LEAF_SUFFIXES
    .iter()
    .any(|suffix| path.ends_with(suffix))
    || ["/hashtags/", "/tags/", "/cha_list/", "/tag_list/"]
      .iter()
      .any(|segment| path.contains(segment))
}

fn materialize_raw_snapshot(
  paths: &WorkspacePaths,
  record: PreparedRecord,
) -> AppResult<(PreparedRecord, bool)> {
  let final_path = paths.raw.join(format!("{}.json", record.identity_hash));
  match fs::symlink_metadata(&final_path) {
    Ok(_) => return adopt_orphan_snapshot(&final_path, record),
    Err(error) if error.kind() == ErrorKind::NotFound => {}
    Err(error) => return Err(record_file_error(error)),
  }

  match write_new_raw_file(paths, &final_path, &record.raw_bytes)? {
    true => Ok((record, true)),
    false => adopt_orphan_snapshot(&final_path, record),
  }
}

fn adopt_orphan_snapshot(path: &Path, record: PreparedRecord) -> AppResult<(PreparedRecord, bool)> {
  let bytes = read_bounded_regular_file(path)?;
  if sha256_hex(&bytes) != record.raw_hash {
    return Err(integrity_error("孤儿原始记录文件哈希与本次采集记录不一致"));
  }
  Ok((record, false))
}

fn write_new_raw_file(paths: &WorkspacePaths, final_path: &Path, bytes: &[u8]) -> AppResult<bool> {
  paths.revalidate_directories()?;
  let temp_path = paths.temp.join(format!("raw-{}.tmp", Uuid::new_v4()));
  let mut options = OpenOptions::new();
  options.write(true).create_new(true);
  #[cfg(unix)]
  options.mode(0o600);
  let mut file = options.open(&temp_path).map_err(record_file_error)?;
  let write_result = (|| -> AppResult<bool> {
    file.write_all(bytes).map_err(record_file_error)?;
    file.sync_all().map_err(record_file_error)?;
    paths.revalidate_directories()?;
    match fs::hard_link(&temp_path, final_path) {
      Ok(()) => {
        sync_directory(&paths.raw)?;
        Ok(true)
      }
      Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(false),
      Err(error) => Err(record_file_error(error)),
    }
  })();
  drop(file);
  fs::remove_file(&temp_path).ok();
  let _ = sync_directory(&paths.temp);
  write_result
}

fn verify_existing_raw_file(
  paths: &WorkspacePaths,
  input: &NormalizedInput,
  raw: &RawRecordView,
  expected_identity_hash: &str,
) -> AppResult<()> {
  let expected_relative = raw_relative_path(expected_identity_hash)?;
  if raw.raw_file_path != expected_relative
    || raw.task_run_id.as_deref() != Some(input.task_run_id.as_str())
    || raw.data_type != input.data_type
  {
    return Err(integrity_error("raw_record 文件路径或观察身份不一致"));
  }
  let bytes = read_bounded_regular_file(&paths.raw.join(format!("{expected_identity_hash}.json")))?;
  if sha256_hex(&bytes) != raw.raw_hash {
    return Err(integrity_error("原始记录文件哈希校验失败"));
  }
  Ok(())
}

fn raw_relative_path(identity_hash: &str) -> AppResult<String> {
  if identity_hash.len() != 64
    || !identity_hash
      .bytes()
      .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
  {
    return Err(integrity_error("原始记录身份哈希格式无效"));
  }
  Ok(format!("{RAW_DIRECTORY}/{identity_hash}.json"))
}

fn read_bounded_regular_file(path: &Path) -> AppResult<Vec<u8>> {
  let metadata = fs::symlink_metadata(path).map_err(record_file_error)?;
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    return Err(permission_error(
      "原始记录路径必须是普通文件，不能是符号链接",
    ));
  }
  if metadata.len() > MAX_RAW_RECORD_BYTES as u64 {
    return Err(integrity_error("原始记录文件超过 16 MiB 安全上限"));
  }
  let file = File::open(path).map_err(record_file_error)?;
  let mut bytes = Vec::with_capacity(metadata.len() as usize);
  file
    .take(MAX_RAW_RECORD_BYTES as u64 + 1)
    .read_to_end(&mut bytes)
    .map_err(record_file_error)?;
  if bytes.len() > MAX_RAW_RECORD_BYTES {
    return Err(integrity_error("原始记录文件超过 16 MiB 安全上限"));
  }
  Ok(bytes)
}

fn cleanup_files(paths: &[PathBuf], raw_directory: &Path) {
  for path in paths {
    fs::remove_file(path).ok();
  }
  let _ = sync_directory(raw_directory);
}

fn sync_directory(path: &Path) -> AppResult<()> {
  File::open(path)
    .and_then(|directory| directory.sync_all())
    .map_err(record_file_error)
}

fn validate_running_scope(connection: &Connection, input: &NormalizedInput) -> AppResult<()> {
  let state = connection
    .query_row(
      "SELECT task.status, run.status, task.platforms_json, task.data_types_json,
              plan.schema_version, plan.plan_json
       FROM task_run run
       JOIN collection_task task ON task.id = run.task_id
       LEFT JOIN collection_plan plan
         ON plan.id = run.plan_id AND plan.task_id = task.id
        AND plan.validation_status = 'valid' AND plan.confirmed_by_user = 1
       WHERE task.id = ?1 AND run.id = ?2 AND run.task_id = task.id",
      params![input.task_id, input.task_run_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
          row.get::<_, String>(3)?,
          row.get::<_, Option<i64>>(4)?,
          row.get::<_, Option<String>>(5)?,
        ))
      },
    )
    .optional()
    .map_err(database_error)?
    .ok_or_else(|| validation_error("任务运行记录不存在或不属于该任务"))?;
  if state.0 != "running" || state.1 != "running" {
    return Err(validation_error("只允许为运行中的任务持久化采集记录"));
  }
  if !json_array_contains(&state.2, &input.platform)? {
    return Err(validation_error("采集平台不在任务确认范围内"));
  }
  let confirmed_internal_type = state.5.as_deref().is_some_and(|plan_json| match state.4 {
    Some(3) => plan_array_contains(plan_json, "internal_data_types", &input.data_type),
    Some(4) => plan_steps_contain_data_type(plan_json, &input.data_type),
    _ => false,
  });
  if !json_array_contains(&state.3, &input.data_type)? && !confirmed_internal_type {
    return Err(validation_error("数据类型不在任务确认范围内"));
  }
  Ok(())
}

fn validate_registered_workspace_root(connection: &Connection, root: &Path) -> AppResult<()> {
  let mut statement = connection
    .prepare("SELECT root_path, schema_version FROM workspace ORDER BY created_at LIMIT 2")
    .map_err(database_error)?;
  let rows = statement
    .query_map([], |row| {
      Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })
    .map_err(database_error)?;
  let workspaces = rows
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(database_error)?;
  if workspaces.len() != 1 {
    return Err(integrity_error(
      "工作区数据库必须且只能包含一条工作区元数据",
    ));
  }
  let (registered_root, schema_version) = &workspaces[0];
  let registered_root = fs::canonicalize(registered_root).map_err(record_file_error)?;
  if registered_root != root {
    return Err(permission_error("工作区数据库登记路径与当前根目录不一致"));
  }
  if *schema_version != CURRENT_SCHEMA_VERSION {
    return Err(integrity_error("工作区数据库尚未完成当前 Schema 迁移"));
  }
  Ok(())
}

fn json_array_contains(text: &str, expected: &str) -> AppResult<bool> {
  let values =
    serde_json::from_str::<Vec<String>>(text).map_err(|_| integrity_error("任务范围 JSON 损坏"))?;
  Ok(values.iter().any(|value| value == expected))
}

fn plan_array_contains(plan_json: &str, field: &str, expected: &str) -> bool {
  serde_json::from_str::<Value>(plan_json)
    .ok()
    .and_then(|plan| plan.get(field).and_then(Value::as_array).cloned())
    .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(expected)))
}

fn plan_steps_contain_data_type(plan_json: &str, expected: &str) -> bool {
  serde_json::from_str::<Value>(plan_json)
    .ok()
    .and_then(|plan| plan.get("steps").and_then(Value::as_array).cloned())
    .is_some_and(|steps| {
      steps
        .iter()
        .any(|step| step.get("data_type").and_then(Value::as_str) == Some(expected))
    })
}

fn insert_normalized_record(
  connection: &Connection,
  id: &str,
  raw_record_id: &str,
  task_id: &str,
  platform: &str,
  fields: &NormalizedFields,
  created_at: &str,
) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO normalized_record (
        id, raw_record_id, task_id, platform, author_id, author_name, content_text,
        content_url, published_at, region, metrics_json, tags_json, account_fields_json,
        field_evidence_json, normalized_schema_version, created_at
      ) VALUES (
        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16
      )",
      params![
        id,
        raw_record_id,
        task_id,
        platform,
        fields.author_id,
        fields.author_name,
        fields.content_text,
        fields.content_url,
        fields.published_at,
        fields.region,
        fields.metrics_json.to_string(),
        fields.tags_json.to_string(),
        fields.account_fields_json.to_string(),
        fields.field_evidence_json.to_string(),
        NORMALIZED_SCHEMA_VERSION,
        created_at
      ],
    )
    .map(|_| ())
    .map_err(database_error)
}

fn find_raw_record(
  connection: &Connection,
  input: &NormalizedInput,
  platform_record_id: &str,
) -> AppResult<Option<RawRecordView>> {
  connection
    .query_row(
      "SELECT id, task_id, task_run_id, platform, data_type, platform_record_id,
              raw_url, raw_file_path, raw_hash, summary_json, collected_at, created_at
       FROM raw_record
       WHERE task_id = ?1 AND task_run_id = ?2 AND platform = ?3
         AND data_type = ?4 AND platform_record_id = ?5",
      params![
        input.task_id,
        input.task_run_id,
        input.platform,
        input.data_type,
        platform_record_id
      ],
      map_raw_record,
    )
    .optional()
    .map_err(database_error)
}

fn find_normalized_record(
  connection: &Connection,
  raw_record_id: &str,
) -> AppResult<Option<NormalizedRecordView>> {
  connection
    .query_row(
      "SELECT id, raw_record_id, task_id, platform, author_id, author_name,
              content_text, content_url, published_at, region, metrics_json,
              tags_json, account_fields_json, field_evidence_json,
              normalized_schema_version, created_at
       FROM normalized_record WHERE raw_record_id = ?1",
      params![raw_record_id],
      map_normalized_record,
    )
    .optional()
    .map_err(database_error)
}

fn get_raw_record(connection: &Connection, id: &str) -> AppResult<RawRecordView> {
  connection
    .query_row(
      "SELECT id, task_id, task_run_id, platform, data_type, platform_record_id,
              raw_url, raw_file_path, raw_hash, summary_json, collected_at, created_at
       FROM raw_record WHERE id = ?1",
      params![id],
      map_raw_record,
    )
    .map_err(database_error)
}

fn get_normalized_record(connection: &Connection, id: &str) -> AppResult<NormalizedRecordView> {
  connection
    .query_row(
      "SELECT id, raw_record_id, task_id, platform, author_id, author_name,
              content_text, content_url, published_at, region, metrics_json,
              tags_json, account_fields_json, field_evidence_json,
              normalized_schema_version, created_at
       FROM normalized_record WHERE id = ?1",
      params![id],
      map_normalized_record,
    )
    .map_err(database_error)
}

fn map_raw_record(row: &Row<'_>) -> rusqlite::Result<RawRecordView> {
  Ok(RawRecordView {
    id: row.get(0)?,
    task_id: row.get(1)?,
    task_run_id: row.get(2)?,
    platform: row.get(3)?,
    data_type: row.get(4)?,
    platform_record_id: row.get(5)?,
    raw_url: row.get(6)?,
    raw_file_path: row.get(7)?,
    raw_hash: row.get(8)?,
    summary_json: json_column(row, 9)?,
    collected_at: row.get(10)?,
    created_at: row.get(11)?,
  })
}

fn map_normalized_record(row: &Row<'_>) -> rusqlite::Result<NormalizedRecordView> {
  Ok(NormalizedRecordView {
    id: row.get(0)?,
    raw_record_id: row.get(1)?,
    task_id: row.get(2)?,
    platform: row.get(3)?,
    author_id: row.get(4)?,
    author_name: row.get(5)?,
    content_text: row.get(6)?,
    content_url: row.get(7)?,
    published_at: row.get(8)?,
    region: row.get(9)?,
    metrics_json: json_column(row, 10)?,
    tags_json: json_column(row, 11)?,
    account_fields_json: json_column(row, 12)?,
    field_evidence_json: json_column(row, 13)?,
    normalized_schema_version: row.get(14)?,
    created_at: row.get(15)?,
  })
}

fn json_column(row: &Row<'_>, index: usize) -> rusqlite::Result<Value> {
  let text = row.get::<_, String>(index)?;
  serde_json::from_str(&text)
    .map_err(|error| rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(error)))
}

#[derive(Debug)]
struct WorkspacePaths {
  root: PathBuf,
  database: PathBuf,
  raw: PathBuf,
  temp: PathBuf,
}

impl WorkspacePaths {
  fn validate(root_path: &Path) -> AppResult<Self> {
    ensure_directory(root_path, "工作区根目录")?;
    let root = fs::canonicalize(root_path).map_err(record_file_error)?;
    let database_path = root_path.join(DATABASE_FILE_NAME);
    ensure_regular_file(&database_path, "工作区数据库")?;
    let database = fs::canonicalize(&database_path).map_err(record_file_error)?;
    if database.parent() != Some(root.as_path()) {
      return Err(permission_error("工作区数据库不在工作区根目录内"));
    }
    ensure_directory(&root_path.join(RAW_PARENT_DIRECTORY), "原始数据目录")?;
    ensure_directory(&root_path.join(RAW_DIRECTORY), "TikHub 原始数据目录")?;
    ensure_directory(&root_path.join(TEMP_DIRECTORY), "临时目录")?;
    let raw = fs::canonicalize(root_path.join(RAW_DIRECTORY)).map_err(record_file_error)?;
    let temp = fs::canonicalize(root_path.join(TEMP_DIRECTORY)).map_err(record_file_error)?;
    if !raw.starts_with(&root) || !temp.starts_with(&root) {
      return Err(permission_error("工作区写入目录越过根目录边界"));
    }
    Ok(Self {
      root,
      database,
      raw,
      temp,
    })
  }

  fn revalidate_directories(&self) -> AppResult<()> {
    ensure_directory(&self.raw, "TikHub 原始数据目录")?;
    ensure_directory(&self.temp, "临时目录")?;
    let raw = fs::canonicalize(&self.raw).map_err(record_file_error)?;
    let temp = fs::canonicalize(&self.temp).map_err(record_file_error)?;
    if raw != self.raw
      || temp != self.temp
      || !raw.starts_with(&self.root)
      || !temp.starts_with(&self.root)
    {
      return Err(permission_error("工作区目录在写入期间发生变化"));
    }
    Ok(())
  }
}

fn ensure_directory(path: &Path, label: &str) -> AppResult<()> {
  let metadata = fs::symlink_metadata(path).map_err(record_file_error)?;
  if metadata.file_type().is_symlink() || !metadata.is_dir() {
    return Err(permission_error(format!("{label}必须是非符号链接目录")));
  }
  Ok(())
}

fn ensure_regular_file(path: &Path, label: &str) -> AppResult<()> {
  let metadata = fs::symlink_metadata(path).map_err(record_file_error)?;
  if metadata.file_type().is_symlink() || !metadata.is_file() {
    return Err(permission_error(format!("{label}必须是非符号链接普通文件")));
  }
  Ok(())
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;
