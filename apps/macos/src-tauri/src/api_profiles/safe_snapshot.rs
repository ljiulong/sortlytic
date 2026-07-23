use std::path::Path;

use rusqlite::OptionalExtension;

use super::ApiProfileRegistry;
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::workspace::{open_workspace_database, DATABASE_FILE_NAME};

pub(super) fn read(root_path: &Path) -> AppResult<serde_json::Value> {
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME))?;
  let details = connection
    .query_row(
      "SELECT safe_details_json
       FROM audit_log
       WHERE entity_type = 'api_profile_registry'
         AND action = 'mirror_api_profiles'
       ORDER BY rowid DESC
       LIMIT 1",
      [],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|error| database_error("无法读取 API 配置安全状态镜像", error))?
    .ok_or_else(|| snapshot_error("API 配置安全状态镜像不存在"))?;
  let details = serde_json::from_str::<serde_json::Value>(&details)
    .map_err(|_| snapshot_error("API 配置安全状态镜像已损坏"))?;
  details
    .get("registry_view")
    .filter(|value| value.is_object())
    .cloned()
    .ok_or_else(|| snapshot_error("API 配置安全状态镜像缺少注册表视图"))
}

pub(super) fn value(registry: &ApiProfileRegistry) -> serde_json::Value {
  let mut tikhub_profiles = registry.tikhub_profiles.values().collect::<Vec<_>>();
  tikhub_profiles.sort_by(|left, right| {
    left
      .created_at
      .cmp(&right.created_at)
      .then_with(|| left.name.cmp(&right.name))
  });
  let mut ai_profiles = registry.ai_profiles.values().collect::<Vec<_>>();
  ai_profiles.sort_by(|left, right| {
    left
      .created_at
      .cmp(&right.created_at)
      .then_with(|| left.name.cmp(&right.name))
  });
  serde_json::json!({
    "activeProfileIds": {
      "tikhub": registry.active_profile_ids.tikhub,
      "ai": registry.active_profile_ids.ai,
    },
    "tikhubProfiles": tikhub_profiles.into_iter().map(|profile| {
      let credential = registry.credentials.get(&profile.credential_ref_id);
      serde_json::json!({
        "kind": "tikhub",
        "id": profile.id,
        "name": profile.name,
        "baseUrl": profile.base_url,
        "status": profile.status,
        "revision": profile.revision,
        "maskedKey": credential.map(|value| mask_secret(&value.secret)),
        "hasCredential": credential.is_some(),
        "isActive": registry.active_profile_ids.tikhub.as_deref() == Some(profile.id.as_str()),
        "lastTestedAt": profile.last_tested_at,
        "testSummary": profile.test_summary.as_ref().map(|summary| serde_json::json!({
          "maskedAccount": summary.masked_account,
          "balance": summary.balance,
          "freeCredit": summary.free_credit,
          "availableCredit": summary.available_credit,
          "todayUsage": summary.today_usage,
        })),
        "createdAt": profile.created_at,
        "updatedAt": profile.updated_at,
      })
    }).collect::<Vec<_>>(),
    "aiProfiles": ai_profiles.into_iter().map(|profile| {
      let credential = profile
        .credential_ref_id
        .as_ref()
        .and_then(|id| registry.credentials.get(id));
      serde_json::json!({
        "kind": "ai",
        "id": profile.id,
        "name": profile.name,
        "providerType": profile.provider_type,
        "apiFormat": profile.api_format,
        "baseUrl": profile.base_url,
        "defaultModelId": profile.default_model_id,
        "status": profile.status,
        "revision": profile.revision,
        "maskedKey": credential.map(|value| mask_secret(&value.secret)),
        "hasCredential": credential.is_some(),
        "isActive": registry.active_profile_ids.ai.as_deref() == Some(profile.id.as_str()),
        "lastTestedAt": profile.last_tested_at,
        "createdAt": profile.created_at,
        "updatedAt": profile.updated_at,
      })
    }).collect::<Vec<_>>(),
  })
}

fn mask_secret(secret: &str) -> String {
  let chars = secret.chars().collect::<Vec<_>>();
  if chars.len() <= 8 {
    return "[REDACTED]".to_string();
  }
  let prefix = chars.iter().take(4).collect::<String>();
  let suffix = chars[chars.len() - 4..].iter().collect::<String>();
  format!("{prefix}...[REDACTED]...{suffix}")
}

fn database_error(context: &str, error: rusqlite::Error) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    format!("{context}：{error}"),
    AppErrorStage::Database,
    false,
  )
}

fn snapshot_error(message: &str) -> AppError {
  AppError::new(
    AppErrorCode::DatabaseError,
    message,
    AppErrorStage::Database,
    false,
  )
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;

  use chrono::Utc;
  use uuid::Uuid;

  use super::*;
  use crate::api_profiles::{
    api_profile_registry_path, save_api_profile_registry, sync_api_profile_mirror,
    ActiveApiProfileIds, AiApiFormat, AiApiProfile, AiProviderType, ApiCredential,
    ApiProfileStatus, CredentialProviderType, TikhubApiProfile, TikhubSafeTestSummary,
    API_PROFILE_SCHEMA_VERSION,
  };
  use crate::workspace::create_workspace;

  #[test]
  fn safe_registry_snapshot_remains_readable_without_opening_the_secret_registry() {
    const TIKHUB_SENTINEL: &str = "tikhub-status-read-sentinel-7193";
    const AI_SENTINEL: &str = "ai-status-read-sentinel-4821";
    let root = std::env::temp_dir().join(format!("api-safe-snapshot-{}", Uuid::new_v4()));
    create_workspace("API 安全状态测试", &root).unwrap();
    let tikhub_id = Uuid::new_v4().to_string();
    let tikhub_secret_id = Uuid::new_v4().to_string();
    let ai_id = Uuid::new_v4().to_string();
    let ai_secret_id = Uuid::new_v4().to_string();
    let timestamp = Utc::now().to_rfc3339();
    let registry = ApiProfileRegistry {
      schema_version: API_PROFILE_SCHEMA_VERSION,
      active_profile_ids: ActiveApiProfileIds {
        tikhub: Some(tikhub_id.clone()),
        ai: Some(ai_id.clone()),
      },
      tikhub_profiles: BTreeMap::from([(
        tikhub_id.clone(),
        TikhubApiProfile {
          id: tikhub_id.clone(),
          name: "TikHub 生产".to_string(),
          base_url: "https://api.tikhub.io".to_string(),
          credential_ref_id: tikhub_secret_id.clone(),
          revision: 1,
          status: ApiProfileStatus::Success,
          last_tested_at: Some(timestamp.clone()),
          test_summary: Some(TikhubSafeTestSummary {
            masked_account: Some("s***n@example.test".to_string()),
            balance: Some(4.0),
            free_credit: Some(1.0),
            available_credit: Some(5.0),
            today_usage: Some(2.0),
          }),
          created_at: timestamp.clone(),
          updated_at: timestamp.clone(),
        },
      )]),
      ai_profiles: BTreeMap::from([(
        ai_id.clone(),
        AiApiProfile {
          id: ai_id.clone(),
          name: "AI 生产".to_string(),
          provider_type: AiProviderType::CustomOpenaiCompatible,
          api_format: AiApiFormat::OpenaiCompatible,
          base_url: "https://example.test/v1".to_string(),
          default_model_id: "model-test".to_string(),
          credential_ref_id: Some(ai_secret_id.clone()),
          revision: 1,
          status: ApiProfileStatus::Success,
          last_tested_at: Some(timestamp.clone()),
          created_at: timestamp.clone(),
          updated_at: timestamp.clone(),
        },
      )]),
      credentials: BTreeMap::from([
        (
          tikhub_secret_id.clone(),
          ApiCredential {
            id: tikhub_secret_id,
            provider_type: CredentialProviderType::Tikhub,
            profile_id: tikhub_id.clone(),
            revision: 1,
            secret: TIKHUB_SENTINEL.to_string(),
          },
        ),
        (
          ai_secret_id.clone(),
          ApiCredential {
            id: ai_secret_id,
            provider_type: CredentialProviderType::CustomOpenaiCompatible,
            profile_id: ai_id.clone(),
            revision: 1,
            secret: AI_SENTINEL.to_string(),
          },
        ),
      ]),
    };
    save_api_profile_registry(&root, &registry).unwrap();
    sync_api_profile_mirror(&root).unwrap();
    fs::write(
      api_profile_registry_path(&root),
      b"{ secret registry must not be opened",
    )
    .unwrap();

    let snapshot = read(&root).unwrap();
    let snapshot_text = serde_json::to_string(&snapshot).unwrap();

    assert_eq!(
      snapshot.pointer("/activeProfileIds/tikhub"),
      Some(&serde_json::Value::String(tikhub_id))
    );
    assert_eq!(
      snapshot.pointer("/activeProfileIds/ai"),
      Some(&serde_json::Value::String(ai_id))
    );
    assert_eq!(
      snapshot.pointer("/tikhubProfiles/0/hasCredential"),
      Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
      snapshot.pointer("/aiProfiles/0/hasCredential"),
      Some(&serde_json::Value::Bool(true))
    );
    assert!(!snapshot_text.contains(TIKHUB_SENTINEL));
    assert!(!snapshot_text.contains(AI_SENTINEL));
    fs::remove_dir_all(root).ok();
  }
}
