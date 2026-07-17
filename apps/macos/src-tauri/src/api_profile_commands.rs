use serde::{Deserialize, Serialize};

use crate::api_profiles::{
  AiApiFormat, AiProviderType, ApiProfileRegistry, ApiProfileStatus, TikhubSafeTestSummary,
};
use crate::domain::{AppError, AppErrorCode, AppErrorStage, AppResult};
use crate::secrets::mask_secret;

use super::{resolve_workspace_root, AppState};

mod service;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiProfileKind {
  Tikhub,
  Ai,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SaveApiProfileInput {
  Tikhub {
    id: Option<String>,
    name: String,
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
  },
  Ai {
    id: Option<String>,
    name: String,
    #[serde(rename = "providerType")]
    provider_type: AiProviderType,
    #[serde(rename = "apiFormat")]
    api_format: AiApiFormat,
    #[serde(rename = "baseUrl")]
    base_url: String,
    #[serde(rename = "defaultModelId")]
    default_model_id: String,
    #[serde(rename = "apiKey")]
    api_key: Option<String>,
  },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApiProfileRegistryView {
  pub active_profile_ids: SafeActiveProfileIds,
  pub tikhub_profiles: Vec<TikhubApiProfileView>,
  pub ai_profiles: Vec<AiApiProfileView>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SafeActiveProfileIds {
  pub tikhub: Option<String>,
  pub ai: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TikhubApiProfileView {
  pub kind: ApiProfileKind,
  pub id: String,
  pub name: String,
  pub base_url: String,
  pub status: ApiProfileStatus,
  pub revision: u64,
  pub masked_key: Option<String>,
  pub has_credential: bool,
  pub is_active: bool,
  pub last_tested_at: Option<String>,
  pub test_summary: Option<TikhubTestSummaryView>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TikhubTestSummaryView {
  pub masked_account: Option<String>,
  pub balance: Option<f64>,
  pub free_credit: Option<f64>,
  pub available_credit: Option<f64>,
  pub today_usage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiApiProfileView {
  pub kind: ApiProfileKind,
  pub id: String,
  pub name: String,
  pub provider_type: AiProviderType,
  pub api_format: AiApiFormat,
  pub base_url: String,
  pub default_model_id: String,
  pub status: ApiProfileStatus,
  pub revision: u64,
  pub masked_key: Option<String>,
  pub has_credential: bool,
  pub is_active: bool,
  pub last_tested_at: Option<String>,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestApiProfileResult {
  pub success: bool,
  pub message: String,
  pub registry: ApiProfileRegistryView,
}

#[tauri::command]
pub(super) fn get_api_profile_registry(
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::get_registry(&root).map(|registry| safe_registry_view(&registry))
}

#[tauri::command]
pub(super) fn save_api_profile(
  input: SaveApiProfileInput,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::save_profile(&root, input).map(|registry| safe_registry_view(&registry))
}

#[tauri::command]
pub(super) async fn test_api_profile(
  kind: ApiProfileKind,
  profile_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<TestApiProfileResult> {
  let root = resolve_workspace_root(root_path, &state)?;
  let result =
    tauri::async_runtime::spawn_blocking(move || service::test_profile(&root, kind, &profile_id))
      .await
      .map_err(|_| command_error("API 配置测试后台任务意外终止"))??;
  Ok(TestApiProfileResult {
    success: result.success,
    message: result.message,
    registry: safe_registry_view(&result.registry),
  })
}

#[tauri::command]
pub(super) fn activate_api_profile(
  kind: ApiProfileKind,
  profile_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::activate_profile(&root, kind, &profile_id).map(|registry| safe_registry_view(&registry))
}

#[tauri::command]
pub(super) fn delete_api_profile(
  kind: ApiProfileKind,
  profile_id: String,
  root_path: Option<String>,
  state: tauri::State<'_, AppState>,
) -> AppResult<ApiProfileRegistryView> {
  let root = resolve_workspace_root(root_path, &state)?;
  service::delete_profile(&root, kind, &profile_id).map(|registry| safe_registry_view(&registry))
}

fn safe_registry_view(registry: &ApiProfileRegistry) -> ApiProfileRegistryView {
  let mut tikhub_profiles = registry
    .tikhub_profiles
    .values()
    .map(|profile| {
      let credential = registry.credentials.get(&profile.credential_ref_id);
      TikhubApiProfileView {
        kind: ApiProfileKind::Tikhub,
        id: profile.id.clone(),
        name: profile.name.clone(),
        base_url: profile.base_url.clone(),
        status: profile.status,
        revision: profile.revision,
        masked_key: credential.map(|value| mask_secret(&value.secret)),
        has_credential: credential.is_some(),
        is_active: registry.active_profile_ids.tikhub.as_deref() == Some(profile.id.as_str()),
        last_tested_at: profile.last_tested_at.clone(),
        test_summary: profile.test_summary.as_ref().map(safe_tikhub_summary),
        created_at: profile.created_at.clone(),
        updated_at: profile.updated_at.clone(),
      }
    })
    .collect::<Vec<_>>();
  tikhub_profiles.sort_by(|left, right| {
    profile_order(&left.created_at, &left.name, &right.created_at, &right.name)
  });

  let mut ai_profiles = registry
    .ai_profiles
    .values()
    .map(|profile| {
      let credential = profile
        .credential_ref_id
        .as_ref()
        .and_then(|id| registry.credentials.get(id));
      AiApiProfileView {
        kind: ApiProfileKind::Ai,
        id: profile.id.clone(),
        name: profile.name.clone(),
        provider_type: profile.provider_type,
        api_format: profile.api_format,
        base_url: profile.base_url.clone(),
        default_model_id: profile.default_model_id.clone(),
        status: profile.status,
        revision: profile.revision,
        masked_key: credential.map(|value| mask_secret(&value.secret)),
        has_credential: credential.is_some(),
        is_active: registry.active_profile_ids.ai.as_deref() == Some(profile.id.as_str()),
        last_tested_at: profile.last_tested_at.clone(),
        created_at: profile.created_at.clone(),
        updated_at: profile.updated_at.clone(),
      }
    })
    .collect::<Vec<_>>();
  ai_profiles.sort_by(|left, right| {
    profile_order(&left.created_at, &left.name, &right.created_at, &right.name)
  });

  ApiProfileRegistryView {
    active_profile_ids: SafeActiveProfileIds {
      tikhub: registry.active_profile_ids.tikhub.clone(),
      ai: registry.active_profile_ids.ai.clone(),
    },
    tikhub_profiles,
    ai_profiles,
  }
}

fn safe_tikhub_summary(summary: &TikhubSafeTestSummary) -> TikhubTestSummaryView {
  TikhubTestSummaryView {
    masked_account: summary.masked_account.clone(),
    balance: summary.balance,
    free_credit: summary.free_credit,
    available_credit: summary.available_credit,
    today_usage: summary.today_usage,
  }
}

fn profile_order(
  left_created: &str,
  left_name: &str,
  right_created: &str,
  right_name: &str,
) -> std::cmp::Ordering {
  left_created
    .cmp(right_created)
    .then_with(|| left_name.cmp(right_name))
}

fn command_error(message: impl Into<String>) -> AppError {
  AppError::new(
    AppErrorCode::ValidationError,
    message,
    AppErrorStage::SecretStore,
    false,
  )
}

#[cfg(test)]
mod tests {
  use std::fs;

  use rusqlite::params;
  use serde_json::json;
  use uuid::Uuid;

  use super::*;
  use crate::domain::AppError;
  use crate::tikhub::TikhubConnectionTestResult;
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

  const AI_SECRET: &str = "sk-ai-safe-view-sentinel-123456789";
  const TIKHUB_SECRET: &str = "tk-safe-view-sentinel-987654321";

  fn workspace(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("api-command-{label}-{}", Uuid::new_v4()));
    create_workspace("API 命令测试", &root).unwrap();
    root
  }

  fn ai_input(id: Option<String>, name: &str, key: Option<&str>) -> SaveApiProfileInput {
    SaveApiProfileInput::Ai {
      id,
      name: name.to_string(),
      provider_type: AiProviderType::Openai,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url: "https://api.openai.com/v1".to_string(),
      default_model_id: "gpt-test".to_string(),
      api_key: key.map(str::to_string),
    }
  }

  fn successful_tikhub_test() -> TikhubConnectionTestResult {
    TikhubConnectionTestResult {
      success: true,
      base_url: "https://api.tikhub.io".to_string(),
      masked_email: Some("s***n@example.test".to_string()),
      balance: Some(4.0),
      free_credit: Some(1.0),
      available_credit: Some(5.0),
      email_verified: Some(true),
      api_key_status: Some(1),
      daily_usage_json: json!({"data":{"total_requests":12}}),
      message: "TikHub Token 可用".to_string(),
    }
  }

  #[test]
  fn ai_profile_requires_explicit_activation_after_active_profile_is_deleted() {
    let root = workspace("ai-explicit-reactivation");
    let first = service::save_profile(&root, ai_input(None, "OpenAI A", Some(AI_SECRET))).unwrap();
    let first_id = first.ai_profiles.values().next().unwrap().id.clone();
    let first_test = service::test_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
    assert_eq!(
      first_test.registry.active_profile_ids.ai.as_deref(),
      Some(first_id.as_str())
    );

    let second = service::save_profile(
      &root,
      ai_input(None, "OpenAI B", Some("sk-second-123456789")),
    )
    .unwrap();
    let second_id = second
      .ai_profiles
      .values()
      .find(|profile| profile.name == "OpenAI B")
      .unwrap()
      .id
      .clone();
    let second_test = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
    assert_eq!(
      second_test.registry.active_profile_ids.ai.as_deref(),
      Some(first_id.as_str())
    );

    let deleted = service::delete_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
    assert!(deleted.active_profile_ids.ai.is_none());
    let retested = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
    assert!(retested.registry.active_profile_ids.ai.is_none());
    let activated = service::activate_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
    assert_eq!(
      activated.active_profile_ids.ai.as_deref(),
      Some(second_id.as_str())
    );
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn tikhub_profile_requires_explicit_activation_after_active_profile_is_deleted() {
    let root = workspace("tikhub-explicit-reactivation");
    let first = service::save_profile(
      &root,
      SaveApiProfileInput::Tikhub {
        id: None,
        name: "TikHub A".to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        api_key: Some(TIKHUB_SECRET.to_string()),
      },
    )
    .unwrap();
    let first_id = first.tikhub_profiles.values().next().unwrap().id.clone();
    let first_test =
      service::test_profile_with(&root, ApiProfileKind::Tikhub, &first_id, |_, _, _| {
        Ok(successful_tikhub_test())
      })
      .unwrap();
    assert_eq!(
      first_test.registry.active_profile_ids.tikhub.as_deref(),
      Some(first_id.as_str())
    );

    let second = service::save_profile(
      &root,
      SaveApiProfileInput::Tikhub {
        id: None,
        name: "TikHub B".to_string(),
        base_url: "https://api.tikhub.dev".to_string(),
        api_key: Some("tk-second-123456789".to_string()),
      },
    )
    .unwrap();
    let second_id = second
      .tikhub_profiles
      .values()
      .find(|profile| profile.name == "TikHub B")
      .unwrap()
      .id
      .clone();
    let second_test =
      service::test_profile_with(&root, ApiProfileKind::Tikhub, &second_id, |_, _, _| {
        Ok(successful_tikhub_test())
      })
      .unwrap();
    assert_eq!(
      second_test.registry.active_profile_ids.tikhub.as_deref(),
      Some(first_id.as_str())
    );

    let deleted = service::delete_profile(&root, ApiProfileKind::Tikhub, &first_id).unwrap();
    assert!(deleted.active_profile_ids.tikhub.is_none());
    let retested =
      service::test_profile_with(&root, ApiProfileKind::Tikhub, &second_id, |_, _, _| {
        Ok(successful_tikhub_test())
      })
      .unwrap();
    assert!(retested.registry.active_profile_ids.tikhub.is_none());
    let activated = service::activate_profile(&root, ApiProfileKind::Tikhub, &second_id).unwrap();
    assert_eq!(
      activated.active_profile_ids.tikhub.as_deref(),
      Some(second_id.as_str())
    );
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn safe_views_switch_profiles_and_keep_blank_edit_keys() {
    let root = workspace("ai");
    let first = service::save_profile(&root, ai_input(None, "OpenAI A", Some(AI_SECRET))).unwrap();
    let first_id = first.ai_profiles.values().next().unwrap().id.clone();
    let view_json = serde_json::to_string(&safe_registry_view(&first)).unwrap();
    assert!(!view_json.contains(AI_SECRET));
    assert!(view_json.contains("maskedKey"));
    assert!(first.active_profile_ids.ai.is_none());

    let tested = service::test_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
    assert!(tested.success);
    assert_eq!(
      tested.registry.active_profile_ids.ai.as_deref(),
      Some(first_id.as_str())
    );

    let second = service::save_profile(
      &root,
      ai_input(None, "OpenAI B", Some("sk-second-123456789")),
    )
    .unwrap();
    let second_id = second
      .ai_profiles
      .values()
      .find(|profile| profile.name == "OpenAI B")
      .unwrap()
      .id
      .clone();
    let tested_second = service::test_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
    assert_eq!(
      tested_second.registry.active_profile_ids.ai.as_deref(),
      Some(first_id.as_str())
    );
    let activated = service::activate_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
    assert_eq!(
      activated.active_profile_ids.ai.as_deref(),
      Some(second_id.as_str())
    );

    let edited = service::save_profile(
      &root,
      ai_input(Some(second_id.clone()), "OpenAI B 编辑", Some("")),
    )
    .unwrap();
    let edited_profile = edited.ai_profiles.get(&second_id).unwrap();
    assert_eq!(edited_profile.status, ApiProfileStatus::Untested);
    assert!(edited
      .credentials
      .contains_key(edited_profile.credential_ref_id.as_ref().unwrap()));
    assert!(edited.active_profile_ids.ai.is_none());
    assert!(service::activate_profile(&root, ApiProfileKind::Ai, &second_id).is_err());

    assert!(
      service::test_profile(&root, ApiProfileKind::Ai, &second_id)
        .unwrap()
        .success
    );
    let reactivated = service::activate_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
    assert_eq!(
      reactivated.active_profile_ids.ai.as_deref(),
      Some(second_id.as_str())
    );
    let deleted_current = service::delete_profile(&root, ApiProfileKind::Ai, &second_id).unwrap();
    assert!(deleted_current.active_profile_ids.ai.is_none());

    let deleted = service::delete_profile(&root, ApiProfileKind::Ai, &first_id).unwrap();
    assert!(!deleted.ai_profiles.contains_key(&first_id));
    let audit: String = open_workspace_database(root.join(DATABASE_FILE_NAME))
      .unwrap()
      .query_row(
        "SELECT COALESCE(group_concat(safe_details_json, '\n'), '') FROM audit_log",
        [],
        |row| row.get(0),
      )
      .unwrap();
    assert!(!audit.contains(AI_SECRET));
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn tikhub_test_persists_safe_summary_and_redacts_failures() {
    let root = workspace("tikhub");
    let registry = service::save_profile(
      &root,
      SaveApiProfileInput::Tikhub {
        id: None,
        name: "TikHub 主账号".to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        api_key: Some(TIKHUB_SECRET.to_string()),
      },
    )
    .unwrap();
    let profile_id = registry.tikhub_profiles.values().next().unwrap().id.clone();
    let success =
      service::test_profile_with(&root, ApiProfileKind::Tikhub, &profile_id, |_, _, _| {
        Ok(successful_tikhub_test())
      })
      .unwrap();
    assert!(success.success);
    assert_eq!(
      success.registry.active_profile_ids.tikhub.as_deref(),
      Some(profile_id.as_str())
    );
    assert_eq!(
      success.registry.tikhub_profiles[&profile_id]
        .test_summary
        .as_ref()
        .unwrap()
        .today_usage,
      Some(12.0)
    );
    assert!(
      !serde_json::to_string(&safe_registry_view(&success.registry))
        .unwrap()
        .contains(TIKHUB_SECRET)
    );

    let failed =
      service::test_profile_with(&root, ApiProfileKind::Tikhub, &profile_id, |_, _, _| {
        Err(AppError::validation(
          format!("失败：{TIKHUB_SECRET}"),
          AppErrorStage::Collection,
        ))
      })
      .unwrap();
    assert!(!failed.success);
    assert!(!failed.message.contains(TIKHUB_SECRET));
    assert_eq!(
      failed.registry.tikhub_profiles[&profile_id].status,
      ApiProfileStatus::Failed
    );
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn rejects_sensitive_ai_urls_before_persisting_profile_data() {
    let root = workspace("ai-sensitive-url");
    let sentinel = "url-secret-sentinel-987654321";
    let input = SaveApiProfileInput::Ai {
      id: None,
      name: "不安全端点".to_string(),
      provider_type: AiProviderType::CustomOpenaiCompatible,
      api_format: AiApiFormat::OpenaiCompatible,
      base_url: format!("https://user:{sentinel}@example.test/v1?api_key={sentinel}#token"),
      default_model_id: "model-test".to_string(),
      api_key: Some(AI_SECRET.to_string()),
    };

    let error = service::save_profile(&root, input).unwrap_err();

    assert!(error.message.contains("AI Base URL"));
    let registry_json = fs::read(root.join("secrets/api-config.json")).unwrap();
    let database = fs::read(root.join(DATABASE_FILE_NAME)).unwrap();
    assert!(!registry_json
      .windows(sentinel.len())
      .any(|value| value == sentinel.as_bytes()));
    assert!(!database
      .windows(sentinel.len())
      .any(|value| value == sentinel.as_bytes()));
    fs::remove_dir_all(root).ok();
  }

  #[test]
  fn active_runtime_snapshot_blocks_tikhub_edit_and_delete() {
    let root = workspace("snapshot");
    let registry = service::save_profile(
      &root,
      SaveApiProfileInput::Tikhub {
        id: None,
        name: "快照账号".to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        api_key: Some(TIKHUB_SECRET.to_string()),
      },
    )
    .unwrap();
    let profile = registry.tikhub_profiles.values().next().unwrap();
    let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    connection
      .execute_batch("DROP TRIGGER trg_collection_runtime_snapshot_insert;")
      .unwrap();
    connection.execute(
      "INSERT INTO collection_task (id,name,source_type,status,created_at,updated_at) VALUES ('task','t','form','running',?1,?1)",
      params!["2026-07-17T00:00:00+00:00"],
    ).unwrap();
    connection.execute(
      "INSERT INTO task_run (id,task_id,status,started_at,claimed_at) VALUES ('run','task','running',?1,?1)",
      params!["2026-07-17T00:00:00+00:00"],
    ).unwrap();
    connection
      .execute(
        "INSERT INTO collection_runtime_snapshot (
         id,task_run_id,workspace_id,runtime_contract_version,plan_id,plan_schema_version,
         plan_json,connector_type,connector_id,connector_config_version,base_url,secret_ref_id,
         secret_revision,secret_provider_type,secret_provider_id,connector_tested_at,
         connector_test_status,created_at
       ) SELECT 'snapshot','run',id,1,'plan',2,'{}','tikhub','default',1,?1,?2,1,
                'tikhub',?3,?4,'success',?4 FROM workspace",
        params![
          profile.base_url,
          profile.credential_ref_id,
          profile.id,
          "2026-07-17T00:00:00+00:00"
        ],
      )
      .unwrap();
    drop(connection);

    let edit = SaveApiProfileInput::Tikhub {
      id: Some(profile.id.clone()),
      name: "禁止编辑".to_string(),
      base_url: profile.base_url.clone(),
      api_key: None,
    };
    assert!(service::save_profile(&root, edit).is_err());
    assert!(service::delete_profile(&root, ApiProfileKind::Tikhub, &profile.id).is_err());
    fs::remove_dir_all(root).ok();
  }
}
