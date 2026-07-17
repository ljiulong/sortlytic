use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Barrier, LazyLock, Mutex};
use std::thread;

use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

use super::{
  load_api_profile_registry, sync_api_profile_mirror, update_api_profile_registry, AiApiFormat,
  AiApiProfile, AiProviderType, ApiProfileRegistry, ApiProfileStatus, MIRROR_LOCK,
};
use crate::workspace::{create_workspace, DATABASE_FILE_NAME};

type MirrorHook = Arc<dyn Fn() + Send + Sync>;

static BEFORE_MIRROR_HOOKS: LazyLock<Mutex<BTreeMap<PathBuf, MirrorHook>>> =
  LazyLock::new(|| Mutex::new(BTreeMap::new()));

thread_local! {
  static REGISTRY_LOCK_HELD: Cell<bool> = const { Cell::new(false) };
}

pub(super) struct RegistryLockStateGuard;

impl Drop for RegistryLockStateGuard {
  fn drop(&mut self) {
    REGISTRY_LOCK_HELD.set(false);
  }
}

pub(super) fn mark_registry_lock_held() -> RegistryLockStateGuard {
  REGISTRY_LOCK_HELD.set(true);
  RegistryLockStateGuard
}

fn registry_lock_held_by_current_thread() -> bool {
  REGISTRY_LOCK_HELD.get()
}

struct MirrorHookGuard {
  root_path: PathBuf,
}

impl Drop for MirrorHookGuard {
  fn drop(&mut self) {
    BEFORE_MIRROR_HOOKS.lock().unwrap().remove(&self.root_path);
  }
}

pub(super) fn run_before_mirror_hook(root_path: &Path) {
  let hook = BEFORE_MIRROR_HOOKS.lock().unwrap().get(root_path).cloned();
  if let Some(hook) = hook {
    hook();
  }
}

fn install_before_mirror_hook(root_path: &Path, hook: MirrorHook) -> MirrorHookGuard {
  BEFORE_MIRROR_HOOKS
    .lock()
    .unwrap()
    .insert(root_path.to_path_buf(), hook);
  MirrorHookGuard {
    root_path: root_path.to_path_buf(),
  }
}

fn add_ollama_profile(registry: &mut ApiProfileRegistry, name: &str) {
  let profile_id = Uuid::new_v4().to_string();
  let timestamp = Utc::now().to_rfc3339();
  registry.ai_profiles.insert(
    profile_id.clone(),
    AiApiProfile {
      id: profile_id,
      name: name.to_string(),
      provider_type: AiProviderType::Ollama,
      api_format: AiApiFormat::Ollama,
      base_url: "http://127.0.0.1:11434".to_string(),
      default_model_id: "llama3.2".to_string(),
      credential_ref_id: None,
      revision: 1,
      status: ApiProfileStatus::Success,
      last_tested_at: Some(timestamp.clone()),
      created_at: timestamp.clone(),
      updated_at: timestamp,
    },
  );
}

#[test]
fn stale_registry_reader_cannot_overwrite_a_newer_sqlite_mirror() {
  let root = std::env::temp_dir().join(format!("api-mirror-race-{}", Uuid::new_v4()));
  create_workspace("API 镜像并发测试", &root).unwrap();
  update_api_profile_registry(&root, |registry| {
    add_ollama_profile(registry, "旧配置");
    Ok(())
  })
  .unwrap();
  sync_api_profile_mirror(&root).unwrap();

  let mirror_entered = Arc::new(Barrier::new(2));
  let release_mirror = Arc::new(Barrier::new(2));
  let first_call = Arc::new(Mutex::new(true));
  let mirror_held_registry_lock = Arc::new(AtomicBool::new(true));
  let _hook_guard = install_before_mirror_hook(&root, {
    let mirror_entered = Arc::clone(&mirror_entered);
    let release_mirror = Arc::clone(&release_mirror);
    let first_call = Arc::clone(&first_call);
    let mirror_held_registry_lock = Arc::clone(&mirror_held_registry_lock);
    Arc::new(move || {
      let should_pause = {
        let mut first_call = first_call.lock().unwrap();
        let should_pause = *first_call;
        *first_call = false;
        should_pause
      };
      if should_pause {
        mirror_held_registry_lock.store(registry_lock_held_by_current_thread(), Ordering::SeqCst);
        mirror_entered.wait();
        release_mirror.wait();
      }
    })
  });

  let stale_root = root.clone();
  let stale_sync = thread::spawn(move || sync_api_profile_mirror(stale_root));
  mirror_entered.wait();
  let registry_lock_available = !mirror_held_registry_lock.load(Ordering::SeqCst);
  let mirror_lock_held = MIRROR_LOCK.try_lock().is_err();

  let current_root = root.clone();
  let (registry_updated, wait_for_update) = mpsc::channel();
  let current_sync = thread::spawn(move || {
    update_api_profile_registry(&current_root, |registry| {
      add_ollama_profile(registry, "新配置");
      Ok(())
    })?;
    registry_updated.send(()).unwrap();
    sync_api_profile_mirror(&current_root)
  });

  if registry_lock_available {
    wait_for_update.recv().unwrap();
  }
  release_mirror.wait();
  if !registry_lock_available {
    wait_for_update.recv().unwrap();
  }
  stale_sync.join().unwrap().unwrap();
  current_sync.join().unwrap().unwrap();

  assert!(
    registry_lock_available,
    "SQLite 镜像写入期间不得持续持有注册表锁"
  );
  assert!(
    mirror_lock_held,
    "读取 JSON 与写入 SQLite 必须位于同一次镜像串行边界内"
  );
  let registry = load_api_profile_registry(&root).unwrap();
  let connection = Connection::open(root.join(DATABASE_FILE_NAME)).unwrap();
  let mirrored_profiles: i64 = connection
    .query_row("SELECT COUNT(*) FROM model_provider", [], |row| row.get(0))
    .unwrap();
  assert_eq!(mirrored_profiles, registry.ai_profiles.len() as i64);
  std::fs::remove_dir_all(root).ok();
}
