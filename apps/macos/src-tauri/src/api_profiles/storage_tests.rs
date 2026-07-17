use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use super::storage::{install_write_failure, WriteFailurePoint};
use super::{
  api_profile_registry_path, load_api_profile_registry, save_api_profile_registry, ApiCredential,
  ApiProfileRegistry, ApiProfileStatus, CredentialProviderType, TikhubApiProfile,
};

const ATTEMPTED_SECRET: &str = "atomic-write-secret-sentinel-6482";

#[test]
fn interrupted_atomic_replacement_preserves_the_previous_registry() {
  for failure in [
    WriteFailurePoint::TempPermissions,
    WriteFailurePoint::TempWrite,
    WriteFailurePoint::TempSync,
    WriteFailurePoint::Rename,
  ] {
    let root = private_root(failure.label());
    let original = ApiProfileRegistry::default();
    save_api_profile_registry(&root, &original).expect("initial registry should save");
    let path = api_profile_registry_path(&root);
    let original_bytes = fs::read(&path).expect("initial registry should be readable");
    let replacement = registry_with_secret(ATTEMPTED_SECRET);

    let failure_guard = install_write_failure(&root, failure);
    let error = save_api_profile_registry(&root, &replacement)
      .expect_err("injected storage failure should reject replacement");
    drop(failure_guard);

    assert_eq!(
      fs::read(&path).expect("original registry should remain readable"),
      original_bytes,
      "{} failure replaced or damaged the original registry",
      failure.label()
    );
    assert_eq!(
      load_api_profile_registry(&root).expect("original registry should still load"),
      original
    );
    assert!(!serialized_error(&error).contains(ATTEMPTED_SECRET));
    assert_no_temporary_registry_files(&root);
    fs::remove_dir_all(root).ok();
  }
}

fn registry_with_secret(secret: &str) -> ApiProfileRegistry {
  let mut registry = ApiProfileRegistry::default();
  let profile_id = Uuid::new_v4().to_string();
  let credential_id = Uuid::new_v4().to_string();
  let timestamp = Utc::now().to_rfc3339();
  registry.tikhub_profiles.insert(
    profile_id.clone(),
    TikhubApiProfile {
      id: profile_id.clone(),
      name: "Replacement TikHub".to_string(),
      base_url: "https://api.tikhub.io".to_string(),
      credential_ref_id: credential_id.clone(),
      revision: 1,
      status: ApiProfileStatus::Untested,
      last_tested_at: None,
      test_summary: None,
      created_at: timestamp.clone(),
      updated_at: timestamp,
    },
  );
  registry.credentials.insert(
    credential_id.clone(),
    ApiCredential {
      id: credential_id,
      provider_type: CredentialProviderType::Tikhub,
      profile_id,
      revision: 1,
      secret: secret.to_string(),
    },
  );
  registry
}

fn private_root(label: &str) -> PathBuf {
  let root = std::env::temp_dir().join(format!("api-storage-failure-{label}-{}", Uuid::new_v4()));
  fs::create_dir(&root).expect("test root should be created");
  fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
    .expect("test root should be private");
  root
}

fn serialized_error(error: &crate::domain::AppError) -> String {
  serde_json::to_string(error).expect("error should serialize")
}

fn assert_no_temporary_registry_files(root: &Path) {
  let entries = fs::read_dir(root.join("secrets"))
    .expect("registry directory should remain readable")
    .map(|entry| {
      entry
        .expect("registry entry should be readable")
        .file_name()
    })
    .collect::<Vec<_>>();
  assert_eq!(entries.len(), 1, "temporary registry file was not removed");
  assert_eq!(entries[0].as_bytes(), b"api-config.json");
}
