use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use rusqlite::params;
use uuid::Uuid;

use super::{create_workspace, open_workspace, open_workspace_database, WORKSPACE_DIRS};

const PRIVATE_DIRECTORY_MODE: u32 = 0o700;
const PRIVATE_FILE_MODE: u32 = 0o600;

#[test]
fn new_workspace_uses_private_permissions() {
  let root_path = unique_temp_workspace("new-private-permissions");
  let workspace = create_workspace("权限测试", &root_path).expect("workspace should be created");

  assert_workspace_permissions(&root_path);
  assert_mode(&workspace.database_path, PRIVATE_FILE_MODE);

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn opening_workspace_repairs_existing_permissions() {
  let root_path = unique_temp_workspace("repair-private-permissions");
  let workspace =
    create_workspace("权限修复测试", &root_path).expect("workspace should be created");
  let connection = open_workspace_database(&workspace.database_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO audit_log (
        id, entity_type, action, safe_details_json, created_at
      ) VALUES (?1, 'workspace', 'permission_repair_fixture', '{}', ?2)",
      params![Uuid::new_v4().to_string(), "2026-07-13T00:00:00Z"],
    )
    .expect("write should keep valid SQLite sidecars alive");

  for directory in workspace_directory_paths(&root_path) {
    set_mode(&directory, 0o777);
  }
  set_mode(&root_path, 0o777);
  set_mode(&workspace.database_path, 0o666);
  for sidecar in database_sidecar_paths(&workspace.database_path) {
    assert!(sidecar.is_file(), "{} should exist", sidecar.display());
    set_mode(&sidecar, 0o666);
  }

  open_workspace(&root_path).expect("trusted workspace permissions should be repaired");

  assert_workspace_permissions(&root_path);
  assert_mode(&workspace.database_path, PRIVATE_FILE_MODE);
  for sidecar in database_sidecar_paths(&workspace.database_path) {
    assert_mode(&sidecar, PRIVATE_FILE_MODE);
  }

  drop(connection);
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn invalid_workspace_identity_does_not_change_permissions() {
  let root_path = unique_temp_workspace("invalid-identity-permissions-?#%");
  let other_root = unique_temp_workspace("invalid-identity-other-root");
  let workspace =
    create_workspace("身份顺序测试", &root_path).expect("workspace should be created");
  fs::create_dir_all(&other_root).expect("other root should exist");
  let connection = open_workspace_database(&workspace.database_path).expect("database should open");
  connection
    .execute(
      "UPDATE workspace SET root_path = ?1",
      params![other_root.to_string_lossy()],
    )
    .expect("fixture should corrupt the registered root");
  connection
    .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
      Ok((
        row.get::<_, i64>(0)?,
        row.get::<_, i64>(1)?,
        row.get::<_, i64>(2)?,
      ))
    })
    .expect("identity fixture should be checkpointed");
  drop(connection);
  for sidecar in database_sidecar_paths(&workspace.database_path) {
    fs::remove_file(sidecar).ok();
  }
  set_mode(&root_path, 0o777);
  set_mode(&workspace.database_path, 0o666);
  let sidecars = database_sidecar_paths(&workspace.database_path);
  assert!(sidecars.iter().all(|path| !path.exists()));

  open_workspace(&root_path).expect_err("invalid identity must fail before permission repair");

  assert_mode(&root_path, 0o777);
  assert_mode(&workspace.database_path, 0o666);
  assert!(sidecars.iter().all(|path| !path.exists()));

  fs::remove_dir_all(root_path).ok();
  fs::remove_dir_all(other_root).ok();
}

#[test]
fn opening_rejects_an_unpaired_wal_without_creating_shm() {
  let root_path = unique_temp_workspace("unpaired-sidecar");
  let workspace =
    create_workspace("附属文件配对测试", &root_path).expect("workspace should be created");
  let [wal_path, shm_path] = database_sidecar_paths(&workspace.database_path);
  let wal_bytes = b"unpaired-wal-must-remain-untouched";
  fs::write(&wal_path, wal_bytes).expect("unpaired WAL fixture should be created");
  set_mode(&wal_path, PRIVATE_FILE_MODE);
  assert!(!shm_path.exists());

  let error = open_workspace(&root_path).expect_err("unpaired sidecars must fail closed");

  assert!(error.message.contains("必须成对"));
  assert_eq!(
    fs::read(&wal_path).expect("WAL fixture should remain readable"),
    wal_bytes
  );
  assert_mode(&wal_path, PRIVATE_FILE_MODE);
  assert!(!shm_path.exists());

  fs::remove_file(wal_path).ok();
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn identity_probe_reads_uncheckpointed_wal_changes() {
  let root_path = unique_temp_workspace("wal-aware-identity");
  let other_root = unique_temp_workspace("wal-aware-other-root");
  let workspace =
    create_workspace("WAL 身份测试", &root_path).expect("workspace should be created");
  fs::create_dir_all(&other_root).expect("other root should exist");
  let connection = open_workspace_database(&workspace.database_path).expect("database should open");
  connection
    .execute(
      "UPDATE workspace SET root_path = ?1",
      params![other_root.to_string_lossy()],
    )
    .expect("uncheckpointed WAL should contain the invalid identity");
  assert!(database_sidecar_paths(&workspace.database_path)
    .iter()
    .all(|path| path.is_file()));

  let error = open_workspace(&root_path).expect_err("WAL identity change must be observed");

  assert!(error.message.contains("登记") && error.message.contains("不一致"));

  drop(connection);
  fs::remove_dir_all(root_path).ok();
  fs::remove_dir_all(other_root).ok();
}

#[test]
fn opening_does_not_broaden_a_restricted_directory() {
  let root_path = unique_temp_workspace("restricted-directory");
  create_workspace("目录权限边界测试", &root_path).expect("workspace should be created");
  let raw_path = root_path.join("raw");
  set_mode(&raw_path, 0o500);

  let error = open_workspace(&root_path).expect_err("owner write permission must not be added");

  assert!(error.message.contains("拒绝自动放宽"));
  assert_mode(&raw_path, 0o500);

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn opening_does_not_broaden_a_read_only_database() {
  let root_path = unique_temp_workspace("read-only-database");
  let workspace =
    create_workspace("数据库权限边界测试", &root_path).expect("workspace should be created");
  set_mode(&workspace.database_path, 0o400);

  let error = open_workspace(&root_path).expect_err("owner write permission must not be added");

  assert!(error.message.contains("拒绝自动放宽"));
  assert_mode(&workspace.database_path, 0o400);

  fs::remove_dir_all(root_path).ok();
}

#[test]
fn opening_rejects_symlinked_database_sidecar_without_touching_target() {
  let root_path = unique_temp_workspace("symlinked-sidecar");
  let workspace =
    create_workspace("附属文件测试", &root_path).expect("workspace should be created");
  let outside_path = unique_temp_workspace("sidecar-target");
  fs::write(&outside_path, b"outside-sidecar-target").expect("outside target should be created");
  set_mode(&outside_path, 0o666);
  let before = fs::read(&outside_path).expect("outside target should be readable");
  let before_mode = file_mode(&outside_path);
  let [wal_path, _] = database_sidecar_paths(&workspace.database_path);
  fs::remove_file(&wal_path).ok();
  symlink(&outside_path, &wal_path).expect("malicious sidecar symlink should be created");

  let error = open_workspace(&root_path).expect_err("sidecar symlink must be rejected");

  assert!(error.message.contains("附属文件") || error.message.contains("符号链接"));
  assert_eq!(
    fs::read(&outside_path).expect("outside target should remain readable"),
    before
  );
  assert_eq!(file_mode(&outside_path), before_mode);

  fs::remove_file(wal_path).ok();
  fs::remove_file(outside_path).ok();
  fs::remove_dir_all(root_path).ok();
}

#[test]
fn live_sqlite_sidecars_remain_private() {
  let root_path = unique_temp_workspace("live-private-sidecars");
  let workspace =
    create_workspace("附属权限测试", &root_path).expect("workspace should be created");
  let connection = open_workspace_database(&workspace.database_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO audit_log (
        id, entity_type, action, safe_details_json, created_at
      ) VALUES (?1, 'workspace', 'permission_test', '{}', ?2)",
      params![Uuid::new_v4().to_string(), "2026-07-13T00:00:00Z"],
    )
    .expect("write should create live SQLite sidecars");

  assert_mode(&workspace.database_path, PRIVATE_FILE_MODE);
  for sidecar in database_sidecar_paths(&workspace.database_path) {
    assert!(sidecar.is_file(), "{} should exist", sidecar.display());
    assert_mode(&sidecar, PRIVATE_FILE_MODE);
  }

  drop(connection);
  fs::remove_dir_all(root_path).ok();
}

fn assert_workspace_permissions(root_path: &Path) {
  assert_mode(root_path, PRIVATE_DIRECTORY_MODE);
  for directory in workspace_directory_paths(root_path) {
    assert_mode(&directory, PRIVATE_DIRECTORY_MODE);
  }
}

fn workspace_directory_paths(root_path: &Path) -> Vec<PathBuf> {
  let mut paths = BTreeSet::new();
  for directory in WORKSPACE_DIRS {
    let mut path = root_path.to_path_buf();
    for component in directory.split('/') {
      path.push(component);
      paths.insert(path.clone());
    }
  }
  paths.into_iter().collect()
}

fn database_sidecar_paths(database_path: &Path) -> [PathBuf; 2] {
  ["-wal", "-shm"].map(|suffix| {
    let mut path = database_path.as_os_str().to_os_string();
    path.push(suffix);
    PathBuf::from(path)
  })
}

fn assert_mode(path: &Path, expected: u32) {
  assert_eq!(
    file_mode(path),
    expected,
    "{} should have mode {expected:o}",
    path.display()
  );
}

fn file_mode(path: &Path) -> u32 {
  fs::symlink_metadata(path)
    .unwrap_or_else(|error| panic!("{} metadata should load: {error}", path.display()))
    .permissions()
    .mode()
    & 0o7777
}

fn set_mode(path: &Path, mode: u32) {
  fs::set_permissions(path, fs::Permissions::from_mode(mode))
    .unwrap_or_else(|error| panic!("{} mode should change: {error}", path.display()));
}

fn unique_temp_workspace(label: &str) -> PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
