use super::*;
use crate::workspace::create_workspace;

#[test]
fn queued_run_can_be_claimed_and_completed_atomically() {
  let (root_path, task, plan) = prepared_task_workspace("execution-success");
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");

  assert_eq!(queued.plan_id.as_deref(), Some(plan.id.as_str()));
  assert_eq!(queued.attempt_number, 1);
  assert!(queued.claimed_at.is_none());
  let serialized = serde_json::to_value(&queued).expect("run should serialize");
  assert_eq!(serialized["plan_id"], plan.id);
  assert_eq!(serialized["attempt_number"], 1);
  assert!(serialized["claimed_at"].is_null());

  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let running_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(running.id, queued.id);
  assert_eq!(running.status, "running");
  assert!(running.claimed_at.is_some());
  assert_eq!(running_task.status, "running");

  let completed = complete_task_run(
    &root_path,
    &running.id,
    serde_json::json!({ "request_count": 1 }),
  )
  .expect("running task should complete");
  let completed_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(completed.status, "success");
  assert!(completed.ended_at.is_some());
  assert_eq!(completed.claimed_at, running.claimed_at);
  assert_eq!(completed_task.status, "success");
  assert!(completed_task.completed_at.is_some());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn failed_run_can_create_a_new_retry_run() {
  let (root_path, task, plan) = prepared_task_workspace("execution-retry");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");

  let failed = fail_task_run(
    &root_path,
    &running.id,
    "TIKHUB_REQUEST_ERROR",
    "网络超时",
    false,
  )
  .expect("running task should fail");
  let failed_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(failed.status, "failed");
  assert!(!failed.retryable);
  assert_eq!(failed.plan_id.as_deref(), Some(plan.id.as_str()));
  assert_eq!(failed.attempt_number, 1);
  assert!(failed.claimed_at.is_some());
  assert_eq!(failed_task.status, "failed");

  let retry = retry_task(&root_path, &task.id, None).expect("retry should enqueue");
  assert_ne!(retry.id, running.id);
  assert_eq!(retry.status, "queued");
  assert_eq!(retry.plan_id, failed.plan_id);
  assert_eq!(retry.attempt_number, 2);
  assert!(retry.claimed_at.is_none());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn interrupted_running_task_is_requeued_and_claimable_again() {
  let (root_path, task, plan) = prepared_task_workspace("execution-recovery");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let first_claim = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");

  let recovered = recover_interrupted_runs(&root_path).expect("recovery should succeed");
  let recovered_run = get_task_run(
    &open_workspace_connection(&root_path).expect("database should open"),
    &first_claim.id,
  )
  .expect("run should reload");
  let recovered_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(recovered, 1);
  assert_eq!(recovered_run.status, "queued");
  assert_eq!(recovered_run.plan_id.as_deref(), Some(plan.id.as_str()));
  assert_eq!(recovered_run.attempt_number, 1);
  assert!(recovered_run.claimed_at.is_none());
  assert_eq!(recovered_task.status, "queued");

  let second_claim = claim_next_task(&root_path)
    .expect("second claim should succeed")
    .expect("recovered run should be claimable");
  assert_eq!(second_claim.id, first_claim.id);
  assert!(second_claim.claimed_at.is_some());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn a_new_confirmed_plan_starts_its_own_attempt_sequence() {
  let (root_path, task, first_plan) = prepared_task_workspace("execution-new-plan");
  let first_run = enqueue_task(&root_path, &task.id).expect("first plan should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("first run should be claimed");
  fail_task_run(
    &root_path,
    &running.id,
    "TIKHUB_REQUEST_ERROR",
    "网络超时",
    true,
  )
  .expect("first run should fail");

  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_task SET status = 'waiting_confirmation' WHERE id = ?1",
      params![task.id],
    )
    .expect("test fixture should reopen plan editing");
  drop(connection);

  let mut replacement_input = execution_plan_input(&task.id);
  replacement_input.source = "user_edited".to_string();
  replacement_input.plan_json["steps"][0]["params"]["item_id"] = serde_json::json!("video-2");
  let replacement =
    save_collection_plan(&root_path, replacement_input).expect("replacement plan should save");
  confirm_collection_plan(&root_path, &task.id, &replacement.id)
    .expect("replacement plan should confirm");
  let replacement_run =
    enqueue_task(&root_path, &task.id).expect("replacement plan should enqueue");

  assert_eq!(first_run.plan_id.as_deref(), Some(first_plan.id.as_str()));
  assert_eq!(first_run.attempt_number, 1);
  assert_eq!(
    replacement_run.plan_id.as_deref(),
    Some(replacement.id.as_str())
  );
  assert_eq!(replacement_run.attempt_number, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn legacy_runs_without_a_plan_cannot_be_claimed_or_retried() {
  let (root_path, task, _) = prepared_task_workspace("execution-legacy-run");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, status, started_at, current_stage)
       VALUES ('legacy-run', ?1, 'queued', '2026-07-13T08:00:00+00:00', '等待执行')",
      params![task.id],
    )
    .expect("legacy run should insert");
  drop(connection);

  assert!(claim_next_task(&root_path)
    .expect("claim should not fail")
    .is_none());

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  connection
    .execute(
      "UPDATE task_run SET status = 'failed' WHERE id = 'legacy-run'",
      [],
    )
    .expect("legacy run should become failed");
  connection
    .execute(
      "UPDATE collection_task SET status = 'failed' WHERE id = ?1",
      params![task.id],
    )
    .expect("task should become failed");
  drop(connection);

  let legacy = get_task_run(
    &open_workspace_connection(&root_path).expect("database should reopen"),
    "legacy-run",
  )
  .expect("legacy terminal run should remain readable");
  assert!(legacy.plan_id.is_none());
  assert_eq!(legacy.attempt_number, 1);
  assert!(legacy.claimed_at.is_none());

  let error = retry_task(&root_path, &task.id, None)
    .expect_err("legacy failed run must require plan reconfirmation");
  assert!(error.message.contains("重新确认"));
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let run_count = connection
    .query_row(
      "SELECT COUNT(*) FROM task_run WHERE task_id = ?1",
      params![task.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("run count should load");
  assert_eq!(run_count, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn enqueue_rejects_a_missing_confirmed_plan_without_mutation() {
  let (root_path, task, _) = prepared_task_workspace("execution-missing-plan");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_plan SET confirmed_by_user = 0 WHERE task_id = ?1",
      params![task.id],
    )
    .expect("confirmed plan should be removed from the fixture");
  drop(connection);

  let error =
    enqueue_task(&root_path, &task.id).expect_err("missing confirmed plan must reject enqueue");
  assert!(error.message.contains("重新确认"));

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let run_count = connection
    .query_row(
      "SELECT COUNT(*) FROM task_run WHERE task_id = ?1",
      params![task.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("run count should load");
  let task_status = connection
    .query_row(
      "SELECT status FROM collection_task WHERE id = ?1",
      params![task.id],
      |row| row.get::<_, String>(0),
    )
    .expect("task status should load");
  assert_eq!(run_count, 0);
  assert_eq!(task_status, "waiting_confirmation");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn enqueue_rejects_ambiguous_confirmed_plans_without_mutation() {
  let (root_path, task, plan) = prepared_task_workspace("execution-ambiguous-plan");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO collection_plan (
        id, task_id, source, schema_version, plan_json, validation_status,
        validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
      )
      SELECT 'duplicate-confirmed-plan', task_id, source, schema_version, plan_json,
             validation_status, validation_errors_json, cost_estimate_json, 1,
             created_at, updated_at
      FROM collection_plan WHERE id = ?1",
      params![plan.id],
    )
    .expect("ambiguous confirmed plan should insert");
  drop(connection);

  let error =
    enqueue_task(&root_path, &task.id).expect_err("ambiguous confirmed plans must reject enqueue");
  assert!(error.message.contains("唯一") && error.message.contains("采集计划"));

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let run_count = connection
    .query_row(
      "SELECT COUNT(*) FROM task_run WHERE task_id = ?1",
      params![task.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("run count should load");
  let task_status = connection
    .query_row(
      "SELECT status FROM collection_task WHERE id = ?1",
      params![task.id],
      |row| row.get::<_, String>(0),
    )
    .expect("task status should load");
  assert_eq!(run_count, 0);
  assert_eq!(task_status, "waiting_confirmation");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn enqueue_rejects_a_stale_confirmation_when_a_newer_plan_exists() {
  let (root_path, task, first_plan) = prepared_task_workspace("execution-stale-confirmation");
  let mut replacement_input = execution_plan_input(&task.id);
  replacement_input.source = "user_edited".to_string();
  replacement_input.plan_json["steps"][0]["params"]["item_id"] = serde_json::json!("video-2");
  let replacement =
    save_collection_plan(&root_path, replacement_input).expect("replacement plan should save");

  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_plan
       SET confirmed_by_user = 1, validation_status = 'valid',
           created_at = '2026-07-13T08:00:00+00:00'
       WHERE id = ?1",
      params![first_plan.id],
    )
    .expect("late confirmation should restore the stale plan marker");
  connection
    .execute(
      "UPDATE collection_plan
       SET confirmed_by_user = 0, created_at = '2026-07-13T08:00:01+00:00'
       WHERE id = ?1",
      params![replacement.id],
    )
    .expect("replacement should remain the latest unconfirmed plan");
  connection
    .execute(
      "UPDATE collection_task
       SET status = 'waiting_confirmation', confirmed_at = '2026-07-13T08:00:02+00:00'
       WHERE id = ?1",
      params![task.id],
    )
    .expect("late confirmation should restore the task marker");
  let fixture = connection
    .query_row(
      "SELECT
         (SELECT id FROM collection_plan WHERE task_id = ?1
          ORDER BY created_at DESC, id DESC LIMIT 1),
         (SELECT id FROM collection_plan WHERE task_id = ?1
          AND confirmed_by_user = 1 AND validation_status = 'valid'),
         (SELECT confirmed_at FROM collection_task WHERE id = ?1)",
      params![task.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .expect("race fixture should be readable");
  assert_eq!(fixture.0, replacement.id);
  assert_eq!(fixture.1, first_plan.id);
  assert!(fixture.2.is_some());
  drop(connection);

  let error = enqueue_task(&root_path, &task.id)
    .expect_err("stale confirmation must not enqueue an older plan");
  assert!(error.message.contains("最新") && error.message.contains("重新确认"));

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let run_count = connection
    .query_row(
      "SELECT COUNT(*) FROM task_run WHERE task_id = ?1",
      params![task.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("run count should load");
  let task_state = connection
    .query_row(
      "SELECT status, confirmed_at FROM collection_task WHERE id = ?1",
      params![task.id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .expect("task state should load");
  assert_eq!(run_count, 0);
  assert_eq!(task_state.0, "waiting_confirmation");
  assert!(task_state.1.is_some());
  drop(connection);

  confirm_collection_plan(&root_path, &task.id, &replacement.id)
    .expect("latest replacement should be confirmable after rejecting stale state");
  let run = enqueue_task(&root_path, &task.id).expect("latest confirmed plan should enqueue");
  assert_eq!(run.plan_id.as_deref(), Some(replacement.id.as_str()));
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let confirmed_ids = {
    let mut statement = connection
      .prepare(
        "SELECT id FROM collection_plan
         WHERE task_id = ?1 AND confirmed_by_user = 1
         ORDER BY created_at DESC, id DESC",
      )
      .expect("confirmed plan query should prepare");
    let rows = statement
      .query_map(params![task.id], |row| row.get::<_, String>(0))
      .expect("confirmed plans should query");
    rows
      .collect::<rusqlite::Result<Vec<_>>>()
      .expect("confirmed plans should load")
  };
  assert_eq!(confirmed_ids, vec![replacement.id]);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn failed_confirmation_persists_needs_review_before_returning_error() {
  let (root_path, task, plan) = prepared_task_workspace("execution-invalid-confirmation");
  let mut invalid_plan = execution_plan_input(&task.id).plan_json;
  invalid_plan["time_range"] = Value::Null;
  invalid_plan["steps"][0]["params"]["time_range"] = Value::Null;
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_plan
       SET plan_json = ?1, validation_status = 'valid', validation_errors_json = '[]',
           confirmed_by_user = 1
       WHERE id = ?2",
      params![invalid_plan.to_string(), plan.id],
    )
    .expect("test fixture should invalidate the persisted plan");
  drop(connection);

  let error = confirm_collection_plan(&root_path, &task.id, &plan.id)
    .expect_err("invalid persisted plan must fail confirmation");
  assert!(error.message.contains("未通过后端校验"));

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let persisted = connection
    .query_row(
      "SELECT validation_status, validation_errors_json, confirmed_by_user,
              (SELECT confirmed_at FROM collection_task WHERE id = ?2)
       FROM collection_plan WHERE id = ?1",
      params![plan.id, task.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, i64>(2)?,
          row.get::<_, Option<String>>(3)?,
        ))
      },
    )
    .expect("failed validation state should load");
  assert_eq!(persisted.0, "needs_review");
  assert!(serde_json::from_str::<Vec<String>>(&persisted.1).is_ok_and(|errors| !errors.is_empty()));
  assert_eq!(persisted.2, 0);
  assert!(persisted.3.is_none());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn latest_plan_order_breaks_timestamp_ties_by_id() {
  let (root_path, task, first_plan) = prepared_task_workspace("execution-plan-order-tie");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO collection_plan (
        id, task_id, source, schema_version, plan_json, validation_status,
        validation_errors_json, cost_estimate_json, confirmed_by_user, created_at, updated_at
      )
      SELECT 'zz-tiebreak-plan', task_id, source, schema_version, plan_json,
             validation_status, validation_errors_json, cost_estimate_json, 0,
             '2026-07-13T08:00:00+00:00', updated_at
      FROM collection_plan WHERE id = ?1",
      params![first_plan.id],
    )
    .expect("tie-break plan should insert");
  connection
    .execute(
      "UPDATE collection_plan SET created_at = '2026-07-13T08:00:00+00:00'
       WHERE id = ?1",
      params![first_plan.id],
    )
    .expect("first plan timestamp should match");

  let latest = latest_plan_for_task(&connection, &task.id).expect("latest plan should load");
  assert_eq!(latest.id, "zz-tiebreak-plan");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn cancelling_a_running_task_preserves_claim_audit_time() {
  let (root_path, task, _) = prepared_task_workspace("execution-cancel-claim");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("task should be claimed");

  cancel_task(&root_path, &task.id).expect("running task should cancel");
  let cancelled = get_task_run(
    &open_workspace_connection(&root_path).expect("database should open"),
    &running.id,
  )
  .expect("cancelled run should load");

  assert_eq!(cancelled.status, "cancelled");
  assert_eq!(cancelled.claimed_at, running.claimed_at);

  std::fs::remove_dir_all(root_path).ok();
}

fn prepared_task_workspace(
  label: &str,
) -> (std::path::PathBuf, CollectionTaskView, CollectionPlanView) {
  let root_path = unique_temp_workspace(label);
  create_workspace("执行器测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "执行任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should create");
  let plan =
    save_collection_plan(&root_path, execution_plan_input(&task.id)).expect("plan should save");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan should confirm");
  (root_path, task, plan)
}

fn execution_plan_input(task_id: &str) -> SaveCollectionPlanInput {
  SaveCollectionPlanInput {
    task_id: task_id.to_string(),
    source: "form_generated".to_string(),
    plan_json: serde_json::json!({
      "platforms": ["tiktok"],
      "data_types": ["comments"],
      "region": "US",
      "time_range": "2026-07-01/2026-07-07",
      "steps": [{
        "endpoint_key": "tiktok.comments",
        "platform": "tiktok",
        "data_type": "comments",
        "params": {
          "item_id": "video-1",
          "region": "US",
          "time_range": "2026-07-01/2026-07-07"
        }
      }],
      "request_limit": 1,
      "missing_fields": [],
      "requires_user_confirmation": true
    }),
    validation_status: "valid".to_string(),
    validation_errors_json: None,
    cost_estimate_json: None,
  }
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
