use std::path::Path;

use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::domain::AppResult;

use super::{
  find_profile_by_credential, normalize_provider_type, permission_error, registry_for_access,
  scoped_workspace_connection, secret_store_error, validate_secret_ref_provider, ProfileLocation,
};

pub(crate) fn read_secret_for_snapshot(
  root_path: impl AsRef<Path>,
  secret_ref_id: &str,
  expected_provider_type: &str,
  expected_profile_id: &str,
  expected_revision: u64,
) -> AppResult<String> {
  let expected_provider_type = normalize_provider_type(expected_provider_type)?;
  let root_path = root_path.as_ref();
  let connection = scoped_workspace_connection(root_path)?;
  validate_secret_ref_provider(&connection, secret_ref_id, &expected_provider_type)?;

  let registry = registry_for_access(root_path)?;
  let location = find_profile_by_credential(&registry, secret_ref_id)
    .ok_or_else(|| secret_store_error("运行快照引用的密钥尚未迁移，请重新绑定后重试"))?;
  let (actual_provider_type, actual_profile_id) = match &location {
    ProfileLocation::Tikhub(profile_id) => ("tikhub", profile_id.as_str()),
    ProfileLocation::Ai(profile_id) => ("model_provider", profile_id.as_str()),
  };
  if actual_provider_type != expected_provider_type {
    return Err(permission_error("运行快照的 API 配置身份与当前凭据不匹配"));
  }

  let credential = registry
    .credentials
    .get(secret_ref_id)
    .ok_or_else(|| secret_store_error("运行快照引用的密钥需要重新输入"))?;
  if credential.profile_id != actual_profile_id || credential.revision != expected_revision {
    return Err(permission_error("运行快照的密钥修订号与当前凭据不匹配"));
  }

  let exact_profile_identity = actual_profile_id == expected_profile_id;
  let bound_legacy_alias = !exact_profile_identity
    && allows_bound_legacy_tikhub_alias(
      &connection,
      actual_provider_type,
      secret_ref_id,
      expected_profile_id,
      expected_revision,
    )?;
  if !exact_profile_identity && !bound_legacy_alias {
    return Err(permission_error("运行快照的 API 配置身份与当前凭据不匹配"));
  }
  Ok(credential.secret.clone())
}

fn allows_bound_legacy_tikhub_alias(
  connection: &Connection,
  actual_provider_type: &str,
  secret_ref_id: &str,
  expected_profile_id: &str,
  expected_revision: u64,
) -> AppResult<bool> {
  if actual_provider_type != "tikhub" || Uuid::parse_str(expected_profile_id).is_ok() {
    return Ok(false);
  }
  let expected_revision = i64::try_from(expected_revision)
    .map_err(|_| permission_error("运行快照的密钥修订号超出支持范围"))?;
  connection
    .query_row(
      "SELECT EXISTS(
         SELECT 1
         FROM collection_runtime_snapshot AS snapshot
         JOIN task_run AS run ON run.id = snapshot.task_run_id
         WHERE snapshot.connector_type = 'tikhub'
           AND snapshot.secret_provider_type = 'tikhub'
           AND snapshot.secret_ref_id = ?1
           AND snapshot.secret_provider_id = ?2
           AND snapshot.secret_revision = ?3
           AND run.status IN ('queued', 'running')
       )",
      params![secret_ref_id, expected_profile_id, expected_revision],
      |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists == 1)
    .map_err(super::database_error)
}
