use std::{fs, sync::mpsc, thread, time::Duration};

use rusqlite::{params, Connection, ErrorCode, TransactionBehavior};
use uuid::Uuid;

use super::{service, ApiProfileKind, SaveApiProfileInput};
use crate::api_profiles::TikhubApiProfile;
use crate::domain::AppResult;
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

const TIKHUB_SECRET: &str = "tk-claim-race-sentinel-987654321";

thread_local! {
  static AFTER_MUTABILITY_CHECK_HOOK:
    std::cell::RefCell<Option<Box<dyn FnOnce() + Send>>> = std::cell::RefCell::new(None);
}

fn install_after_mutability_check_hook(hook: impl FnOnce() + Send + 'static) {
  AFTER_MUTABILITY_CHECK_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

pub(super) fn run_after_mutability_check_hook() {
  let hook = AFTER_MUTABILITY_CHECK_HOOK.with(|slot| slot.borrow_mut().take());
  if let Some(hook) = hook {
    hook();
  }
}

fn workspace(label: &str) -> std::path::PathBuf {
  let root = std::env::temp_dir().join(format!("api-claim-race-{label}-{}", Uuid::new_v4()));
  create_workspace("API 领取竞态测试", &root).unwrap();
  root
}

fn tikhub_input(id: Option<String>, name: &str) -> SaveApiProfileInput {
  let api_key = id.is_none().then(|| TIKHUB_SECRET.to_string());
  SaveApiProfileInput::Tikhub {
    id,
    name: name.to_string(),
    base_url: "https://api.tikhub.io".to_string(),
    api_key,
  }
}

fn fixture(label: &str) -> (std::path::PathBuf, TikhubApiProfile, Vec<u8>) {
  let root = workspace(label);
  let registry = service::save_profile(&root, tikhub_input(None, "领取竞态账号")).unwrap();
  let profile = registry.tikhub_profiles.values().next().unwrap().clone();
  let json = fs::read(root.join("secrets/api-config.json")).unwrap();
  (root, profile, json)
}

fn mutate_tikhub(
  root: &std::path::Path,
  profile: &TikhubApiProfile,
  operation: &str,
) -> AppResult<()> {
  if operation == "edit" {
    service::save_profile(root, tikhub_input(Some(profile.id.clone()), "不应写入")).map(|_| ())
  } else {
    service::delete_profile(root, ApiProfileKind::Tikhub, &profile.id).map(|_| ())
  }
}

fn insert_snapshot(connection: &Connection, profile: &TikhubApiProfile, snapshot_id: &str) {
  let now = "2026-07-17T00:00:00+00:00";
  connection
    .execute_batch("DROP TRIGGER trg_collection_runtime_snapshot_insert;")
    .unwrap();
  connection
    .execute(
      "INSERT INTO collection_task (id,name,source_type,status,created_at,updated_at)
       VALUES ('task','t','form','running',?1,?1)",
      params![now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO task_run (id,task_id,status,started_at,claimed_at,current_stage)
       VALUES ('run','task','running',?1,?1,'执行采集')",
      params![now],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO collection_runtime_snapshot (
         id,task_run_id,workspace_id,runtime_contract_version,plan_id,plan_schema_version,
         plan_json,connector_type,connector_id,connector_config_version,base_url,secret_ref_id,
         secret_revision,secret_provider_type,secret_provider_id,connector_tested_at,
         connector_test_status,created_at
       ) SELECT ?1,'run',id,1,'plan',2,'{}','tikhub','default',1,?2,?3,1,
                'tikhub',?4,?5,'success',?5 FROM workspace",
      params![
        snapshot_id,
        profile.base_url,
        profile.credential_ref_id,
        profile.id,
        now
      ],
    )
    .unwrap();
}

#[test]
fn claim_transaction_serializes_tikhub_edit_and_delete() {
  for operation in ["edit", "delete"] {
    let (root, profile, before) = fixture(&format!("claim-first-{operation}"));
    let mut connection = open_workspace_database(root.join(DATABASE_FILE_NAME)).unwrap();
    let transaction = connection
      .transaction_with_behavior(TransactionBehavior::Immediate)
      .unwrap();
    insert_snapshot(&transaction, &profile, "claim-first");

    let (started_tx, started_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let worker_root = root.clone();
    let worker_profile = profile.clone();
    thread::spawn(move || {
      started_tx.send(()).unwrap();
      result_tx
        .send(mutate_tikhub(&worker_root, &worker_profile, operation))
        .unwrap();
    });
    started_rx.recv().unwrap();
    assert!(result_rx.recv_timeout(Duration::from_millis(250)).is_err());
    assert_eq!(
      fs::read(root.join("secrets/api-config.json")).unwrap(),
      before
    );

    transaction.commit().unwrap();
    let error = result_rx
      .recv_timeout(Duration::from_secs(5))
      .unwrap()
      .unwrap_err();
    assert!(error.message.contains("任务快照引用"), "{}", error.message);
    assert_eq!(
      fs::read(root.join("secrets/api-config.json")).unwrap(),
      before
    );
    fs::remove_dir_all(root).ok();
  }
}

#[test]
fn claim_cannot_enter_between_mutability_check_and_registry_write() {
  for operation in ["edit", "delete"] {
    let (root, profile, before) = fixture(&format!("claim-gap-{operation}"));
    let (checked_tx, checked_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (mutation_tx, mutation_rx) = mpsc::channel();
    let worker_root = root.clone();
    let worker_profile = profile.clone();
    thread::spawn(move || {
      install_after_mutability_check_hook(move || {
        checked_tx.send(()).unwrap();
        release_rx.recv().unwrap();
      });
      mutation_tx
        .send(mutate_tikhub(&worker_root, &worker_profile, operation))
        .unwrap();
    });
    checked_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let mut connection = Connection::open(root.join(DATABASE_FILE_NAME)).unwrap();
    connection.busy_timeout(Duration::ZERO).unwrap();
    let transaction = match connection.transaction_with_behavior(TransactionBehavior::Immediate) {
      Ok(transaction) => Some(transaction),
      Err(error) => {
        assert_eq!(error.sqlite_error_code(), Some(ErrorCode::DatabaseBusy));
        None
      }
    };
    let interleaved = transaction.is_some();
    if let Some(transaction) = transaction.as_ref() {
      insert_snapshot(transaction, &profile, "claim-gap");
    }
    release_tx.send(()).unwrap();
    let mut changed_during_claim = false;
    for _ in 0..100 {
      changed_during_claim =
        interleaved && fs::read(root.join("secrets/api-config.json")).unwrap() != before;
      if changed_during_claim || !interleaved {
        break;
      }
      thread::sleep(Duration::from_millis(10));
    }
    if let Some(transaction) = transaction {
      transaction.commit().unwrap();
    }
    mutation_rx
      .recv_timeout(Duration::from_secs(5))
      .unwrap()
      .unwrap();

    assert!(
      !interleaved,
      "领取事务进入了可变性检查与 JSON 写入之间的窗口"
    );
    assert!(
      !changed_during_claim,
      "未提交的领取快照存在时 JSON 已被修改"
    );
    fs::remove_dir_all(root).ok();
  }
}
