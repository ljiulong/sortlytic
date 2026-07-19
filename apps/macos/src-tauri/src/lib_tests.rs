use super::*;

#[test]
fn legacy_api_profile_reads_are_not_exposed_as_tauri_commands() {
  let source = include_str!("lib.rs");
  let handler = source
    .split(".invoke_handler(tauri::generate_handler![")
    .nth(1)
    .expect("Tauri invoke handler should exist");

  for command in [
    "list_secret_refs",
    "get_tikhub_connector",
    "list_model_providers",
    "list_model_profiles",
  ] {
    assert!(
      !handler.contains(&format!("\n      {command},")),
      "legacy command {command} must not remain externally invokable"
    );
  }
}

#[test]
fn latest_task_run_states_are_exposed_as_a_single_batch_command() {
  let source = include_str!("lib.rs");
  let handler = source
    .split(".invoke_handler(tauri::generate_handler![")
    .nth(1)
    .expect("Tauri invoke handler should exist");

  assert!(handler.contains("\n      list_latest_task_runs,"));
}

#[test]
fn stored_record_counts_are_exposed_as_an_active_workspace_command() {
  let source = include_str!("lib.rs");
  let handler = source
    .split(".invoke_handler(tauri::generate_handler![")
    .nth(1)
    .expect("Tauri invoke handler should exist");
  let command = source
    .split("fn list_task_record_counts(")
    .nth(1)
    .and_then(|tail| tail.split("#[tauri::command]").next())
    .expect("record count command should exist");

  assert!(handler.contains("\n      list_task_record_counts,"));
  assert!(command.contains("resolve_workspace_root"));
  assert!(command.contains("records::list_task_record_counts"));
}

#[test]
fn task_results_are_exposed_as_an_active_workspace_command() {
  let source = include_str!("lib.rs");
  let handler = source
    .split(".invoke_handler(tauri::generate_handler![")
    .nth(1)
    .expect("Tauri invoke handler should exist");
  let command = source
    .split("fn list_task_results(")
    .nth(1)
    .and_then(|tail| tail.split("#[tauri::command]").next())
    .expect("task results command should exist");

  assert!(handler.contains("\n      list_task_results,"));
  assert!(command.contains("resolve_workspace_root"));
  assert!(command.contains("records::list_task_results"));
}

#[test]
fn packaged_app_can_open_a_completed_export_path() {
  let capability: serde_json::Value =
    serde_json::from_str(include_str!("../capabilities/default.json"))
      .expect("default capability must be valid JSON");
  let opener = capability["permissions"]
    .as_array()
    .and_then(|permissions| {
      permissions.iter().find(|permission| {
        permission
          .get("identifier")
          .and_then(serde_json::Value::as_str)
          == Some("opener:allow-open-path")
      })
    })
    .expect("export files need the narrow opener command permission");

  assert_eq!(
    opener
      .pointer("/allow/0/path")
      .and_then(serde_json::Value::as_str),
    Some("$APPDATA/**"),
    "open_path must stay scoped to Sortlytic application data"
  );
}

#[test]
fn packaged_app_includes_the_noto_sans_sc_license() {
  let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
    .expect("Tauri config must be valid JSON");

  assert_eq!(
    config
      .pointer("/bundle/resources/assets~1fonts~1OFL.txt")
      .and_then(serde_json::Value::as_str),
    Some("licenses/NotoSansSC-OFL.txt"),
    "the packaged app must carry the Noto Sans SC copyright and OFL text"
  );
}

#[test]
fn packaged_macos_app_seals_resources_with_an_ad_hoc_signature() {
  let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
    .expect("Tauri config must be valid JSON");

  assert_eq!(
    config
      .pointer("/bundle/macOS/signingIdentity")
      .and_then(serde_json::Value::as_str),
    Some("-"),
    "unsigned release builds still need a complete ad-hoc app bundle signature"
  );
}

#[test]
fn packaged_window_can_reach_the_narrow_layout_breakpoint() {
  let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
    .expect("Tauri config must be valid JSON");
  let min_width = config
    .pointer("/app/windows/0/minWidth")
    .and_then(serde_json::Value::as_u64)
    .expect("the main window must declare a minimum width");

  assert!(
    min_width <= 390,
    "the packaged app must support the required 390px narrow-window acceptance state"
  );
}

#[test]
fn prompt_activation_keeps_real_model_regressions_off_the_ui_thread() {
  let source = include_str!("lib.rs");
  let command = source
    .split("async fn activate_prompt_version(")
    .nth(1)
    .and_then(|tail| tail.split("fn list_prompt_regression_cases(").next())
    .expect("prompt activation should be an async Tauri command");

  assert!(command.contains("tauri::async_runtime::spawn_blocking"));
  assert!(command.contains("prompts::activate_prompt_version"));
}

#[test]
fn default_workspace_initialization_preserves_an_explicit_workspace() {
  let active_root = std::env::temp_dir().join(format!("active-workspace-{}", uuid::Uuid::new_v4()));
  let default_root =
    std::env::temp_dir().join(format!("default-workspace-{}", uuid::Uuid::new_v4()));
  let active = workspace::create_workspace("显式工作区", &active_root)
    .expect("active workspace should be created");
  let state = AppState::new();
  state.set_active_workspace(workspace_context_from_summary(&active));

  let summary = ensure_default_workspace_for_state(default_root.clone(), &state)
    .expect("active workspace should remain open");

  assert_eq!(summary.id, active.id);
  assert_eq!(summary.root_path, active.root_path);
  assert!(!default_root.exists());

  std::fs::remove_dir_all(active_root).ok();
  std::fs::remove_dir_all(default_root).ok();
}

#[test]
fn command_root_must_match_the_active_workspace() {
  let active_root = std::env::temp_dir().join(format!("active-root-{}", uuid::Uuid::new_v4()));
  let other_root = std::env::temp_dir().join(format!("other-root-{}", uuid::Uuid::new_v4()));
  std::fs::create_dir_all(&active_root).expect("active root should exist");
  std::fs::create_dir_all(&other_root).expect("other root should exist");
  let state = AppState::new();
  state.set_active_workspace(WorkspaceContext {
    id: "active-workspace".to_string(),
    name: "活动工作区".to_string(),
    root_path: active_root.clone(),
    schema_version: workspace::CURRENT_SCHEMA_VERSION,
  });

  let matching = resolve_workspace_root(Some(active_root.to_string_lossy().to_string()), &state);
  let mismatched = resolve_workspace_root(Some(other_root.to_string_lossy().to_string()), &state);

  assert_eq!(matching.expect("matching root should pass"), active_root);
  assert_eq!(
    mismatched.expect_err("other root must be rejected").code,
    domain::AppErrorCode::PermissionError
  );
  std::fs::remove_dir_all(active_root).ok();
  std::fs::remove_dir_all(other_root).ok();
}

#[test]
fn command_root_cannot_replace_a_missing_active_workspace() {
  let state = AppState::new();
  let error = resolve_workspace_root(Some("/tmp/arbitrary-workspace".to_string()), &state)
    .expect_err("commands require an active workspace");

  assert_eq!(error.code, domain::AppErrorCode::ValidationError);
}
