use std::path::Path;

use crate::api_profiles::{
  sync_api_profile_mirror, update_api_profile_registry, ApiCredential, ApiProfileStatus,
  CredentialProviderType, TikhubApiProfile,
};
use crate::domain::AppResult;
use crate::tasks::ReviseCollectionTaskInput;

impl ReviseCollectionTaskInput {
  pub(crate) fn user_edited_for_test(
    task_id: impl Into<String>,
    name: impl Into<String>,
    platforms: Vec<String>,
    data_types: Vec<String>,
    plan_json: serde_json::Value,
  ) -> Self {
    Self {
      task_id: task_id.into(),
      name: name.into(),
      platforms,
      data_types,
      source: "user_edited".to_string(),
      plan_json,
    }
  }
}

const TEST_PROFILE_ID: &str = "00000000-0000-4000-8000-000000000001";
const TEST_CREDENTIAL_ID: &str = "00000000-0000-4000-8000-000000000002";
const TESTED_AT: &str = "2026-07-01T00:00:00Z";
const TEST_CREDENTIAL: &str = "sortlytic-test-only-tikhub-token";

pub(super) fn install_successful_tikhub_profile(root_path: impl AsRef<Path>) -> AppResult<()> {
  let root_path = root_path.as_ref();
  update_api_profile_registry(root_path, |registry| {
    registry.tikhub_profiles.insert(
      TEST_PROFILE_ID.to_string(),
      TikhubApiProfile {
        id: TEST_PROFILE_ID.to_string(),
        name: "任务测试 TikHub API".to_string(),
        base_url: "https://api.tikhub.io".to_string(),
        credential_ref_id: TEST_CREDENTIAL_ID.to_string(),
        revision: 1,
        status: ApiProfileStatus::Success,
        last_tested_at: Some(TESTED_AT.to_string()),
        test_summary: None,
        created_at: TESTED_AT.to_string(),
        updated_at: TESTED_AT.to_string(),
      },
    );
    registry.credentials.insert(
      TEST_CREDENTIAL_ID.to_string(),
      ApiCredential {
        id: TEST_CREDENTIAL_ID.to_string(),
        provider_type: CredentialProviderType::Tikhub,
        profile_id: TEST_PROFILE_ID.to_string(),
        revision: 1,
        secret: TEST_CREDENTIAL.to_string(),
      },
    );
    registry.active_profile_ids.tikhub = Some(TEST_PROFILE_ID.to_string());
    Ok(())
  })?;
  sync_api_profile_mirror(root_path)
}

#[cfg(test)]
mod tests {
  use uuid::Uuid;

  use super::*;
  use crate::api_profiles::load_api_profile_registry;
  use crate::workspace::create_workspace;

  #[test]
  fn installs_the_same_successful_profile_and_credential_idempotently() {
    let root_path = std::env::temp_dir().join(format!(
      "sortlytic-task-test-tikhub-profile-{}",
      Uuid::new_v4()
    ));
    create_workspace("任务测试配置", &root_path).expect("workspace should be created");

    install_successful_tikhub_profile(&root_path).expect("profile should install");
    let first = load_api_profile_registry(&root_path).expect("registry should load");
    install_successful_tikhub_profile(&root_path).expect("profile should reinstall");
    let second = load_api_profile_registry(&root_path).expect("registry should reload");

    assert_eq!(first, second);
    assert_eq!(
      second.active_profile_ids.tikhub.as_deref(),
      Some(TEST_PROFILE_ID)
    );
    assert_eq!(
      second
        .credentials
        .get(TEST_CREDENTIAL_ID)
        .map(|credential| credential.secret.as_str()),
      Some(TEST_CREDENTIAL)
    );
    std::fs::remove_dir_all(root_path).ok();
  }
}
