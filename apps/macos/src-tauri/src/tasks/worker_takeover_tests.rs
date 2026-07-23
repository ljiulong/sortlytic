use super::*;
use crate::tasks::test_support::install_successful_tikhub_profile;
use crate::tasks::{
  claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task, get_task,
  get_task_run, save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::create_workspace;
use serde_json::json;
use uuid::Uuid;

#[test]
fn takeover_after_response_fences_all_old_generation_side_effects() {
  let root = std::env::temp_dir().join(format!("worker-response-takeover-{}", Uuid::new_v4()));
  create_workspace("响应后接管栅栏测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "响应后接管任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["item_detail".to_string()],
    },
  )
  .expect("task should create");
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
          "params": {"item_id": "video-takeover"}
        }],
        "record_limit": 1,
        "request_limit": 1,
        "budget_limit": {"currency": "USD", "amount_micros": 35_000_000},
        "missing_fields": [],
        "requires_user_confirmation": true
      }),
      validation_status: "valid".to_string(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )
  .expect("plan should save");
  assert_eq!(plan.schema_version, 2);
  confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should confirm");
  enqueue_task(&root, &task.id).expect("task should enqueue");
  let run = claim_next_task(&root)
    .expect("task claim should succeed")
    .expect("queued task should exist");

  let connection = super::open_workspace_connection(&root).expect("database should open");
  let now = Utc::now();
  connection
    .execute(
      "INSERT INTO task_worker_lease (
         id, owner_id, lease_expires_at, created_at, updated_at, generation
       ) VALUES ('task_worker', 'stale-owner', ?1, ?2, ?2, 1)",
      params![now.timestamp_millis() + 120_000, now.to_rfc3339()],
    )
    .expect("stale lease should install");
  drop(connection);
  let stale = WorkerFence::new("stale-owner".to_string(), 1).expect("stale fence should construct");

  let execution_error = execute_claimed_run_with_guard(
    &root,
    &run,
    Some(&stale),
    |_| Ok(()),
    |_request| {
      let connection = super::open_workspace_connection(&root)?;
      let changed = connection
        .execute(
          "UPDATE task_worker_lease
           SET owner_id = 'replacement-owner', generation = 2,
               lease_expires_at = ?1, updated_at = ?2
           WHERE id = 'task_worker' AND owner_id = 'stale-owner' AND generation = 1",
          params![
            Utc::now().timestamp_millis() + 120_000,
            Utc::now().to_rfc3339()
          ],
        )
        .map_err(database_error)?;
      assert_eq!(changed, 1, "response callback should install generation 2");
      let record = json!({
        "aweme_id": "video-takeover",
        "author": {
          "user_id": "account-takeover",
          "nickname": "旧代账号"
        }
      });
      Ok(CollectionPage {
        records: vec![record.clone()],
        next_cursor: None,
        has_more: false,
        raw_response: json!({"code": 200, "data": record, "has_more": false}),
      })
    },
  )
  .expect_err("generation 1 must stop after generation 2 takes over");
  assert_eq!(
    execution_error
      .safe_details
      .get("operation")
      .map(String::as_str),
    Some("task_worker_fence")
  );

  let terminal_error = finalize_claimed_run(&root, &run, Some(&stale), Err(execution_error))
    .expect_err("generation 1 must not write a success or failure terminal");
  assert_eq!(
    terminal_error
      .safe_details
      .get("operation")
      .map(String::as_str),
    Some("task_worker_fence")
  );

  let connection = super::open_workspace_connection(&root).expect("database should reopen");
  let stale_state = connection
    .query_row(
      "SELECT
         (SELECT COUNT(*) FROM raw_record WHERE task_run_id = ?1),
         (SELECT COUNT(*)
          FROM normalized_record AS normalized
          JOIN raw_record AS raw ON raw.id = normalized.raw_record_id
          WHERE raw.task_run_id = ?1),
         (SELECT COUNT(*) FROM collected_account WHERE task_run_id = ?1),
         (SELECT status
          FROM collection_page_checkpoint
          WHERE task_run_step_id IN (
            SELECT id FROM task_run_step WHERE task_run_id = ?1
          )),
         (SELECT status FROM task_run WHERE id = ?1),
         (SELECT status FROM collection_task WHERE id = ?2)",
      params![run.id, task.id],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, i64>(2)?,
          row.get::<_, String>(3)?,
          row.get::<_, String>(4)?,
          row.get::<_, String>(5)?,
        ))
      },
    )
    .expect("stale generation state should load");
  assert_eq!(
    stale_state,
    (
      0,
      0,
      0,
      "requesting".to_string(),
      "running".to_string(),
      "running".to_string()
    )
  );
  assert_eq!(json_file_count(&root.join("raw/tikhub")), 0);
  drop(connection);

  let replacement = WorkerFence::new("replacement-owner".to_string(), 2)
    .expect("replacement fence should construct");
  assert_eq!(
    super::super::recovery::recover_interrupted_runs_with_fence(&root, &replacement)
      .expect("generation 2 should resolve the interrupted request"),
    0
  );

  let connection = super::open_workspace_connection(&root).expect("database should reopen");
  let checkpoint_status: String = connection
    .query_row(
      "SELECT status
       FROM collection_page_checkpoint
       WHERE task_run_step_id IN (
         SELECT id FROM task_run_step WHERE task_run_id = ?1
       )",
      [&run.id],
      |row| row.get(0),
    )
    .expect("checkpoint should load");
  assert_eq!(checkpoint_status, "uncertain");
  let recovered_run = get_task_run(&connection, &run.id).expect("run should remain readable");
  assert_eq!(recovered_run.status, "failed");
  assert_eq!(
    recovered_run.error_code.as_deref(),
    Some("UNCERTAIN_REQUEST_AFTER_CRASH")
  );
  assert_eq!(
    get_task(&root, &task.id)
      .expect("task should remain readable")
      .status,
    "failed"
  );
  assert_eq!(json_file_count(&root.join("raw/tikhub")), 0);

  std::fs::remove_dir_all(root).ok();
}

fn json_file_count(path: &Path) -> usize {
  std::fs::read_dir(path)
    .expect("raw directory should be readable")
    .filter_map(Result::ok)
    .filter(|entry| {
      entry
        .path()
        .extension()
        .is_some_and(|extension| extension == "json")
    })
    .count()
}
