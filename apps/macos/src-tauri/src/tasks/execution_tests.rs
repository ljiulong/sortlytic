use super::*;
use crate::workspace::create_workspace;

#[path = "execution_claim_tests.rs"]
mod execution_claim_tests;

#[path = "execution_completion_tests.rs"]
mod execution_completion_tests;

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
    true,
  )
  .expect("running task should fail");
  let failed_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(failed.status, "failed");
  assert!(failed.retryable);
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
fn enqueue_and_retry_materialize_complete_run_step_snapshots() {
  let (root_path, task, plan) = prepared_multi_step_task_workspace("execution-step-snapshot");

  let first_run = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let first_snapshot = run_step_snapshot(&root_path, &first_run.id);
  assert_eq!(
    first_snapshot
      .iter()
      .map(|(_, endpoint, status, started_at)| (
        endpoint.as_str(),
        status.as_str(),
        started_at.as_deref()
      ))
      .collect::<Vec<_>>(),
    vec![
      ("tiktok.account_profile", "pending", None),
      ("tiktok.item_detail", "pending", None),
    ]
  );
  assert_eq!(
    first_snapshot.len(),
    plan.plan_json["steps"]
      .as_array()
      .expect("plan steps should be an array")
      .len()
  );

  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  fail_task_run(
    &root_path,
    &running.id,
    "TIKHUB_REQUEST_ERROR",
    "网络超时",
    true,
  )
  .expect("run should become retryable");
  let retry = retry_task(&root_path, &task.id, None).expect("retry should enqueue");
  let retry_snapshot = run_step_snapshot(&root_path, &retry.id);

  assert_eq!(
    retry_snapshot
      .iter()
      .map(|(_, endpoint, status, started_at)| (
        endpoint.as_str(),
        status.as_str(),
        started_at.as_deref()
      ))
      .collect::<Vec<_>>(),
    vec![
      ("tiktok.account_profile", "pending", None),
      ("tiktok.item_detail", "pending", None),
    ]
  );
  assert!(first_snapshot.iter().all(|(first_id, _, _, _)| {
    retry_snapshot
      .iter()
      .all(|(retry_id, _, _, _)| retry_id != first_id)
  }));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn run_creation_rolls_back_when_any_step_snapshot_insert_fails() {
  for operation in ["enqueue", "retry"] {
    let (root_path, task, _) =
      prepared_multi_step_task_workspace(&format!("execution-step-snapshot-rollback-{operation}"));
    if operation == "retry" {
      enqueue_task(&root_path, &task.id).expect("first attempt should enqueue");
      let running = claim_next_task(&root_path)
        .expect("claim should succeed")
        .expect("first attempt should be claimed");
      fail_task_run(
        &root_path,
        &running.id,
        "TIKHUB_REQUEST_ERROR",
        "网络超时",
        true,
      )
      .expect("first attempt should become retryable");
    }

    let before = task_execution_mutation_state(&root_path, &task.id);
    open_workspace_connection(&root_path)
      .expect("database should open")
      .execute_batch(
        "CREATE TRIGGER fail_second_run_step_snapshot
         BEFORE INSERT ON task_run_step
         WHEN (
           SELECT COUNT(*) FROM task_run_step
           WHERE task_run_id = NEW.task_run_id
         ) >= 1
         BEGIN
           SELECT RAISE(ABORT, 'test run-step snapshot failure');
         END;",
      )
      .expect("failure trigger should install");

    let result = if operation == "enqueue" {
      enqueue_task(&root_path, &task.id)
    } else {
      retry_task(&root_path, &task.id, None)
    };
    result.expect_err("partial run-step snapshots must roll back the whole run creation");

    assert_eq!(task_execution_mutation_state(&root_path, &task.id), before);
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn non_retryable_failure_cannot_use_the_ordinary_retry_command() {
  let (root_path, task, _) = prepared_task_workspace("execution-non-retryable");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  fail_task_run(
    &root_path,
    &running.id,
    "UNCERTAIN_REQUEST_AFTER_CRASH",
    "远端请求状态不确定",
    false,
  )
  .expect("running task should fail");

  let error = retry_task(&root_path, &task.id, None)
    .expect_err("non-retryable failure must require an explicit override flow");
  assert!(error.message.contains("不可直接重试"));
  assert_eq!(task_run_count_and_state(&root_path, &task.id).0, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn failed_task_cannot_bypass_retry_policy_through_enqueue() {
  let (root_path, task, _) = prepared_task_workspace("execution-enqueue-retry-bypass");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  fail_task_run(
    &root_path,
    &running.id,
    "UNCERTAIN_REQUEST_AFTER_CRASH",
    "远端请求状态不确定",
    false,
  )
  .expect("running task should fail");

  let error = enqueue_task(&root_path, &task.id)
    .expect_err("failed task must not bypass the explicit retry policy through enqueue");
  assert!(error.message.contains("重试流程"));
  assert_eq!(task_run_count_and_state(&root_path, &task.id).0, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn ordinary_retry_cannot_forge_an_internal_recovery_directive() {
  for reserved_stage in [
    "恢复响应入库",
    "恢复重试",
    "恢复待发送",
    "恢复续页",
    "恢复收尾",
    "恢复等待",
  ] {
    let (root_path, task, _) =
      prepared_task_workspace(&format!("execution-forged-{}", Uuid::new_v4()));
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    fail_task_run(
      &root_path,
      &running.id,
      "TIKHUB_REQUEST_ERROR",
      "网络超时",
      true,
    )
    .expect("running task should fail");

    let error = retry_task(&root_path, &task.id, Some(format!(" {reserved_stage} ")))
      .expect_err("ordinary retry must not forge a recovery directive");
    assert!(error.message.contains("保留恢复阶段"));
    assert_eq!(task_run_count_and_state(&root_path, &task.id).0, 1);

    std::fs::remove_dir_all(root_path).ok();
  }
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
  replacement_input.plan_json["steps"][0]["params"]["keyword"] = serde_json::json!("truck");
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
  let (task, plan) = prepared_task_in_workspace(&root_path, "执行任务");
  (root_path, task, plan)
}

fn prepared_multi_step_task_workspace(
  label: &str,
) -> (std::path::PathBuf, CollectionTaskView, CollectionPlanView) {
  let root_path = unique_temp_workspace(label);
  create_workspace("执行器多步骤测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "多步骤执行任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["account_profile".to_string(), "item_detail".to_string()],
    },
  )
  .expect("multi-step task should create");
  let plan = save_collection_plan(
    &root_path,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: "form_generated".to_string(),
      plan_json: serde_json::json!({
        "platforms": ["tiktok"],
        "data_types": ["account_profile", "item_detail"],
        "region": null,
        "time_range": null,
        "steps": [
          {
            "endpoint_key": "tiktok.account_profile",
            "platform": "tiktok",
            "data_type": "account_profile",
            "params": { "account_id": "creator-1" }
          },
          {
            "endpoint_key": "tiktok.item_detail",
            "platform": "tiktok",
            "data_type": "item_detail",
            "params": { "item_id": "video-1" }
          }
        ],
        "record_limit": 2,
        "request_limit": 1,
        "budget_limit": {
          "currency": "USD",
          "amount_micros": 35_000_000
        },
        "missing_fields": [],
        "requires_user_confirmation": true
      }),
      validation_status: "valid".to_string(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )
  .expect("multi-step plan should save");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("multi-step plan should confirm");
  (root_path, task, plan)
}

fn prepared_task_in_workspace(
  root_path: &Path,
  name: &str,
) -> (CollectionTaskView, CollectionPlanView) {
  let task = create_collection_task(
    root_path,
    CreateCollectionTaskInput {
      name: name.to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("task should create");
  let plan =
    save_collection_plan(root_path, execution_plan_input(&task.id)).expect("plan should save");
  confirm_collection_plan(root_path, &task.id, &plan.id).expect("plan should confirm");
  (task, plan)
}

fn execution_plan_input(task_id: &str) -> SaveCollectionPlanInput {
  SaveCollectionPlanInput {
    task_id: task_id.to_string(),
    source: "form_generated".to_string(),
    plan_json: serde_json::json!({
      "platforms": ["tiktok"],
      "data_types": ["keyword_search"],
      "region": "US",
      "time_range": "近 30 天",
      "steps": [{
        "endpoint_key": "tiktok.keyword_search",
        "platform": "tiktok",
        "data_type": "keyword_search",
        "params": {
          "keyword": "car",
          "region": "US",
          "time_range": "近 30 天"
        }
      }],
      "record_limit": 1200,
      "request_limit": 1,
      "budget_limit": {
        "currency": "USD",
        "amount_micros": 35_000_000
      },
      "missing_fields": [],
      "requires_user_confirmation": true
    }),
    validation_status: "valid".to_string(),
    validation_errors_json: None,
    cost_estimate_json: None,
  }
}

fn set_run_step_status(root_path: &Path, run_id: &str, plan_id: &str, status: &str) -> String {
  let connection = open_workspace_connection(root_path).expect("database should open");
  let run_step_id = connection
    .query_row(
      "SELECT run_step.id
       FROM task_run_step AS run_step
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1 AND api_step.plan_id = ?2
       ORDER BY api_step.step_order, api_step.id
       LIMIT 1",
      params![run_id, plan_id],
      |row| row.get::<_, String>(0),
    )
    .expect("materialized run step should load");
  let changed = connection
    .execute(
      "UPDATE task_run_step
       SET status = ?1, started_at = '2026-07-13T08:00:00+00:00',
           updated_at = '2026-07-13T08:00:00+00:00'
       WHERE id = ?2 AND task_run_id = ?3",
      params![status, run_step_id, run_id],
    )
    .expect("run step status should update");
  assert_eq!(changed, 1);
  run_step_id
}

fn insert_requesting_checkpoint(root_path: &Path, run_step_id: &str) -> String {
  let checkpoint_id = Uuid::new_v4().to_string();
  open_workspace_connection(root_path)
    .expect("database should open")
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, status,
         request_attempt_count, cost_actual_json, requested_at, created_at, updated_at
       ) VALUES (
         ?1, ?2, 0, ?3, 'requesting', 1,
         '{\"currency\":\"USD\",\"amount_micros\":100}',
         '2026-07-13T08:01:00+00:00', '2026-07-13T08:01:00+00:00',
         '2026-07-13T08:01:00+00:00'
       )",
      params![checkpoint_id, run_step_id, Uuid::new_v4().to_string()],
    )
    .expect("requesting checkpoint should insert");
  checkpoint_id
}

fn insert_prepared_checkpoint(root_path: &Path, run_step_id: &str) {
  open_workspace_connection(root_path)
    .expect("database should open")
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, status, created_at, updated_at
       ) VALUES (?1, ?2, 0, ?3, 'prepared',
                 '2026-07-13T08:01:00+00:00', '2026-07-13T08:01:00+00:00')",
      params![
        Uuid::new_v4().to_string(),
        run_step_id,
        Uuid::new_v4().to_string()
      ],
    )
    .expect("prepared checkpoint should insert");
}

fn run_step_snapshot(
  root_path: &Path,
  run_id: &str,
) -> Vec<(String, String, String, Option<String>)> {
  let connection = open_workspace_connection(root_path).expect("database should open");
  let mut statement = connection
    .prepare(
      "SELECT run_step.id, api_step.endpoint_key, run_step.status, run_step.started_at
       FROM task_run_step AS run_step
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1
       ORDER BY api_step.step_order, api_step.id",
    )
    .expect("run-step snapshot query should prepare");
  statement
    .query_map(params![run_id], |row| {
      Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .expect("run-step snapshot should query")
    .collect::<rusqlite::Result<Vec<_>>>()
    .expect("run-step snapshot should load")
}

fn task_execution_mutation_state(
  root_path: &Path,
  task_id: &str,
) -> (i64, i64, i64, String, Option<String>) {
  open_workspace_connection(root_path)
    .expect("database should open")
    .query_row(
      "SELECT
         (SELECT COUNT(*) FROM task_run WHERE task_id = ?1),
         (SELECT COUNT(*) FROM task_run_step AS run_step
          JOIN task_run AS run ON run.id = run_step.task_run_id
          WHERE run.task_id = ?1),
         (SELECT COUNT(*) FROM task_log AS log
          JOIN task_run AS run ON run.id = log.task_run_id
          WHERE run.task_id = ?1),
         status, confirmed_at
       FROM collection_task WHERE id = ?1",
      params![task_id],
      |row| {
        Ok((
          row.get(0)?,
          row.get(1)?,
          row.get(2)?,
          row.get(3)?,
          row.get(4)?,
        ))
      },
    )
    .expect("task execution mutation state should load")
}

fn forge_plan_execution_contract(
  root_path: &Path,
  plan: &CollectionPlanView,
  schema_version: i64,
  corrupt_budget: bool,
) {
  let mut plan_json = plan.plan_json.clone();
  if corrupt_budget {
    plan_json
      .as_object_mut()
      .expect("plan fixture should be an object")
      .remove("budget_limit");
  }
  let connection = open_workspace_connection(root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_plan
       SET schema_version = ?1, plan_json = ?2, validation_status = 'valid'
       WHERE id = ?3",
      params![schema_version, plan_json.to_string(), plan.id],
    )
    .expect("test should forge the persisted plan contract");
}

fn assert_reconfirmation_quarantine(root_path: &Path, run_id: &str, task_id: &str, plan_id: &str) {
  let connection = open_workspace_connection(root_path).expect("database should reopen");
  let run = get_task_run(&connection, run_id).expect("quarantined run should load");
  let task = get_task_by_id(&connection, task_id).expect("quarantined task should load");
  let plan_state = connection
    .query_row(
      "SELECT validation_status, validation_errors_json, confirmed_by_user,
              (SELECT COUNT(*) FROM task_log
               WHERE task_run_id = ?2 AND message LIKE '%重新确认%')
       FROM collection_plan WHERE id = ?1",
      params![plan_id, run_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, i64>(2)?,
          row.get::<_, i64>(3)?,
        ))
      },
    )
    .expect("quarantine plan state should load");

  assert_eq!(run.status, "failed");
  assert!(run.ended_at.is_some());
  assert!(run.claimed_at.is_none());
  assert_eq!(
    run.error_code.as_deref(),
    Some("PLAN_RECONFIRMATION_REQUIRED")
  );
  assert!(!run.retryable);
  assert_eq!(task.status, "waiting_confirmation");
  assert!(task.confirmed_at.is_none());
  assert_eq!(plan_state.0, "needs_review");
  assert!(plan_state.1.contains("重新确认"));
  assert_eq!(plan_state.2, 0);
  assert_eq!(plan_state.3, 1);
}

fn task_run_count_and_state(root_path: &Path, task_id: &str) -> (i64, String, Option<String>) {
  let connection = open_workspace_connection(root_path).expect("database should reopen");
  connection
    .query_row(
      "SELECT
         (SELECT COUNT(*) FROM task_run WHERE task_id = ?1),
         status, confirmed_at
       FROM collection_task WHERE id = ?1",
      params![task_id],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .expect("task run count and state should load")
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("smart-data-workbench-{label}-{}", Uuid::new_v4()))
}
