use super::*;
use crate::workspace::create_workspace;

#[test]
fn persists_raw_file_and_normalized_record_atomically() {
  let workspace = TestWorkspace::new("persist", &["keyword_search"]);
  let run_id = workspace.insert_running_task_run();
  let result = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![tiktok_video("video-1", "第一版")],
  )
  .expect("page should persist");

  assert_eq!((result.inserted_count, result.existing_count), (1, 0));
  let raw = &result.raw_records[0];
  let normalized = &result.normalized_records[0];
  assert_eq!(raw.task_run_id.as_deref(), Some(run_id.as_str()));
  assert_eq!(raw.data_type, "keyword_search");
  assert_eq!(normalized.content_text.as_deref(), Some("第一版"));
  assert_eq!(normalized.author_id.as_deref(), Some("author-1"));
  assert_eq!(normalized.metrics_json["digg_count"], 12);
  assert_eq!(workspace.count_rows("raw_record"), 1);
  assert_eq!(workspace.count_rows("normalized_record"), 1);
  assert_eq!(
    serde_json::from_slice::<Value>(
      &read_bounded_regular_file(&workspace.root.join(&raw.raw_file_path))
        .expect("raw file should read")
    )
    .expect("raw JSON should parse"),
    tiktok_video("video-1", "第一版")
  );
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(workspace.root.join(&raw.raw_file_path))
      .expect("raw metadata should read")
      .permissions()
      .mode()
      & 0o777;
    assert_eq!(mode, 0o600);
  }
}

#[test]
fn identity_is_idempotent_only_within_the_same_run_and_data_type() {
  let workspace = TestWorkspace::new("identity", &["keyword_search", "item_detail"]);
  let first_run = workspace.insert_running_task_run();
  let first = persist_page(
    &workspace,
    &first_run,
    "keyword_search",
    vec![tiktok_video("shared", "第一版")],
  )
  .expect("first observation should persist");
  let retried = persist_page(
    &workspace,
    &first_run,
    "keyword_search",
    vec![tiktok_video("shared", "第二版")],
  )
  .expect("retry should preserve first snapshot");
  let detail = persist_page(
    &workspace,
    &first_run,
    "item_detail",
    vec![serde_json::json!({
      "aweme_detail": {
        "aweme_id": "shared",
        "desc": "详情版",
        "create_time": i64::MIN
      }
    })],
  )
  .expect("detail is a distinct observation");
  workspace.finish_run(&first_run);
  let second_run = workspace.insert_running_task_run();
  let second = persist_page(
    &workspace,
    &second_run,
    "keyword_search",
    vec![tiktok_video("shared", "新运行")],
  )
  .expect("new run is a distinct observation");

  assert_eq!(retried.raw_records[0].id, first.raw_records[0].id);
  assert_eq!(
    retried.normalized_records[0].content_text.as_deref(),
    Some("第一版")
  );
  assert_ne!(detail.raw_records[0].id, first.raw_records[0].id);
  assert_eq!(detail.normalized_records[0].published_at, None);
  assert_ne!(second.raw_records[0].id, first.raw_records[0].id);
  assert_eq!(workspace.count_rows("raw_record"), 3);
}

#[test]
fn invalid_or_conflicting_page_rolls_back_without_files() {
  let workspace = TestWorkspace::new("rollback", &["keyword_search"]);
  let run_id = workspace.insert_running_task_run();
  let invalid = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![
      tiktok_video("video-1", "有效"),
      serde_json::json!({ "desc": "缺少 ID" }),
    ],
  )
  .expect_err("invalid record must reject complete page");
  let conflict = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![
      tiktok_video("video-1", "版本一"),
      tiktok_video("video-1", "版本二"),
    ],
  )
  .expect_err("conflicting duplicate must reject complete page");

  assert!(invalid.message.contains("平台记录 ID"));
  assert!(conflict.message.contains("冲突记录"));
  assert_eq!(workspace.count_rows("raw_record"), 0);
  assert_eq!(json_file_count(&workspace.root.join(RAW_DIRECTORY)), 0);
}

#[test]
fn rejects_oversized_record_before_file_or_database_write() {
  let workspace = TestWorkspace::new("oversized", &["keyword_search"]);
  let run_id = workspace.insert_running_task_run();
  let error = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![serde_json::json!({
      "aweme_id": "video-large",
      "desc": "x".repeat(MAX_RAW_RECORD_BYTES + 1)
    })],
  )
  .expect_err("oversized record must be rejected");

  assert!(error.message.contains("16 MiB"));
  assert_eq!(workspace.count_rows("raw_record"), 0);
  assert_eq!(json_file_count(&workspace.root.join(RAW_DIRECTORY)), 0);

  let page_error = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![
      tiktok_video("page-large-1", &"x".repeat(MAX_RAW_RECORD_BYTES / 2)),
      tiktok_video("page-large-2", &"y".repeat(MAX_RAW_RECORD_BYTES / 2)),
    ],
  )
  .expect_err("oversized page must be rejected");
  assert!(page_error.message.contains("整页"));

  let existing_value = tiktok_video("existing-large", "预置文件");
  let prepared = prepared_record(
    &workspace,
    &run_id,
    "keyword_search",
    existing_value.clone(),
  );
  let existing_path = workspace
    .root
    .join(RAW_DIRECTORY)
    .join(format!("{}.json", prepared.identity_hash));
  fs::File::create(&existing_path)
    .and_then(|file| file.set_len(MAX_RAW_RECORD_BYTES as u64 + 1))
    .expect("oversized sparse file should be created");
  let existing_error = persist_page(&workspace, &run_id, "keyword_search", vec![existing_value])
    .expect_err("oversized existing file must be rejected");
  assert!(existing_error.message.contains("16 MiB"));
  assert_eq!(workspace.count_rows("raw_record"), 0);
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_raw_directory_before_writing_outside_workspace() {
  use std::os::unix::fs::symlink;

  let workspace = TestWorkspace::new("symlink", &["keyword_search"]);
  let run_id = workspace.insert_running_task_run();
  let outside = std::env::temp_dir().join(format!("records-outside-{}", Uuid::new_v4()));
  fs::create_dir_all(&outside).expect("outside directory should exist");
  fs::remove_dir_all(workspace.root.join(RAW_DIRECTORY)).expect("raw directory should remove");
  symlink(&outside, workspace.root.join(RAW_DIRECTORY)).expect("symlink should create");

  let error = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![tiktok_video("video-1", "越界")],
  )
  .expect_err("symlinked raw directory must be rejected");
  assert!(matches!(
    error.code,
    crate::domain::AppErrorCode::PermissionError | crate::domain::AppErrorCode::WorkspaceError
  ));
  assert_eq!(json_file_count(&outside), 0);

  fs::remove_file(workspace.root.join(RAW_DIRECTORY)).expect("raw symlink should remove");
  fs::create_dir(workspace.root.join(RAW_DIRECTORY)).expect("raw directory should restore");
  fs::remove_dir_all(workspace.root.join(TEMP_DIRECTORY)).expect("temp directory should remove");
  symlink(&outside, workspace.root.join(TEMP_DIRECTORY)).expect("temp symlink should create");
  let temp_error = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![tiktok_video("video-2", "临时目录越界")],
  )
  .expect_err("symlinked temp directory must be rejected");
  assert!(matches!(
    temp_error.code,
    crate::domain::AppErrorCode::PermissionError | crate::domain::AppErrorCode::WorkspaceError
  ));
  assert_eq!(
    fs::read_dir(&outside).expect("outside should read").count(),
    0
  );
  fs::remove_dir_all(outside).ok();
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_database_and_mismatched_registered_root() {
  use std::os::unix::fs::symlink;

  let workspace = TestWorkspace::new("root-boundary", &["keyword_search"]);
  let run_id = workspace.insert_running_task_run();
  let substitute = std::env::temp_dir().join(format!("records-substitute-{}", Uuid::new_v4()));
  fs::create_dir_all(substitute.join(RAW_DIRECTORY)).expect("raw directory should exist");
  fs::create_dir_all(substitute.join(TEMP_DIRECTORY)).expect("temp directory should exist");
  symlink(
    workspace.root.join(DATABASE_FILE_NAME),
    substitute.join(DATABASE_FILE_NAME),
  )
  .expect("database symlink should create");
  let symlink_error = super::persist_prepared_records(
    &substitute,
    &normalized_input(
      &workspace,
      &run_id,
      "keyword_search",
      vec![tiktok_video("v", "x")],
    ),
    vec![prepared_record(
      &workspace,
      &run_id,
      "keyword_search",
      tiktok_video("v", "x"),
    )],
  )
  .expect_err("database symlink must be rejected");
  assert!(matches!(
    symlink_error.code,
    crate::domain::AppErrorCode::PermissionError | crate::domain::AppErrorCode::WorkspaceError
  ));
  fs::remove_dir_all(substitute).ok();

  let connection =
    open_workspace_database(workspace.root.join(DATABASE_FILE_NAME)).expect("database should open");
  connection
    .execute(
      "UPDATE workspace SET root_path = '/tmp/not-this-workspace'",
      [],
    )
    .expect("root metadata should update");
  let mismatch = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![tiktok_video("video-1", "错配")],
  )
  .expect_err("registered root mismatch must be rejected");
  assert!(matches!(
    mismatch.code,
    crate::domain::AppErrorCode::PermissionError | crate::domain::AppErrorCode::WorkspaceError
  ));
}

#[test]
fn recovers_orphan_file_as_the_first_snapshot_after_interruption() {
  let workspace = TestWorkspace::new("orphan", &["keyword_search"]);
  let run_id = workspace.insert_running_task_run();
  let first_value = tiktok_video("video-1", "中断前快照");
  let input = normalized_input(
    &workspace,
    &run_id,
    "keyword_search",
    vec![first_value.clone()],
  );
  let prepared = prepare_record(&input, &first_value).expect("record should prepare");
  let orphan_path = workspace
    .root
    .join(RAW_DIRECTORY)
    .join(format!("{}.json", prepared.identity_hash));
  fs::write(&orphan_path, &prepared.raw_bytes).expect("orphan should be simulated");

  let recovered = persist_page(
    &workspace,
    &run_id,
    "keyword_search",
    vec![tiktok_video("video-1", "重启后变化")],
  )
  .expect("valid orphan should be adopted");
  assert_eq!(
    recovered.normalized_records[0].content_text.as_deref(),
    Some("中断前快照")
  );
  assert_eq!(workspace.count_rows("raw_record"), 1);
}

fn persist_page(
  workspace: &TestWorkspace,
  run_id: &str,
  data_type: &str,
  records: Vec<Value>,
) -> AppResult<PersistCollectionPageResult> {
  super::super::persist_collection_page(
    &workspace.root,
    super::super::PersistCollectionPageInput {
      task_id: workspace.task_id.clone(),
      task_run_id: run_id.to_string(),
      platform: "tiktok".to_string(),
      data_type: data_type.to_string(),
      records,
      collected_at: Some("2026-07-12T08:00:00+00:00".to_string()),
    },
  )
}

fn normalized_input(
  workspace: &TestWorkspace,
  run_id: &str,
  data_type: &str,
  records: Vec<Value>,
) -> NormalizedInput {
  super::super::normalize_input(super::super::PersistCollectionPageInput {
    task_id: workspace.task_id.clone(),
    task_run_id: run_id.to_string(),
    platform: "tiktok".to_string(),
    data_type: data_type.to_string(),
    records,
    collected_at: Some("2026-07-12T08:00:00+00:00".to_string()),
  })
  .expect("input should normalize")
}

fn prepared_record(
  workspace: &TestWorkspace,
  run_id: &str,
  data_type: &str,
  value: Value,
) -> PreparedRecord {
  let input = normalized_input(workspace, run_id, data_type, vec![value.clone()]);
  prepare_record(&input, &value).expect("record should prepare")
}

fn tiktok_video(id: &str, text: &str) -> Value {
  serde_json::json!({
    "aweme_id": id,
    "desc": text,
    "share_url": format!("https://www.tiktok.com/@author/video/{id}"),
    "create_time": 1_720_000_000,
    "region": "US",
    "author": { "uid": "author-1", "nickname": "测试作者" },
    "statistics": { "digg_count": 12, "comment_count": 3 }
  })
}

fn json_file_count(root: &Path) -> usize {
  fs::read_dir(root)
    .map(|entries| {
      entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("json"))
        .count()
    })
    .unwrap_or(0)
}

struct TestWorkspace {
  root: PathBuf,
  task_id: String,
  data_types: Vec<String>,
}

impl TestWorkspace {
  fn new(label: &str, data_types: &[&str]) -> Self {
    let root = std::env::temp_dir().join(format!("records-{label}-{}", Uuid::new_v4()));
    create_workspace("记录测试", &root).expect("workspace should be created");
    Self {
      root,
      task_id: Uuid::new_v4().to_string(),
      data_types: data_types
        .iter()
        .map(|value| (*value).to_string())
        .collect(),
    }
  }

  fn insert_running_task_run(&self) -> String {
    let connection =
      open_workspace_database(self.root.join(DATABASE_FILE_NAME)).expect("database should open");
    let now = "2026-07-12T08:00:00+00:00";
    let exists = connection
      .query_row(
        "SELECT COUNT(*) FROM collection_task WHERE id = ?1",
        params![self.task_id],
        |row| row.get::<_, i64>(0),
      )
      .expect("task count should query");
    if exists == 0 {
      connection
        .execute(
          "INSERT INTO collection_task (
            id, name, source_type, status, platforms_json, data_types_json,
            created_at, updated_at, confirmed_at
          ) VALUES (?1, '记录测试', 'form', 'running', '[\"tiktok\"]', ?2, ?3, ?3, ?3)",
          params![
            self.task_id,
            serde_json::json!(self.data_types).to_string(),
            now
          ],
        )
        .expect("task should insert");
    } else {
      connection
        .execute(
          "UPDATE collection_task SET status = 'running' WHERE id = ?1",
          params![self.task_id],
        )
        .expect("task should resume for next run");
    }
    let run_id = Uuid::new_v4().to_string();
    connection
      .execute(
        "INSERT INTO task_run (id, task_id, status, started_at, current_stage)
         VALUES (?1, ?2, 'running', ?3, '执行采集')",
        params![run_id, self.task_id, now],
      )
      .expect("run should insert");
    run_id
  }

  fn finish_run(&self, run_id: &str) {
    let connection =
      open_workspace_database(self.root.join(DATABASE_FILE_NAME)).expect("database should open");
    connection
      .execute(
        "UPDATE task_run SET status = 'success', ended_at = ?1 WHERE id = ?2",
        params!["2026-07-12T09:00:00+00:00", run_id],
      )
      .expect("run should finish");
    connection
      .execute(
        "UPDATE collection_task SET status = 'success' WHERE id = ?1",
        params![self.task_id],
      )
      .expect("task should finish");
  }

  fn count_rows(&self, table: &str) -> i64 {
    let connection =
      open_workspace_database(self.root.join(DATABASE_FILE_NAME)).expect("database should open");
    connection
      .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
        row.get(0)
      })
      .expect("row count should query")
  }
}

impl Drop for TestWorkspace {
  fn drop(&mut self) {
    fs::remove_dir_all(&self.root).ok();
  }
}
