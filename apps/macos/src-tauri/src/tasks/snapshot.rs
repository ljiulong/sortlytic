use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::api_profiles::{
  load_existing_api_profile_registry, ApiProfileStatus, CredentialProviderType,
};
use crate::domain::AppResult;

use super::{database_error, task_error};

struct ActiveTikhubSnapshotSource {
  profile_id: String,
  profile_revision: i64,
  base_url: String,
  credential_ref_id: String,
  credential_revision: i64,
  tested_at: String,
}

pub(super) fn create_runtime_snapshot(
  root_path: &Path,
  connection: &Connection,
  run_id: &str,
  plan_id: &str,
  created_at: &str,
  allow_create: bool,
) -> AppResult<bool> {
  let existing_plan_id = connection
    .query_row(
      "SELECT plan_id FROM collection_runtime_snapshot WHERE task_run_id = ?1",
      params![run_id],
      |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(database_error)?;
  if let Some(existing_plan_id) = existing_plan_id {
    if existing_plan_id != plan_id {
      return Err(task_error(
        "排队任务已有其他采集计划的不可变运行快照，已拒绝覆盖",
      ));
    }
    return Ok(false);
  }
  if !allow_create {
    return Err(task_error(
      "恢复队列缺少原不可变运行快照，已拒绝使用当前 TikHub API 配置补建",
    ));
  }

  let active = active_tikhub_snapshot_source(root_path)?;
  let expected_store_key = format!("api-config.json#credentials/{}", active.credential_ref_id);
  let source = connection
    .query_row(
      "SELECT workspace.id, plan.schema_version, plan.plan_json,
              connector.id, connector.config_version, connector.base_url,
              connector.secret_ref_id, connector.last_tested_at,
              connector.last_test_status, secret.provider_type, secret.provider_id,
              secret.credential_revision
       FROM task_run AS run
       JOIN collection_plan AS plan
         ON plan.id = run.plan_id AND plan.task_id = run.task_id
       JOIN workspace ON workspace.id = (
         SELECT id FROM workspace LIMIT 1
       )
       JOIN tikhub_connector AS connector
         ON connector.workspace_id = workspace.id AND connector.id = 'default'
       JOIN secret_ref AS secret ON secret.id = connector.secret_ref_id
       WHERE run.id = ?1 AND run.plan_id = ?2 AND run.status = 'queued'
         AND run.claimed_at IS NULL AND plan.schema_version >= 2
         AND plan.validation_status = 'valid' AND plan.confirmed_by_user = 1
         AND connector.id = 'default' AND connector.config_version = ?3
         AND connector.base_url = ?4 AND connector.secret_ref_id = ?5
         AND connector.enabled = 1 AND connector.last_tested_at IS NOT NULL
         AND connector.last_tested_at = ?6 AND connector.last_test_status = 'success'
         AND secret.provider_type = 'tikhub' AND secret.provider_id = ?7
         AND secret.id = ?5 AND secret.credential_revision = ?8
         AND secret.secret_store_key = ?9",
      params![
        run_id,
        plan_id,
        active.profile_revision,
        active.base_url,
        active.credential_ref_id,
        active.tested_at,
        active.profile_id,
        active.credential_revision,
        expected_store_key,
      ],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, String>(2)?,
          row.get::<_, String>(3)?,
          row.get::<_, i64>(4)?,
          row.get::<_, String>(5)?,
          row.get::<_, String>(6)?,
          row.get::<_, String>(7)?,
          row.get::<_, String>(8)?,
          row.get::<_, String>(9)?,
          row.get::<_, String>(10)?,
          row.get::<_, i64>(11)?,
        ))
      },
    )
    .optional()
    .map_err(database_error)?;
  let Some((
    workspace_id,
    plan_schema_version,
    plan_json,
    connector_id,
    connector_config_version,
    base_url,
    secret_ref_id,
    connector_tested_at,
    connector_test_status,
    secret_provider_type,
    secret_provider_id,
    secret_revision,
  )) = source
  else {
    return Err(task_error(
      "当前 TikHub API 配置与 SQLite 派生镜像不一致，任务仍保持排队；请重新保存或切换当前配置后重试",
    ));
  };

  connection
    .execute(
      "INSERT INTO collection_runtime_snapshot (
         id, task_run_id, workspace_id, runtime_contract_version,
         plan_id, plan_schema_version, plan_json, connector_type, connector_id,
         connector_config_version, base_url, secret_ref_id, secret_revision,
         secret_provider_type, secret_provider_id, connector_tested_at,
         connector_test_status, created_at
       ) VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, 'tikhub', ?7, ?8, ?9, ?10,
                 ?11, ?12, ?13, ?14, ?15, ?16)",
      params![
        Uuid::new_v4().to_string(),
        run_id,
        workspace_id,
        plan_id,
        plan_schema_version,
        plan_json,
        connector_id,
        connector_config_version,
        base_url,
        secret_ref_id,
        secret_revision,
        secret_provider_type,
        secret_provider_id,
        connector_tested_at,
        connector_test_status,
        created_at
      ],
    )
    .map_err(database_error)?;
  Ok(true)
}

fn active_tikhub_snapshot_source(root_path: &Path) -> AppResult<ActiveTikhubSnapshotSource> {
  let registry = load_existing_api_profile_registry(root_path)?
    .ok_or_else(|| task_error("API 配置文件不存在，无法为排队任务绑定当前 TikHub API 配置"))?;
  let active_profile_id = registry
    .active_profile_ids
    .tikhub
    .as_deref()
    .ok_or_else(|| task_error("尚未选择当前 TikHub API 配置，任务仍保持排队"))?;
  let profile = registry
    .tikhub_profiles
    .get(active_profile_id)
    .ok_or_else(|| task_error("当前 TikHub API 配置不存在，任务仍保持排队"))?;
  if profile.status != ApiProfileStatus::Success {
    return Err(task_error(
      "当前 TikHub API 配置尚未通过连通测试，任务仍保持排队",
    ));
  }
  let tested_at = profile
    .last_tested_at
    .clone()
    .ok_or_else(|| task_error("当前 TikHub API 配置缺少成功测试时间，任务仍保持排队"))?;
  let credential = registry
    .credentials
    .get(&profile.credential_ref_id)
    .ok_or_else(|| task_error("当前 TikHub API 配置缺少凭据，任务仍保持排队"))?;
  if credential.provider_type != CredentialProviderType::Tikhub
    || credential.profile_id != profile.id
  {
    return Err(task_error(
      "当前 TikHub API 凭据与配置身份不一致，任务仍保持排队",
    ));
  }

  Ok(ActiveTikhubSnapshotSource {
    profile_id: profile.id.clone(),
    profile_revision: checked_revision(profile.revision, "TikHub 配置")?,
    base_url: profile.base_url.clone(),
    credential_ref_id: profile.credential_ref_id.clone(),
    credential_revision: checked_revision(credential.revision, "TikHub 凭据")?,
    tested_at,
  })
}

fn checked_revision(revision: u64, label: &str) -> AppResult<i64> {
  i64::try_from(revision).map_err(|_| task_error(format!("{label}修订号超出可执行范围")))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::api_profiles::{
    api_profile_registry_path, load_api_profile_registry, save_api_profile_registry,
    sync_api_profile_mirror, ActiveApiProfileIds, ApiCredential, ApiProfileRegistry,
    ApiProfileStatus, CredentialProviderType, TikhubApiProfile,
  };
  use crate::tasks::{
    claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task, fail_task_run,
    recover_interrupted_runs, retry_task, save_collection_plan, CreateCollectionTaskInput,
    SaveCollectionPlanInput,
  };
  use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};
  use chrono::Utc;
  use serde_json::json;
  use uuid::Uuid;

  #[test]
  fn first_claim_captures_the_current_connector_and_secret_revisions() {
    let root = std::env::temp_dir().join(format!("runtime-snapshot-{}", Uuid::new_v4()));
    create_workspace("运行时快照测试", &root).expect("workspace should be created");
    let task = create_collection_task(
      &root,
      CreateCollectionTaskInput {
        name: "快照任务".to_string(),
        source_type: "form".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["item_detail".to_string()],
      },
    )
    .expect("task should be created");
    let plan = save_collection_plan(
      &root,
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: "form_generated".to_string(),
        plan_json: json!({
          "platforms": ["tiktok"],
          "data_types": ["item_detail"],
          "region": null,
          "time_range": null,
          "steps": [{
            "endpoint_key": "tiktok.item_detail",
            "platform": "tiktok",
            "data_type": "item_detail",
            "params": {"item_id": "video-1"}
          }],
          "record_limit": 1,
          "request_limit": 1,
          "budget_limit": {"currency": "USD", "amount_micros": 35000000},
          "missing_fields": [],
          "requires_user_confirmation": true
        }),
        validation_status: "valid".to_string(),
        validation_errors_json: None,
        cost_estimate_json: None,
      },
    )
    .expect("plan should be saved");
    confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should be confirmed");
    let (profile_id, secret_ref_id) = install_active_tikhub_profile(&root, 1, 1);

    let queued = enqueue_task(&root, &task.id).expect("task should be queued");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let queued_snapshot_count = connection
      .query_row(
        "SELECT COUNT(*) FROM collection_runtime_snapshot WHERE task_run_id = ?1",
        [&queued.id],
        |row| row.get::<_, i64>(0),
      )
      .expect("queued snapshot count should load");
    assert_eq!(queued_snapshot_count, 0);
    drop(connection);

    let claimed = claim_next_task(&root)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    assert_eq!(claimed.id, queued.id);
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let snapshot: (i64, String, i64, String, i64) = connection
      .query_row(
        "SELECT plan_schema_version, connector_id, connector_config_version,
                secret_ref_id, secret_revision
         FROM collection_runtime_snapshot",
        [],
        |row| {
          Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
          ))
        },
      )
      .expect("runtime snapshot should be created");
    assert_eq!(snapshot, (2, "default".to_string(), 1, secret_ref_id, 1));
    let provider_id: String = connection
      .query_row(
        "SELECT secret_provider_id FROM collection_runtime_snapshot WHERE task_run_id = ?1",
        [&queued.id],
        |row| row.get(0),
      )
      .expect("snapshot provider should load");
    assert_eq!(provider_id, profile_id);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn queued_task_binds_the_profile_that_is_active_at_claim_time() {
    let root = private_workspace("runtime-snapshot-switch");
    let task = prepare_confirmed_task(&root);
    let (first_profile_id, _) = install_active_tikhub_profile(&root, 1, 1);
    let queued = enqueue_task(&root, &task.id).expect("task should queue without a snapshot");

    let mut registry = load_api_profile_registry(&root).expect("registry should load");
    let second_profile_id = Uuid::new_v4().to_string();
    let second_credential_id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();
    registry.tikhub_profiles.insert(
      second_profile_id.clone(),
      TikhubApiProfile {
        id: second_profile_id.clone(),
        name: "备用 TikHub".to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        credential_ref_id: second_credential_id.clone(),
        revision: 4,
        status: ApiProfileStatus::Success,
        last_tested_at: Some(now.clone()),
        test_summary: None,
        created_at: now.clone(),
        updated_at: now,
      },
    );
    registry.credentials.insert(
      second_credential_id.clone(),
      ApiCredential {
        id: second_credential_id.clone(),
        provider_type: CredentialProviderType::Tikhub,
        profile_id: second_profile_id.clone(),
        revision: 3,
        secret: "tk-test-second-profile".to_string(),
      },
    );
    registry.active_profile_ids.tikhub = Some(second_profile_id.clone());
    save_api_profile_registry(&root, &registry).expect("switched registry should save");
    sync_api_profile_mirror(&root).expect("switched registry should mirror");

    claim_next_task(&root)
      .expect("claim should succeed")
      .expect("task should be claimed");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let snapshot: (String, String, i64, i64) = connection
      .query_row(
        "SELECT secret_provider_id, secret_ref_id, connector_config_version, secret_revision
         FROM collection_runtime_snapshot WHERE task_run_id = ?1",
        [&queued.id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
      )
      .expect("runtime snapshot should load");
    assert_ne!(snapshot.0, first_profile_id);
    assert_eq!(snapshot, (second_profile_id, second_credential_id, 4, 3));
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn ordinary_retry_waits_until_claim_to_create_its_snapshot() {
    let root = private_workspace("runtime-snapshot-retry");
    let task = prepare_confirmed_task(&root);
    install_active_tikhub_profile(&root, 1, 1);
    enqueue_task(&root, &task.id).expect("first run should queue");
    let first_run = claim_next_task(&root)
      .expect("first claim should succeed")
      .expect("first run should be claimed");
    fail_task_run(
      &root,
      &first_run.id,
      "TIKHUB_REQUEST_ERROR",
      "test failure",
      true,
    )
    .expect("first run should fail retryably");

    let retry = retry_task(&root, &task.id, None).expect("retry should queue");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    assert_eq!(snapshot_count(&connection, &retry.id), 0);
    drop(connection);

    claim_next_task(&root)
      .expect("retry claim should succeed")
      .expect("retry should be claimed");
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    assert_eq!(snapshot_count(&connection, &retry.id), 1);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn claim_fails_closed_when_registry_is_missing_corrupt_or_has_no_active_profile() {
    for state in ["missing", "corrupt", "no-active", "failed-active"] {
      let root = private_workspace(&format!("runtime-snapshot-{state}"));
      let task = prepare_confirmed_task(&root);
      if state == "failed-active" {
        install_active_tikhub_profile(&root, 1, 1);
      }
      let queued = enqueue_task(&root, &task.id).expect("task should queue");
      let registry_path = api_profile_registry_path(&root);
      match state {
        "missing" => std::fs::remove_file(registry_path).expect("registry should be removed"),
        "corrupt" => {
          std::fs::write(registry_path, b"{not-json").expect("registry should be corrupted")
        }
        "failed-active" => {
          let mut registry = load_api_profile_registry(&root).expect("registry should load");
          let active_id = registry
            .active_profile_ids
            .tikhub
            .as_ref()
            .expect("active profile should exist")
            .clone();
          registry
            .tikhub_profiles
            .get_mut(&active_id)
            .expect("active profile should load")
            .status = ApiProfileStatus::Failed;
          std::fs::write(
            registry_path,
            serde_json::to_vec_pretty(&registry).expect("registry should serialize"),
          )
          .expect("invalid active profile should be written for the test");
        }
        _ => {}
      }

      let error = claim_next_task(&root).expect_err("claim must fail closed");
      assert!(
        error.message.contains(match state {
          "missing" => "不存在",
          "corrupt" => "已损坏",
          "failed-active" => "尚未通过验证",
          _ => "尚未选择",
        }),
        "unexpected error for {state}: {}",
        error.message
      );
      assert_queued_without_snapshot(&root, &queued.id);
      std::fs::remove_dir_all(root).ok();
    }
  }

  #[test]
  fn claim_rejects_a_stale_sqlite_profile_mirror_without_mutating_the_queue() {
    let root = private_workspace("runtime-snapshot-stale-mirror");
    let task = prepare_confirmed_task(&root);
    let (profile_id, _) = install_active_tikhub_profile(&root, 1, 1);
    let queued = enqueue_task(&root, &task.id).expect("task should queue");

    let mut registry = load_api_profile_registry(&root).expect("registry should load");
    let profile = registry
      .tikhub_profiles
      .get_mut(&profile_id)
      .expect("active profile should exist");
    profile.revision = 2;
    profile.updated_at = Utc::now().to_rfc3339();
    save_api_profile_registry(&root, &registry).expect("registry should save without mirroring");

    let error = claim_next_task(&root).expect_err("stale mirror must reject the claim");
    assert!(error.message.contains("派生镜像不一致"));
    assert_queued_without_snapshot(&root, &queued.id);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn recovery_reuses_its_snapshot_without_consulting_the_current_profile() {
    let root = private_workspace("runtime-snapshot-recovery-reuse");
    let task = prepare_confirmed_task(&root);
    install_active_tikhub_profile(&root, 1, 1);
    enqueue_task(&root, &task.id).expect("task should queue");
    let first_claim = claim_next_task(&root)
      .expect("first claim should succeed")
      .expect("task should be claimed");
    assert_eq!(
      recover_interrupted_runs(&root).expect("run should be requeued for recovery"),
      1
    );
    let mut registry = load_api_profile_registry(&root).expect("registry should load");
    registry.active_profile_ids.tikhub = None;
    save_api_profile_registry(&root, &registry).expect("registry should save without active");
    sync_api_profile_mirror(&root).expect("inactive registry should mirror");

    let recovered = claim_next_task(&root)
      .expect("existing snapshot should not require a current profile")
      .expect("recovery should be claimed");
    assert_eq!(recovered.id, first_claim.id);
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    assert_eq!(snapshot_count(&connection, &recovered.id), 1);
    std::fs::remove_dir_all(root).ok();
  }

  #[test]
  fn recovery_without_a_snapshot_never_binds_the_new_current_profile() {
    let root = private_workspace("runtime-snapshot-recovery-missing");
    let task = prepare_confirmed_task(&root);
    install_active_tikhub_profile(&root, 1, 1);
    let queued = enqueue_task(&root, &task.id).expect("task should queue");
    open_workspace_database(root.join(DATABASE_FILE_NAME))
      .expect("database should open")
      .execute(
        "UPDATE task_run SET current_stage = '恢复待发送' WHERE id = ?1",
        [&queued.id],
      )
      .expect("recovery stage should be forged");

    let error = claim_next_task(&root).expect_err("missing recovery snapshot must fail closed");
    assert!(error.message.contains("恢复队列缺少原不可变运行快照"));
    assert_queued_without_snapshot(&root, &queued.id);
    std::fs::remove_dir_all(root).ok();
  }

  fn install_active_tikhub_profile(
    root: &std::path::Path,
    profile_revision: u64,
    credential_revision: u64,
  ) -> (String, String) {
    let now = Utc::now().to_rfc3339();
    let profile_id = Uuid::new_v4().to_string();
    let secret_ref_id = Uuid::new_v4().to_string();
    let mut registry = ApiProfileRegistry {
      active_profile_ids: ActiveApiProfileIds {
        tikhub: Some(profile_id.clone()),
        ai: None,
      },
      ..ApiProfileRegistry::default()
    };
    registry.tikhub_profiles.insert(
      profile_id.clone(),
      TikhubApiProfile {
        id: profile_id.clone(),
        name: "主 TikHub".to_string(),
        base_url: "https://api.tikhub.dev".to_string(),
        credential_ref_id: secret_ref_id.clone(),
        revision: profile_revision,
        status: ApiProfileStatus::Success,
        last_tested_at: Some(now.clone()),
        test_summary: None,
        created_at: now.clone(),
        updated_at: now,
      },
    );
    registry.credentials.insert(
      secret_ref_id.clone(),
      ApiCredential {
        id: secret_ref_id.clone(),
        provider_type: CredentialProviderType::Tikhub,
        profile_id: profile_id.clone(),
        revision: credential_revision,
        secret: "tk-test-runtime-snapshot".to_string(),
      },
    );
    save_api_profile_registry(root, &registry).expect("registry should save");
    sync_api_profile_mirror(root).expect("registry mirror should sync");
    (profile_id, secret_ref_id)
  }

  fn private_workspace(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("{label}-{}", Uuid::new_v4()));
    create_workspace("运行时快照测试", &root).expect("workspace should be created");
    root
  }

  fn prepare_confirmed_task(root: &std::path::Path) -> crate::tasks::CollectionTaskView {
    let task = create_collection_task(
      root,
      CreateCollectionTaskInput {
        name: "快照任务".to_string(),
        source_type: "form".to_string(),
        platforms: vec!["tiktok".to_string()],
        data_types: vec!["item_detail".to_string()],
      },
    )
    .expect("task should be created");
    let plan = save_collection_plan(
      root,
      SaveCollectionPlanInput {
        task_id: task.id.clone(),
        source: "form_generated".to_string(),
        plan_json: json!({
          "platforms": ["tiktok"],
          "data_types": ["item_detail"],
          "region": null,
          "time_range": null,
          "steps": [{
            "endpoint_key": "tiktok.item_detail",
            "platform": "tiktok",
            "data_type": "item_detail",
            "params": {"item_id": "video-1"}
          }],
          "record_limit": 1,
          "request_limit": 1,
          "budget_limit": {"currency": "USD", "amount_micros": 35000000},
          "missing_fields": [],
          "requires_user_confirmation": true
        }),
        validation_status: "valid".to_string(),
        validation_errors_json: None,
        cost_estimate_json: None,
      },
    )
    .expect("plan should be saved");
    confirm_collection_plan(root, &task.id, &plan.id).expect("plan should be confirmed");
    task
  }

  fn snapshot_count(connection: &Connection, run_id: &str) -> i64 {
    connection
      .query_row(
        "SELECT COUNT(*) FROM collection_runtime_snapshot WHERE task_run_id = ?1",
        [run_id],
        |row| row.get(0),
      )
      .expect("snapshot count should load")
  }

  fn assert_queued_without_snapshot(root: &std::path::Path, run_id: &str) {
    let connection =
      open_workspace_database(root.join(DATABASE_FILE_NAME)).expect("database should open");
    let state: (String, Option<String>, String) = connection
      .query_row(
        "SELECT run.status, run.claimed_at, task.status
         FROM task_run AS run
         JOIN collection_task AS task ON task.id = run.task_id
         WHERE run.id = ?1",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
      )
      .expect("run state should load");
    assert_eq!(state, ("queued".to_string(), None, "queued".to_string()));
    assert_eq!(snapshot_count(&connection, run_id), 0);
  }
}
