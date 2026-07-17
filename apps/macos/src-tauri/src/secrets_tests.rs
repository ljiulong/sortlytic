use std::fs;
use std::path::Path;

use super::*;
use crate::api_profiles::api_profile_registry_path;
use crate::workspace::create_workspace;

#[test]
fn masks_secret_without_returning_full_value() {
  let secret = "sk-1234567890abcdef";
  let masked = mask_secret(secret);

  assert_ne!(masked, secret);
  assert!(masked.starts_with("sk-1"));
  assert!(masked.ends_with("cdef"));
  assert!(!masked.contains("567890ab"));
  assert_eq!(mask_secret("short"), "[REDACTED]");
}

#[test]
fn secret_crud_uses_json_as_the_only_plaintext_location() {
  let root = test_workspace("json-crud");
  let original = "tikhub-json-only-sentinel-7193";
  let replacement = "tikhub-json-only-replacement-4821";

  let saved = save_secret(&root, "tikhub", "default", original, Some("主账号".into()))
    .expect("secret should save to JSON");
  assert!(!serde_json::to_string(&saved).unwrap().contains(original));
  assert_eq!(
    read_secret_for_backend(&root, &saved.id, "tikhub").unwrap(),
    original
  );
  assert!(fs::read_to_string(api_profile_registry_path(&root))
    .unwrap()
    .contains(original));
  assert!(!sqlite_safe_text(&root).contains(original));

  let updated = update_secret(&root, &saved.id, replacement).expect("secret should update");
  assert!(!serde_json::to_string(&updated)
    .unwrap()
    .contains(replacement));
  let json = fs::read_to_string(api_profile_registry_path(&root)).unwrap();
  assert!(!json.contains(original));
  assert!(json.contains(replacement));
  assert_eq!(
    read_secret_for_backend(&root, &saved.id, "tikhub").unwrap(),
    replacement
  );
  assert!(!sqlite_safe_text(&root).contains(replacement));

  assert!(delete_secret(&root, &saved.id).unwrap());
  assert!(!fs::read_to_string(api_profile_registry_path(&root))
    .unwrap()
    .contains(replacement));
  assert!(read_secret_for_backend(&root, &saved.id, "tikhub").is_err());
  fs::remove_dir_all(root).ok();
}

#[test]
fn snapshot_secret_reads_require_exact_profile_and_revision() {
  let root = test_workspace("snapshot-revision");
  let original = "snapshot-original-secret-7193";
  let replacement = "snapshot-replacement-secret-4821";
  let saved = save_secret(
    &root,
    "tikhub",
    "default",
    original,
    Some("快照账号".into()),
  )
  .unwrap();

  assert_eq!(
    read_secret_for_snapshot(&root, &saved.id, "tikhub", &saved.provider_id, 1).unwrap(),
    original
  );
  read_secret_for_snapshot(&root, &saved.id, "tikhub", "default", 1)
    .expect_err("没有活动数据库快照时不得仅凭旧别名读取密钥");

  update_secret(&root, &saved.id, replacement).unwrap();
  for error in [
    read_secret_for_snapshot(&root, &saved.id, "tikhub", &saved.provider_id, 1)
      .expect_err("旧快照修订号不得读取新密钥"),
    read_secret_for_snapshot(&root, &saved.id, "tikhub", "another-profile", 2)
      .expect_err("快照配置身份不匹配时必须拒绝"),
    read_secret_for_snapshot(&root, &saved.id, "model_provider", &saved.provider_id, 2)
      .expect_err("快照供应商类型不匹配时必须拒绝"),
  ] {
    assert!(!error.message.contains(original));
    assert!(!error.message.contains(replacement));
  }
  assert_eq!(
    read_secret_for_snapshot(&root, &saved.id, "tikhub", &saved.provider_id, 2).unwrap(),
    replacement
  );
  fs::remove_dir_all(root).ok();
}

#[test]
fn legacy_tikhub_snapshot_alias_requires_an_exact_active_database_binding() {
  let root = test_workspace("legacy-snapshot-alias");
  let secret = "legacy-snapshot-secret-7193";
  let saved = save_secret(
    &root,
    "tikhub",
    "default",
    secret,
    Some("迁移后账号".into()),
  )
  .unwrap();
  for (label, legacy_provider_id, status) in [
    ("active-queued", "provider-legacy-queued", "queued"),
    ("active-running", "provider-legacy-custom", "running"),
  ] {
    insert_runtime_snapshot(&root, label, legacy_provider_id, &saved.id, 1, status);
    assert_eq!(
      read_secret_for_snapshot(&root, &saved.id, "tikhub", legacy_provider_id, 1)
        .expect("活动旧 TikHub 快照应继续读取绑定的当前 JSON 凭据"),
      secret
    );
  }

  let forged_alias =
    read_secret_for_snapshot(&root, &saved.id, "tikhub", "provider-legacy-forged", 1)
      .expect_err("没有完全匹配快照行的旧别名必须拒绝");
  assert_eq!(forged_alias.code, AppErrorCode::PermissionError);

  insert_runtime_snapshot(
    &root,
    "wrong-ref",
    "provider-legacy-wrong-ref",
    "foreign-secret-ref",
    1,
    "running",
  );
  let wrong_ref =
    read_secret_for_snapshot(&root, &saved.id, "tikhub", "provider-legacy-wrong-ref", 1)
      .expect_err("旧别名快照的 secret_ref_id 不一致时必须拒绝");
  assert_eq!(wrong_ref.code, AppErrorCode::PermissionError);

  insert_runtime_snapshot(
    &root,
    "wrong-revision",
    "provider-legacy-wrong-revision",
    &saved.id,
    2,
    "running",
  );
  let wrong_revision = read_secret_for_snapshot(
    &root,
    &saved.id,
    "tikhub",
    "provider-legacy-wrong-revision",
    1,
  )
  .expect_err("旧别名快照的 secret_revision 不一致时必须拒绝");
  assert_eq!(wrong_revision.code, AppErrorCode::PermissionError);

  let legacy_uuid = Uuid::new_v4().to_string();
  insert_runtime_snapshot(&root, "active-uuid", &legacy_uuid, &saved.id, 1, "queued");
  assert_eq!(
    read_secret_for_snapshot(&root, &saved.id, "tikhub", &legacy_uuid, 1)
      .expect("活动旧快照中格式为 UUID 的 provider_id 也应按完整绑定兼容"),
    secret
  );

  let forged_uuid = Uuid::new_v4().to_string();
  let forged_uuid_error = read_secret_for_snapshot(&root, &saved.id, "tikhub", &forged_uuid, 1)
    .expect_err("没有匹配数据库快照的 UUID provider_id 必须拒绝");
  assert_eq!(forged_uuid_error.code, AppErrorCode::PermissionError);

  insert_runtime_snapshot(
    &root,
    "inactive-legacy",
    "provider-legacy-inactive",
    &saved.id,
    1,
    "success",
  );
  let inactive =
    read_secret_for_snapshot(&root, &saved.id, "tikhub", "provider-legacy-inactive", 1)
      .expect_err("非 queued/running 的历史快照不得授权读取密钥");
  assert_eq!(inactive.code, AppErrorCode::PermissionError);
  for error in [
    forged_alias,
    wrong_ref,
    wrong_revision,
    forged_uuid_error,
    inactive,
  ] {
    assert!(!error.message.contains(secret));
  }
  fs::remove_dir_all(root).ok();
}

#[test]
fn json_credentials_remain_isolated_by_workspace_scope() {
  let first = test_workspace("scope-a");
  let second = test_workspace("scope-b");
  let saved = save_secret(&first, "tikhub", "default", "first-workspace-secret", None).unwrap();

  assert!(read_secret_for_backend(&second, &saved.id, "tikhub").is_err());
  assert_eq!(
    read_secret_for_backend(&first, &saved.id, "tikhub").unwrap(),
    "first-workspace-secret"
  );
  fs::remove_dir_all(first).ok();
  fs::remove_dir_all(second).ok();
}

#[test]
fn legacy_reference_can_be_rebound_without_reading_its_old_store_key() {
  let root = test_workspace("legacy-rebind");
  fs::remove_file(api_profile_registry_path(&root)).unwrap();
  let secret_ref_id = Uuid::new_v4().to_string();
  insert_legacy_tikhub(&root, &secret_ref_id, "legacy-system-account");

  let error = read_secret_for_backend(&root, &secret_ref_id, "tikhub").unwrap_err();
  assert!(error.message.contains("重新输入"));
  update_secret(&root, &secret_ref_id, "replacement-json-secret").unwrap();
  assert_eq!(
    read_secret_for_backend(&root, &secret_ref_id, "tikhub").unwrap(),
    "replacement-json-secret"
  );
  fs::remove_dir_all(root).ok();
}

#[test]
fn workspace_scope_rejects_a_database_registered_for_another_root() {
  let root = test_workspace("scope-root");
  let other = std::env::temp_dir().join(format!("secret-other-{}", Uuid::new_v4()));
  fs::create_dir_all(&other).unwrap();
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  connection
    .execute(
      "UPDATE workspace SET root_path = ?1",
      params![other.to_string_lossy()],
    )
    .unwrap();

  let error = validate_workspace_scope(&root).unwrap_err();
  assert_eq!(error.code, AppErrorCode::PermissionError);
  fs::remove_dir_all(root).ok();
  fs::remove_dir_all(other).ok();
}

#[test]
fn runtime_and_manifest_do_not_reference_system_credential_libraries() {
  let source = include_str!("secrets.rs");
  let manifest = include_str!("../Cargo.toml");
  let forbidden = [
    ["key", "ring::"].concat(),
    ["apple_native_", "keyring_store"].concat(),
    ["keyring_", "core"].concat(),
    ["KEYCHAIN_", "SERVICE"].concat(),
  ];

  for value in forbidden {
    assert!(
      !source.contains(&value),
      "found forbidden runtime dependency"
    );
    assert!(
      !manifest.contains(&value),
      "found forbidden manifest dependency"
    );
  }
}

fn test_workspace(label: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("secret-{label}-{}", Uuid::new_v4()));
  create_workspace("密钥测试", &root).unwrap();
  root
}

fn insert_legacy_tikhub(root: &Path, secret_ref_id: &str, store_key: &str) {
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  let workspace_id: String = connection
    .query_row("SELECT id FROM workspace", [], |row| row.get(0))
    .unwrap();
  let now = Utc::now().to_rfc3339();
  connection
    .execute(
      "INSERT INTO secret_ref (
         id, provider_type, provider_id, alias, secret_store_key, masked_hint,
         created_at, updated_at, credential_revision
       ) VALUES (?1, 'tikhub', 'default', '旧 TikHub', ?2, '[REDACTED]', ?3, ?3, 1)",
      params![secret_ref_id, store_key, now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO tikhub_connector (
         id, workspace_id, secret_ref_id, base_url, enabled, config_version,
         last_tested_at, last_test_status, created_at, updated_at
       ) VALUES ('default', ?1, ?2, 'https://api.tikhub.io', 1, 1,
                 ?3, 'success', ?3, ?3)",
      params![workspace_id, secret_ref_id, now],
    )
    .unwrap();
}

fn insert_runtime_snapshot(
  root: &Path,
  label: &str,
  provider_id: &str,
  secret_ref_id: &str,
  secret_revision: u64,
  status: &str,
) {
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  let now = Utc::now().to_rfc3339();
  let secret_revision = i64::try_from(secret_revision).unwrap();
  connection
    .execute_batch("DROP TRIGGER IF EXISTS trg_collection_runtime_snapshot_insert;")
    .unwrap();
  connection
    .execute(
      "INSERT INTO collection_task (id,name,source_type,status,created_at,updated_at)
       VALUES (?1,?2,'form',?3,?4,?4)",
      params![format!("task-{label}"), label, status, now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO task_run (id,task_id,status,started_at,claimed_at,current_stage)
       VALUES (?1,?2,?3,?4,?4,'恢复待发送')",
      params![format!("run-{label}"), format!("task-{label}"), status, now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO collection_runtime_snapshot (
         id,task_run_id,workspace_id,runtime_contract_version,plan_id,plan_schema_version,
         plan_json,connector_type,connector_id,connector_config_version,base_url,secret_ref_id,
         secret_revision,secret_provider_type,secret_provider_id,connector_tested_at,
         connector_test_status,created_at
       ) SELECT ?1,?2,id,1,?3,2,'{}','tikhub','default',1,'https://api.tikhub.io',?4,?5,
                'tikhub',?6,?7,'success',?7 FROM workspace",
      params![
        format!("snapshot-{label}"),
        format!("run-{label}"),
        format!("plan-{label}"),
        secret_ref_id,
        secret_revision,
        provider_id,
        now,
      ],
    )
    .unwrap();
}

fn sqlite_safe_text(root: &Path) -> String {
  let connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
  let refs: String = connection
    .query_row(
      "SELECT COALESCE(group_concat(
         provider_type || '|' || provider_id || '|' || secret_store_key || '|' || masked_hint,
         '\n'
       ), '') FROM secret_ref",
      [],
      |row| row.get(0),
    )
    .unwrap();
  let audit: String = connection
    .query_row(
      "SELECT COALESCE(group_concat(safe_details_json, '\n'), '') FROM audit_log",
      [],
      |row| row.get(0),
    )
    .unwrap();
  format!("{refs}\n{audit}")
}
