use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::domain::AppResult;

use super::{
  database_error, ensure_foreign_key_integrity, update_workspace_schema_version, workspace_error,
};

const MIGRATION_NAME: &str = "api_profile_claim_binding";
const BACKUP_DIRECTORY_NAME: &str = "backups";
const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;
const PERMISSION_BITS: u32 = 0o7777;

const SNAPSHOT_DELETE_TRIGGER_SQL: &str = r#"CREATE TRIGGER trg_collection_runtime_snapshot_immutable_delete
BEFORE DELETE ON collection_runtime_snapshot
WHEN EXISTS (SELECT 1 FROM task_run WHERE id = OLD.task_run_id)
BEGIN
  SELECT RAISE(ABORT, 'collection runtime snapshot cannot be deleted directly');
END;"#;

const CLEAN_FRESH_QUEUED_SNAPSHOTS_SQL: &str = r#"DELETE FROM collection_runtime_snapshot
WHERE task_run_id IN (
  SELECT run.id
  FROM task_run AS run
  WHERE run.status = 'queued'
    AND run.claimed_at IS NULL
    AND COALESCE(run.current_stage, '') NOT IN (
      '恢复响应入库', '恢复重试', '恢复待发送',
      '恢复续页', '恢复收尾', '恢复等待'
    )
    AND NOT EXISTS (
      SELECT 1
      FROM task_run_step AS step
      WHERE step.task_run_id = run.id AND (
        step.status <> 'pending' OR step.stop_reason IS NOT NULL
        OR step.started_at IS NOT NULL OR step.completed_at IS NOT NULL
      )
    )
    AND NOT EXISTS (
      SELECT 1
      FROM collection_page_checkpoint AS checkpoint
      JOIN task_run_step AS step ON step.id = checkpoint.task_run_step_id
      WHERE step.task_run_id = run.id
    )
);"#;

pub(super) fn validate_existing_api_profile_migration(connection: &Connection) -> AppResult<()> {
  if !table_exists(connection, "schema_migrations")? {
    return Ok(());
  }
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker(&name, &checksum)?;
    return Ok(());
  }
  if declared_schema_version(connection)?.is_some_and(|version| version >= 8) {
    return Err(workspace_error(
      "数据库声明为 v8，但缺少 API 配置领取迁移标记",
    ));
  }
  Ok(())
}

pub(super) fn apply_api_profile_migration(connection: &mut Connection) -> AppResult<()> {
  if let Some((name, checksum)) = marker(connection)? {
    validate_marker(&name, &checksum)?;
    update_workspace_schema_version(connection, 8)?;
    return ensure_foreign_key_integrity(connection);
  }

  let workspace_count: i64 = connection
    .query_row("SELECT COUNT(*) FROM workspace", [], |row| row.get(0))
    .map_err(database_error)?;
  if workspace_count > 1 {
    return Err(workspace_error(
      "v8 迁移要求工作区元数据恰好一条，已拒绝继续",
    ));
  }
  if workspace_count == 0 {
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .map_err(database_error)?;
    record_migration(&transaction)?;
    transaction.commit().map_err(database_error)?;
    return Ok(());
  }

  let backup_path = create_consistent_v7_backup(connection)?;
  let transaction = connection
    .transaction_with_behavior(TransactionBehavior::Immediate)
    .map_err(database_error)?;
  transaction
    .execute_batch("DROP TRIGGER IF EXISTS trg_collection_runtime_snapshot_immutable_delete;")
    .map_err(database_error)?;
  let removed = transaction
    .execute(CLEAN_FRESH_QUEUED_SNAPSHOTS_SQL, [])
    .map_err(database_error)?;
  transaction
    .execute_batch(SNAPSHOT_DELETE_TRIGGER_SQL)
    .map_err(database_error)?;
  let workspace_id: String = transaction
    .query_row("SELECT id FROM workspace", [], |row| row.get(0))
    .map_err(database_error)?;
  transaction
    .execute(
      "INSERT INTO audit_log (
         id, entity_type, entity_id, action, safe_details_json, created_at
       ) VALUES (?1, 'workspace', ?2, 'migrate_api_profile_claim_binding', ?3, ?4)",
      params![
        Uuid::new_v4().to_string(),
        workspace_id,
        serde_json::json!({
          "migration_version": 8,
          "removed_fresh_queue_snapshots": removed,
          "backup_file_name": backup_path.file_name().and_then(|name| name.to_str()),
        })
        .to_string(),
        Utc::now().to_rfc3339(),
      ],
    )
    .map_err(database_error)?;
  record_migration(&transaction)?;
  transaction.commit().map_err(database_error)?;
  validate_marker(MIGRATION_NAME, &migration_checksum())?;
  ensure_foreign_key_integrity(connection)
}

fn record_migration(connection: &Connection) -> AppResult<()> {
  connection
    .execute(
      "INSERT INTO schema_migrations (version, name, applied_at, checksum)
       VALUES (8, ?1, ?2, ?3)",
      params![
        MIGRATION_NAME,
        Utc::now().to_rfc3339(),
        migration_checksum()
      ],
    )
    .map_err(database_error)?;
  update_workspace_schema_version(connection, 8)
}

fn create_consistent_v7_backup(connection: &Connection) -> AppResult<PathBuf> {
  let registered_root: String = connection
    .query_row("SELECT root_path FROM workspace", [], |row| row.get(0))
    .map_err(database_error)?;
  let root_path = fs::canonicalize(&registered_root)
    .map_err(|error| workspace_error(format!("无法解析 v8 迁移备份工作区路径：{error}")))?;
  let backup_directory = ensure_private_backup_directory(&root_path)?;
  let backup_path = backup_directory.join(format!(
    "app-v7-before-v8-{}-{}.sqlite",
    Utc::now().timestamp_millis(),
    Uuid::new_v4()
  ));
  let result = (|| -> AppResult<()> {
    let file = OpenOptions::new()
      .write(true)
      .create_new(true)
      .mode(PRIVATE_FILE_MODE)
      .open(&backup_path)
      .map_err(|error| workspace_error(format!("无法创建 v8 迁移备份：{error}")))?;
    file
      .set_permissions(fs::Permissions::from_mode(PRIVATE_FILE_MODE))
      .map_err(|error| workspace_error(format!("无法设置 v8 迁移备份权限：{error}")))?;
    drop(file);
    connection
      .execute("VACUUM INTO ?1", params![backup_path.to_string_lossy()])
      .map_err(|error| database_error(format!("无法生成 v8 迁移一致性备份：{error}")))?;
    fs::set_permissions(&backup_path, fs::Permissions::from_mode(PRIVATE_FILE_MODE))
      .map_err(|error| workspace_error(format!("无法固定 v8 迁移备份权限：{error}")))?;
    validate_private_regular_file(&backup_path)?;
    File::open(&backup_path)
      .and_then(|file| file.sync_all())
      .map_err(|error| workspace_error(format!("无法同步 v8 迁移备份：{error}")))?;
    File::open(&backup_directory)
      .and_then(|file| file.sync_all())
      .map_err(|error| workspace_error(format!("无法同步 v8 迁移备份目录：{error}")))?;
    Ok(())
  })();
  if result.is_err() {
    fs::remove_file(&backup_path).ok();
  }
  result.map(|_| backup_path)
}

fn ensure_private_backup_directory(root_path: &Path) -> AppResult<PathBuf> {
  let directory = root_path.join(BACKUP_DIRECTORY_NAME);
  match fs::symlink_metadata(&directory) {
    Ok(metadata) => validate_private_directory(&metadata)?,
    Err(error) if error.kind() == ErrorKind::NotFound => {
      let mut builder = DirBuilder::new();
      builder.mode(PRIVATE_DIRECTORY_MODE);
      builder
        .create(&directory)
        .map_err(|error| workspace_error(format!("无法创建 v8 迁移备份目录：{error}")))?;
      fs::set_permissions(
        &directory,
        fs::Permissions::from_mode(PRIVATE_DIRECTORY_MODE),
      )
      .map_err(|error| workspace_error(format!("无法设置 v8 迁移备份目录权限：{error}")))?;
      validate_private_directory(
        &fs::symlink_metadata(&directory)
          .map_err(|error| workspace_error(format!("无法检查 v8 备份目录：{error}")))?,
      )?;
    }
    Err(error) => {
      return Err(workspace_error(format!(
        "无法检查 v8 迁移备份目录：{error}"
      )))
    }
  }
  Ok(directory)
}

fn validate_private_directory(metadata: &fs::Metadata) -> AppResult<()> {
  if metadata.file_type().is_symlink()
    || !metadata.is_dir()
    || metadata.permissions().mode() & PERMISSION_BITS != PRIVATE_DIRECTORY_MODE
  {
    return Err(workspace_error(
      "v8 迁移备份目录必须是真实目录且权限为 0700",
    ));
  }
  Ok(())
}

fn validate_private_regular_file(path: &Path) -> AppResult<()> {
  let metadata = fs::symlink_metadata(path)
    .map_err(|error| workspace_error(format!("无法检查 v8 迁移备份：{error}")))?;
  if metadata.file_type().is_symlink()
    || !metadata.is_file()
    || metadata.permissions().mode() & PERMISSION_BITS != PRIVATE_FILE_MODE
  {
    return Err(workspace_error("v8 迁移备份必须是普通文件且权限为 0600"));
  }
  Ok(())
}

fn validate_marker(name: &str, checksum: &str) -> AppResult<()> {
  if name != MIGRATION_NAME || checksum != migration_checksum() {
    return Err(workspace_error(
      "数据库迁移 v8 校验失败，API 配置领取迁移标记或 checksum 不一致",
    ));
  }
  Ok(())
}

fn migration_checksum() -> String {
  let mut hasher = Sha256::new();
  hasher.update(CLEAN_FRESH_QUEUED_SNAPSHOTS_SQL.as_bytes());
  hasher.update(SNAPSHOT_DELETE_TRIGGER_SQL.as_bytes());
  format!("{:x}", hasher.finalize())
}

fn marker(connection: &Connection) -> AppResult<Option<(String, String)>> {
  connection
    .query_row(
      "SELECT name, checksum FROM schema_migrations WHERE version = 8",
      [],
      |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
    .map_err(database_error)
}

fn declared_schema_version(connection: &Connection) -> AppResult<Option<i64>> {
  connection
    .query_row("SELECT MAX(schema_version) FROM workspace", [], |row| {
      row.get(0)
    })
    .map_err(database_error)
}

fn table_exists(connection: &Connection, table: &str) -> AppResult<bool> {
  connection
    .query_row(
      "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
      params![table],
      |row| row.get(0),
    )
    .map_err(database_error)
}

#[cfg(test)]
#[path = "api_profile_migration_tests.rs"]
mod tests;
