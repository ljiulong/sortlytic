use rusqlite::Transaction;
use serde_json::Value;

use super::{database_error, kind_name, ApiProfileKind};
use crate::domain::AppResult;

pub(super) fn auto_activation_allowed(
  transaction: &Transaction<'_>,
  kind: ApiProfileKind,
  profile_id: &str,
  profile_count: usize,
) -> AppResult<bool> {
  let mut statement = transaction
    .prepare(
      "SELECT entity_id,action,safe_details_json FROM audit_log
       WHERE entity_type = 'api_profile'
         AND action IN ('save_api_profile','delete_api_profile')
       ORDER BY rowid",
    )
    .map_err(database_error)?;
  let entries = statement
    .query_map([], |row| {
      Ok((
        row.get::<_, Option<String>>(0)?,
        row.get::<_, String>(1)?,
        row.get::<_, String>(2)?,
      ))
    })
    .map_err(database_error)?;
  let expected_kind = kind_name(kind);
  let mut first_created_id = None;
  let mut any_delete = false;
  let mut deleted_current = false;
  for entry in entries {
    let (entity_id, action, details) = entry.map_err(database_error)?;
    let Ok(details) = serde_json::from_str::<Value>(&details) else {
      return Ok(false);
    };
    if details.get("kind").and_then(Value::as_str) != Some(expected_kind) {
      continue;
    }
    if action == "save_api_profile" && details.get("created").and_then(Value::as_bool) == Some(true)
    {
      let Some(entity_id) = entity_id else {
        return Ok(false);
      };
      first_created_id.get_or_insert(entity_id);
    } else if action == "delete_api_profile" {
      any_delete = true;
      deleted_current |= details
        .get("was_active")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    }
  }
  if deleted_current {
    return Ok(false);
  }
  if let Some(first_created_id) = first_created_id {
    return Ok(first_created_id == profile_id);
  }
  Ok(!any_delete && profile_count == 1)
}
