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
fn interrupted_running_task_without_step_snapshot_fails_closed() {
  let (root_path, task, plan) = prepared_task_workspace("execution-recovery");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let first_claim = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let deleted = open_workspace_connection(&root_path)
    .expect("database should open")
    .execute(
      "DELETE FROM task_run_step WHERE task_run_id = ?1",
      params![first_claim.id],
    )
    .expect("test should remove the materialized run-step snapshot");
  assert_eq!(deleted, 1);

  let recovered = recover_interrupted_runs(&root_path).expect("recovery should succeed");
  let recovered_run = get_task_run(
    &open_workspace_connection(&root_path).expect("database should open"),
    &first_claim.id,
  )
  .expect("run should reload");
  let recovered_task = get_task(&root_path, &task.id).expect("task should load");

  assert_eq!(recovered, 0);
  assert_eq!(recovered_run.status, "failed");
  assert_eq!(recovered_run.plan_id.as_deref(), Some(plan.id.as_str()));
  assert_eq!(recovered_run.attempt_number, 1);
  assert!(recovered_run.claimed_at.is_none());
  assert_eq!(
    recovered_run.error_code.as_deref(),
    Some("CHECKPOINT_EVIDENCE_INCOMPLETE")
  );
  assert_eq!(recovered_task.status, "failed");

  assert!(claim_next_task(&root_path)
    .expect("failed recovery should not break queue scanning")
    .is_none());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn freshly_claimed_run_before_first_request_is_safely_requeued() {
  let (root_path, task, plan) = prepared_task_workspace("execution-recovery-before-request");
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let claimed = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  assert_eq!(claimed.id, queued.id);

  let recovered_count =
    recover_interrupted_runs(&root_path).expect("pre-request recovery should succeed");
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let recovered = get_task_run(&connection, &claimed.id).expect("recovered run should load");
  let recovered_task = get_task_by_id(&connection, &task.id).expect("task should load");
  let snapshot_count = connection
    .query_row(
      "SELECT COUNT(*) FROM task_run_step AS run_step
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1 AND api_step.plan_id = ?2
         AND run_step.status = 'pending'",
      params![claimed.id, plan.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("pending snapshot count should load");

  assert_eq!(recovered_count, 1);
  assert_eq!(snapshot_count, 1);
  assert_eq!(recovered.status, "queued");
  assert_eq!(recovered.current_stage.as_deref(), Some("恢复待发送"));
  assert!(recovered.claimed_at.is_none());
  assert!(recovered.error_code.is_none());
  assert_eq!(recovered_task.status, "queued");
  drop(connection);

  let reclaimed = claim_next_task(&root_path)
    .expect("reclaim should succeed")
    .expect("recovered task should remain claimable");
  assert_eq!(reclaimed.id, claimed.id);
  assert_eq!(reclaimed.current_stage.as_deref(), Some("恢复待发送"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn recovered_prepared_checkpoint_is_revalidated_and_claimed() {
  let (root_path, task, plan) = prepared_task_workspace("execution-recovery-prepared-claim");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let run_step_id = set_run_step_status(&root_path, &running.id, &plan.id, "running");
  insert_prepared_checkpoint(&root_path, &run_step_id);

  assert_eq!(
    recover_interrupted_runs(&root_path).expect("prepared checkpoint should recover"),
    1
  );
  let recovered = get_task_run(
    &open_workspace_connection(&root_path).expect("database should reopen"),
    &running.id,
  )
  .expect("recovered run should load");
  assert_eq!(recovered.status, "queued");
  assert_eq!(recovered.current_stage.as_deref(), Some("恢复待发送"));

  let reclaimed = claim_next_task(&root_path)
    .expect("recovery evidence should be revalidated")
    .expect("valid prepared recovery should be claimed");
  assert_eq!(reclaimed.id, running.id);
  assert_eq!(reclaimed.current_stage.as_deref(), Some("恢复待发送"));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn claim_rejects_a_forged_recovery_directive_without_evidence() {
  let (root_path, task, plan) = prepared_task_workspace("execution-recovery-directive");
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  open_workspace_connection(&root_path)
    .expect("database should open")
    .execute(
      "UPDATE task_run SET current_stage = '恢复响应入库' WHERE id = ?1",
      params![queued.id],
    )
    .expect("recovery directive should persist");

  assert!(claim_next_task(&root_path)
    .expect("forged recovery directive should be quarantined")
    .is_none());
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let quarantined = get_task_run(&connection, &queued.id).expect("run should load");
  let quarantined_task = get_task_by_id(&connection, &task.id).expect("task should load");
  let confirmed = connection
    .query_row(
      "SELECT confirmed_by_user FROM collection_plan WHERE id = ?1",
      params![plan.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("plan confirmation should load");
  assert_eq!(quarantined.status, "failed");
  assert_eq!(
    quarantined.error_code.as_deref(),
    Some("CHECKPOINT_STATE_CONFLICT")
  );
  assert_eq!(quarantined_task.status, "failed");
  assert_eq!(confirmed, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn claim_rolls_back_when_parent_task_is_not_queued() {
  for parent_status in ["running", "success", "cancelled"] {
    let (root_path, task, plan) =
      prepared_task_workspace(&format!("execution-claim-parent-{parent_status}"));
    let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
    open_workspace_connection(&root_path)
      .expect("database should open")
      .execute(
        "UPDATE collection_task SET status = ?1 WHERE id = ?2",
        params![parent_status, task.id],
      )
      .expect("parent status should be forged");

    claim_next_task(&root_path).expect_err("claim must reject a non-queued parent task");
    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    let run = get_task_run(&connection, &queued.id).expect("run should load");
    let parent = get_task_by_id(&connection, &task.id).expect("parent task should load");
    let confirmed = connection
      .query_row(
        "SELECT confirmed_by_user FROM collection_plan WHERE id = ?1",
        params![plan.id],
        |row| row.get::<_, i64>(0),
      )
      .expect("plan confirmation should load");
    assert_eq!(run.status, "queued");
    assert!(run.claimed_at.is_none());
    assert_eq!(parent.status, parent_status);
    assert_eq!(confirmed, 1);

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn running_step_without_a_checkpoint_fails_closed() {
  let (root_path, task, plan) = prepared_task_workspace("execution-running-step-no-checkpoint");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  set_run_step_status(&root_path, &running.id, &plan.id, "running");

  assert_eq!(
    recover_interrupted_runs(&root_path).expect("recovery should fail closed"),
    0
  );
  let recovered = get_task_run(
    &open_workspace_connection(&root_path).expect("database should open"),
    &running.id,
  )
  .expect("run should load");
  assert_eq!(recovered.status, "failed");
  assert_eq!(
    recovered.error_code.as_deref(),
    Some("CHECKPOINT_EVIDENCE_INCOMPLETE")
  );

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
  replacement_input.plan_json["steps"][0]["params"]["keyword"] = serde_json::json!("truck");
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
  let legacy = get_task_run(&connection, "legacy-run").expect("legacy run should load");
  let legacy_task = get_task_by_id(&connection, &task.id).expect("legacy task should load");
  assert_eq!(legacy.status, "failed");
  assert_eq!(
    legacy.error_code.as_deref(),
    Some("RUN_SNAPSHOT_REQUIRES_REVIEW")
  );
  assert!(!legacy.retryable);
  assert_eq!(legacy_task.status, "failed");
  drop(connection);

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
fn enqueue_rejects_non_v2_or_corrupted_confirmed_plans_without_mutation() {
  for (label, schema_version, corrupt_budget) in [
    ("execution-enqueue-v1", 1, false),
    ("execution-enqueue-unknown", 3, false),
    ("execution-enqueue-corrupted-v2", 2, true),
  ] {
    let (root_path, task, plan) = prepared_task_workspace(label);
    forge_plan_execution_contract(&root_path, &plan, schema_version, corrupt_budget);

    let error =
      enqueue_task(&root_path, &task.id).expect_err("non-v2 or corrupted plan must not enqueue");
    assert!(error.message.contains("v2") || error.message.contains("采集计划"));

    let state = task_run_count_and_state(&root_path, &task.id);
    assert_eq!(state.0, 0);
    assert_eq!(state.1, "waiting_confirmation");
    assert!(state.2.is_some());

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn enqueue_rejects_plan_and_step_snapshot_divergence_without_mutation() {
  for (label, mutation) in [
    ("execution-plan-json-divergence", "plan_json"),
    ("execution-step-params-divergence", "step_params"),
    ("execution-step-metadata-divergence", "step_metadata"),
  ] {
    let (root_path, task, plan) = prepared_task_workspace(label);
    let connection = open_workspace_connection(&root_path).expect("database should open");
    match mutation {
      "plan_json" => {
        let mut changed_plan = plan.plan_json.clone();
        changed_plan["steps"][0]["params"]["keyword"] = serde_json::json!("truck");
        connection
          .execute(
            "UPDATE collection_plan SET plan_json = ?1 WHERE id = ?2",
            params![changed_plan.to_string(), plan.id],
          )
          .expect("test should change the confirmed plan body");
      }
      "step_params" => {
        connection
          .execute(
            "UPDATE api_call_step SET params_json = ?1 WHERE plan_id = ?2",
            params![
              serde_json::json!({
                "keyword": "truck",
                "region": "US",
                "time_range": "近 30 天"
              })
              .to_string(),
              plan.id
            ],
          )
          .expect("test should change persisted step parameters");
      }
      "step_metadata" => {
        connection
          .execute(
            "UPDATE api_call_step
             SET endpoint_key = 'tiktok.comments', status = 'success',
                 request_count_estimate = 99
             WHERE plan_id = ?1",
            params![plan.id],
          )
          .expect("test should change persisted step metadata");
      }
      _ => unreachable!("test mutation should be known"),
    }
    drop(connection);

    let error = enqueue_task(&root_path, &task.id)
      .expect_err("divergent plan and step snapshots must not enqueue");
    assert!(error.message.contains("步骤") || error.message.contains("采集计划"));

    let state = task_run_count_and_state(&root_path, &task.id);
    assert_eq!((state.0, state.1), (0, "waiting_confirmation".to_string()));

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn retry_rejects_non_v2_or_corrupted_failed_plan_without_mutation() {
  for (label, schema_version, corrupt_budget) in [
    ("execution-retry-v1", 1, false),
    ("execution-retry-corrupted-v2", 2, true),
  ] {
    let (root_path, task, plan) = prepared_task_workspace(label);
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("task should be claimed");
    fail_task_run(
      &root_path,
      &running.id,
      "TIKHUB_REQUEST_ERROR",
      "网络超时",
      true,
    )
    .expect("run should fail");
    forge_plan_execution_contract(&root_path, &plan, schema_version, corrupt_budget);

    let error = retry_task(&root_path, &task.id, None)
      .expect_err("non-v2 or corrupted failed plan must not create a retry");
    assert!(error.message.contains("v2") || error.message.contains("采集计划"));

    let state = task_run_count_and_state(&root_path, &task.id);
    assert_eq!((state.0, state.1), (1, "failed".to_string()));

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn claim_quarantines_a_queued_v1_run_and_continues_to_valid_v2() {
  let (root_path, task, plan) = prepared_task_workspace("execution-claim-v1");
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  forge_plan_execution_contract(&root_path, &plan, 1, false);
  let (valid_task, _) = prepared_task_in_workspace(&root_path, "后续有效任务");
  let valid_queued = enqueue_task(&root_path, &valid_task.id).expect("valid task should enqueue");

  let claimed = claim_next_task(&root_path)
    .expect("claim should isolate legacy run")
    .expect("valid v2 run behind legacy run should be claimed");
  assert_eq!(claimed.id, valid_queued.id);
  assert_reconfirmation_quarantine(&root_path, &queued.id, &task.id, &plan.id);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn claim_quarantines_a_corrupted_v2_run() {
  let (root_path, task, plan) = prepared_task_workspace("execution-claim-corrupted-v2");
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let run_step_id = set_run_step_status(&root_path, &queued.id, &plan.id, "running");
  insert_prepared_checkpoint(&root_path, &run_step_id);
  forge_plan_execution_contract(&root_path, &plan, 2, true);

  assert!(claim_next_task(&root_path)
    .expect("claim should isolate corrupted v2 run")
    .is_none());
  assert_reconfirmation_quarantine(&root_path, &queued.id, &task.id, &plan.id);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn invalid_plan_with_tainted_prepared_checkpoint_requires_manual_review() {
  let (root_path, task, plan) = prepared_task_workspace("execution-invalid-plan-tainted-prepared");
  let run = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let run_step_id = set_run_step_status(&root_path, &run.id, &plan.id, "running");
  insert_prepared_checkpoint(&root_path, &run_step_id);
  open_workspace_connection(&root_path)
    .expect("database should open")
    .execute(
      "UPDATE collection_page_checkpoint
       SET retryable = 1, last_error_code = 'TIKHUB_REQUEST_ERROR',
           last_error_message = '历史错误'
       WHERE task_run_step_id = ?1",
      params![run_step_id],
    )
    .expect("prepared checkpoint should be tainted");
  forge_plan_execution_contract(&root_path, &plan, 2, true);

  assert!(claim_next_task(&root_path)
    .expect("tainted prepared state should stop")
    .is_none());
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let stopped = get_task_run(&connection, &run.id).expect("run should load");
  let stopped_task = get_task_by_id(&connection, &task.id).expect("task should load");
  assert_eq!(
    stopped.error_code.as_deref(),
    Some("RUN_SNAPSHOT_REQUIRES_REVIEW")
  );
  assert_eq!(stopped_task.status, "failed");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn claim_fails_closed_on_incomplete_run_step_snapshot_and_continues_queue() {
  let (root_path, damaged_task, damaged_plan) =
    prepared_multi_step_task_workspace("execution-claim-incomplete-snapshot");
  let damaged_run =
    enqueue_task(&root_path, &damaged_task.id).expect("damaged task should enqueue");
  let (healthy_task, healthy_plan) = prepared_task_in_workspace(&root_path, "后续完整任务");
  let healthy_run =
    enqueue_task(&root_path, &healthy_task.id).expect("healthy task should enqueue");
  let deleted_step_id =
    set_run_step_status(&root_path, &damaged_run.id, &damaged_plan.id, "running");
  let deleted_checkpoint_id = insert_requesting_checkpoint(&root_path, &deleted_step_id);

  let connection = open_workspace_connection(&root_path).expect("database should open");
  let deleted = connection
    .execute(
      "DELETE FROM task_run_step WHERE id = ?1 AND task_run_id = ?2",
      params![deleted_step_id, damaged_run.id],
    )
    .expect("the run step carrying request evidence should be removable for corruption simulation");
  assert_eq!(deleted, 1);
  let cascaded_checkpoint_count = connection
    .query_row(
      "SELECT COUNT(*) FROM collection_page_checkpoint WHERE id = ?1",
      params![deleted_checkpoint_id],
      |row| row.get::<_, i64>(0),
    )
    .expect("cascaded checkpoint count should load");
  assert_eq!(cascaded_checkpoint_count, 0);
  connection
    .execute(
      "UPDATE task_run SET started_at = '2026-07-13T08:00:00+00:00' WHERE id = ?1",
      params![damaged_run.id],
    )
    .expect("damaged run order should be fixed");
  connection
    .execute(
      "UPDATE task_run SET started_at = '2026-07-13T08:01:00+00:00' WHERE id = ?1",
      params![healthy_run.id],
    )
    .expect("healthy run order should be fixed");
  drop(connection);

  let claimed = claim_next_task(&root_path)
    .expect("claim should quarantine an incomplete snapshot")
    .expect("healthy run behind a damaged run should be claimed");
  assert_eq!(claimed.id, healthy_run.id);
  assert_eq!(claimed.plan_id.as_deref(), Some(healthy_plan.id.as_str()));

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let quarantined = get_task_run(&connection, &damaged_run.id).expect("damaged run should load");
  let quarantined_task =
    get_task_by_id(&connection, &damaged_task.id).expect("damaged task should load");
  let plan_state = connection
    .query_row(
      "SELECT validation_status, confirmed_by_user, plan_json
       FROM collection_plan WHERE id = ?1",
      params![damaged_plan.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, String>(2)?,
        ))
      },
    )
    .expect("damaged run plan should load");
  let remaining_step = connection
    .query_row(
      "SELECT status, stop_reason, completed_at
       FROM task_run_step WHERE task_run_id = ?1",
      params![damaged_run.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .expect("remaining run step should load");
  let safety_log_count = connection
    .query_row(
      "SELECT COUNT(*) FROM task_log
       WHERE task_run_id = ?1 AND stage = '运行快照不完整'
         AND message = '运行步骤快照不完整，可能丢失远端请求证据，已停止自动执行'",
      params![damaged_run.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("snapshot quarantine log should count");

  assert_eq!(quarantined.status, "failed");
  assert_eq!(quarantined.current_stage.as_deref(), Some("运行快照不完整"));
  assert_eq!(
    quarantined.error_code.as_deref(),
    Some("RUN_STEP_SNAPSHOT_INCOMPLETE")
  );
  assert!(!quarantined.retryable);
  assert!(quarantined.ended_at.is_some());
  assert!(quarantined.claimed_at.is_none());
  assert_eq!(quarantined_task.status, "failed");
  assert!(quarantined_task.confirmed_at.is_some());
  assert_eq!(plan_state.0, "valid");
  assert_eq!(plan_state.1, 1);
  assert_eq!(
    serde_json::from_str::<Value>(&plan_state.2).expect("plan JSON should remain valid"),
    damaged_plan.plan_json
  );
  assert_eq!(remaining_step.0, "failed");
  assert_eq!(remaining_step.1.as_deref(), Some("terminal_error"));
  assert!(remaining_step.2.is_some());
  assert_eq!(safety_log_count, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn claim_rejects_untrusted_queued_run_step_states() {
  for (label, status, stop_reason, completed_at, expected_code) in [
    (
      "failed",
      "failed",
      Some("terminal_error"),
      Some("2026-07-13T08:00:01+00:00"),
      "CHECKPOINT_TERMINAL_FAILURE",
    ),
    (
      "cancelled",
      "cancelled",
      Some("user_cancelled"),
      Some("2026-07-13T08:00:01+00:00"),
      "CHECKPOINT_TERMINAL_FAILURE",
    ),
    (
      "running",
      "running",
      None,
      None,
      "CHECKPOINT_EVIDENCE_INCOMPLETE",
    ),
    (
      "success",
      "success",
      None,
      Some("2026-07-13T08:00:01+00:00"),
      "CHECKPOINT_STATE_CONFLICT",
    ),
  ] {
    let (root_path, task, plan) =
      prepared_task_workspace(&format!("execution-claim-untrusted-{label}"));
    let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    let changed = connection
      .execute(
        "UPDATE task_run_step
         SET status = ?1, stop_reason = ?2,
             started_at = '2026-07-13T08:00:00+00:00', completed_at = ?3,
             updated_at = '2026-07-13T08:00:01+00:00'
         WHERE task_run_id = ?4",
        params![status, stop_reason, completed_at, queued.id],
      )
      .expect("run-step state should be forged for fail-closed coverage");
    assert_eq!(changed, 1);
    drop(connection);

    assert!(claim_next_task(&root_path)
      .expect("untrusted queued state should be quarantined")
      .is_none());
    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    let quarantined = get_task_run(&connection, &queued.id).expect("run should load");
    let quarantined_task = get_task_by_id(&connection, &task.id).expect("task should load");
    let confirmed = connection
      .query_row(
        "SELECT confirmed_by_user FROM collection_plan WHERE id = ?1",
        params![plan.id],
        |row| row.get::<_, i64>(0),
      )
      .expect("plan confirmation should load");
    assert_eq!(quarantined.status, "failed");
    assert_eq!(quarantined.error_code.as_deref(), Some(expected_code));
    assert!(!quarantined.retryable);
    assert_eq!(quarantined_task.status, "failed");
    assert_eq!(confirmed, 1);

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn claim_prioritizes_requesting_uncertainty_over_invalid_incomplete_snapshot() {
  let (root_path, task, plan) =
    prepared_multi_step_task_workspace("execution-claim-incomplete-requesting");
  let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let run_step_id = set_run_step_status(&root_path, &queued.id, &plan.id, "running");
  let checkpoint_id = insert_requesting_checkpoint(&root_path, &run_step_id);
  let deleted = open_workspace_connection(&root_path)
    .expect("database should open")
    .execute(
      "DELETE FROM task_run_step WHERE task_run_id = ?1 AND id <> ?2",
      params![queued.id, run_step_id],
    )
    .expect("the other run step should be removed");
  assert_eq!(deleted, 1);
  forge_plan_execution_contract(&root_path, &plan, 2, true);

  assert!(claim_next_task(&root_path)
    .expect("requesting evidence should be quarantined")
    .is_none());
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let quarantined = get_task_run(&connection, &queued.id).expect("run should load");
  let quarantined_task = get_task_by_id(&connection, &task.id).expect("task should load");
  let checkpoint = connection
    .query_row(
      "SELECT status, retryable, last_error_code FROM collection_page_checkpoint
       WHERE id = ?1",
      params![checkpoint_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .expect("checkpoint should load");
  let run_step = connection
    .query_row(
      "SELECT status, stop_reason FROM task_run_step WHERE id = ?1",
      params![run_step_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .expect("run step should load");
  let confirmed = connection
    .query_row(
      "SELECT confirmed_by_user FROM collection_plan WHERE id = ?1",
      params![plan.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("plan confirmation should load");

  assert_eq!(quarantined.status, "failed");
  assert_eq!(
    quarantined.error_code.as_deref(),
    Some("UNCERTAIN_REQUEST_AFTER_CRASH")
  );
  assert_eq!(quarantined_task.status, "failed");
  assert_eq!(checkpoint.0, "uncertain");
  assert_eq!(checkpoint.1, 0);
  assert_eq!(
    checkpoint.2.as_deref(),
    Some("UNCERTAIN_REQUEST_AFTER_CRASH")
  );
  assert_eq!(run_step.0, "failed");
  assert_eq!(run_step.1.as_deref(), Some("uncertain_request"));
  assert_eq!(confirmed, 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn invalid_plan_with_sent_request_evidence_requires_manual_review() {
  for run_state in ["queued", "running"] {
    let (root_path, task, plan) =
      prepared_task_workspace(&format!("execution-invalid-plan-sent-{run_state}"));
    let queued = enqueue_task(&root_path, &task.id).expect("task should enqueue");
    if run_state == "running" {
      claim_next_task(&root_path)
        .expect("claim should succeed")
        .expect("queued task should be claimed");
    }
    let run_step_id = set_run_step_status(&root_path, &queued.id, &plan.id, "running");
    let checkpoint_id = insert_requesting_checkpoint(&root_path, &run_step_id);
    open_workspace_connection(&root_path)
      .expect("database should open")
      .execute(
        "UPDATE collection_page_checkpoint
         SET status = 'failed', last_error_code = 'TIKHUB_REQUEST_ERROR', retryable = 1
         WHERE id = ?1",
        params![checkpoint_id],
      )
      .expect("sent request evidence should become a retryable failure");
    forge_plan_execution_contract(&root_path, &plan, 2, true);

    if run_state == "queued" {
      assert!(claim_next_task(&root_path)
        .expect("sent request evidence should force manual review")
        .is_none());
    } else {
      assert_eq!(
        recover_interrupted_runs(&root_path).expect("recovery should force manual review"),
        0
      );
    }
    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    let quarantined = get_task_run(&connection, &queued.id).expect("run should load");
    let quarantined_task = get_task_by_id(&connection, &task.id).expect("task should load");
    let evidence = connection
      .query_row(
        "SELECT checkpoint.status, checkpoint.request_attempt_count,
                checkpoint.requested_at, plan.validation_status, plan.confirmed_by_user
         FROM collection_page_checkpoint AS checkpoint
         JOIN collection_plan AS plan ON plan.id = ?2
         WHERE checkpoint.id = ?1",
        params![checkpoint_id, plan.id],
        |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
          ))
        },
      )
      .expect("request and plan evidence should load");
    assert_eq!(quarantined.status, "failed");
    assert_eq!(
      quarantined.error_code.as_deref(),
      Some("REQUEST_EVIDENCE_REQUIRES_REVIEW")
    );
    assert!(!quarantined.retryable);
    assert_eq!(quarantined_task.status, "failed");
    assert_eq!(evidence.0, "failed");
    assert_eq!(evidence.1, 1);
    assert!(evidence.2.is_some());
    assert_eq!(evidence.3, "needs_review");
    assert_eq!(evidence.4, 0);

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn invalid_plan_with_cascaded_request_evidence_requires_manual_review() {
  for run_state in ["queued", "running"] {
    let (root_path, task, plan) =
      prepared_multi_step_task_workspace(&format!("execution-invalid-plan-cascade-{run_state}"));
    let run = enqueue_task(&root_path, &task.id).expect("task should enqueue");
    if run_state == "running" {
      claim_next_task(&root_path)
        .expect("claim should succeed")
        .expect("queued task should be claimed");
    }
    let deleted_step_id = set_run_step_status(&root_path, &run.id, &plan.id, "running");
    let checkpoint_id = insert_requesting_checkpoint(&root_path, &deleted_step_id);
    let connection = open_workspace_connection(&root_path).expect("database should open");
    assert_eq!(
      connection
        .execute(
          "DELETE FROM task_run_step WHERE id = ?1",
          params![deleted_step_id]
        )
        .expect("evidence-carrying step should delete"),
      1
    );
    assert_eq!(
      connection
        .query_row(
          "SELECT COUNT(*) FROM collection_page_checkpoint WHERE id = ?1",
          params![checkpoint_id],
          |row| row.get::<_, i64>(0),
        )
        .expect("checkpoint count should load"),
      0
    );
    drop(connection);
    forge_plan_execution_contract(&root_path, &plan, 2, true);

    if run_state == "queued" {
      assert!(claim_next_task(&root_path)
        .expect("incomplete snapshot should stop")
        .is_none());
    } else {
      assert_eq!(
        recover_interrupted_runs(&root_path).expect("incomplete recovery should stop"),
        0
      );
    }
    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    let stopped = get_task_run(&connection, &run.id).expect("run should load");
    let stopped_task = get_task_by_id(&connection, &task.id).expect("task should load");
    let plan_state = connection
      .query_row(
        "SELECT validation_status, confirmed_by_user FROM collection_plan WHERE id = ?1",
        params![plan.id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
      )
      .expect("plan state should load");
    assert_eq!(stopped.status, "failed");
    assert_eq!(
      stopped.error_code.as_deref(),
      Some("RUN_SNAPSHOT_REQUIRES_REVIEW")
    );
    assert!(!stopped.retryable);
    assert_eq!(stopped_task.status, "failed");
    assert_eq!(plan_state, ("needs_review".to_string(), 0));

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn invalid_plan_with_orphan_continuation_requires_manual_review() {
  let (root_path, task, plan) = prepared_task_workspace("execution-invalid-plan-orphan-cursor");
  let run = enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let run_step_id = set_run_step_status(&root_path, &run.id, &plan.id, "running");
  open_workspace_connection(&root_path)
    .expect("database should open")
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, input_cursor_json,
         status, created_at, updated_at
       ) VALUES (?1, ?2, 1, ?3, '{\"cursor\":\"next\"}', 'prepared',
                 '2026-07-13T08:01:00+00:00', '2026-07-13T08:01:00+00:00')",
      params![
        Uuid::new_v4().to_string(),
        run_step_id,
        Uuid::new_v4().to_string()
      ],
    )
    .expect("orphan continuation checkpoint should insert");
  forge_plan_execution_contract(&root_path, &plan, 2, true);

  assert!(claim_next_task(&root_path)
    .expect("orphan continuation should stop")
    .is_none());
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let stopped = get_task_run(&connection, &run.id).expect("run should load");
  let stopped_task = get_task_by_id(&connection, &task.id).expect("task should load");
  assert_eq!(
    stopped.error_code.as_deref(),
    Some("REQUEST_EVIDENCE_REQUIRES_REVIEW")
  );
  assert_eq!(stopped_task.status, "failed");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn incomplete_snapshot_quarantine_is_atomic_on_parent_or_log_failure() {
  for failure_mode in ["parent_state", "log_trigger"] {
    let (root_path, task, plan) = prepared_multi_step_task_workspace(&format!(
      "execution-claim-quarantine-rollback-{failure_mode}"
    ));
    let damaged_run = enqueue_task(&root_path, &task.id).expect("damaged task should enqueue");
    let (healthy_task, _) = prepared_task_in_workspace(&root_path, "回滚后的健康任务");
    let healthy_run =
      enqueue_task(&root_path, &healthy_task.id).expect("healthy task should enqueue");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    let deleted = connection
      .execute(
        "DELETE FROM task_run_step WHERE id = (
           SELECT id FROM task_run_step WHERE task_run_id = ?1 ORDER BY id DESC LIMIT 1
         )",
        params![damaged_run.id],
      )
      .expect("one run step should be removed");
    assert_eq!(deleted, 1);
    connection
      .execute(
        "UPDATE task_run SET started_at = '2026-07-13T08:00:00+00:00' WHERE id = ?1",
        params![damaged_run.id],
      )
      .expect("damaged run order should be fixed");
    connection
      .execute(
        "UPDATE task_run SET started_at = '2026-07-13T08:01:00+00:00' WHERE id = ?1",
        params![healthy_run.id],
      )
      .expect("healthy run order should be fixed");
    if failure_mode == "parent_state" {
      connection
        .execute(
          "UPDATE collection_task SET status = 'running' WHERE id = ?1",
          params![task.id],
        )
        .expect("parent task should be forged");
    } else {
      connection
        .execute_batch(
          "CREATE TRIGGER fail_snapshot_safety_log
           BEFORE INSERT ON task_log
           WHEN NEW.stage = '运行快照不完整'
           BEGIN
             SELECT RAISE(ABORT, 'test snapshot safety log failure');
           END;",
        )
        .expect("log failure trigger should install");
    }
    drop(connection);
    let damaged_before = task_execution_mutation_state(&root_path, &task.id);
    let healthy_before = task_execution_mutation_state(&root_path, &healthy_task.id);

    claim_next_task(&root_path)
      .expect_err("quarantine failure must roll back the claim transaction");

    assert_eq!(
      task_execution_mutation_state(&root_path, &task.id),
      damaged_before
    );
    assert_eq!(
      task_execution_mutation_state(&root_path, &healthy_task.id),
      healthy_before
    );
    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    let damaged_after =
      get_task_run(&connection, &damaged_run.id).expect("damaged run should load");
    let healthy_after =
      get_task_run(&connection, &healthy_run.id).expect("healthy run should load");
    let confirmed = connection
      .query_row(
        "SELECT confirmed_by_user FROM collection_plan WHERE id = ?1",
        params![plan.id],
        |row| row.get::<_, i64>(0),
      )
      .expect("plan confirmation should load");
    assert_eq!(damaged_after.status, "queued");
    assert_eq!(healthy_after.status, "queued");
    assert_eq!(confirmed, 1);

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn recovery_requires_manual_review_for_invalid_plan_with_unsafe_run_state() {
  for (label, schema_version, corrupt_budget) in [
    ("execution-recover-v1", 1, false),
    ("execution-recover-corrupted-v2", 2, true),
  ] {
    let (root_path, task, plan) = prepared_task_workspace(label);
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("task should be claimed");
    let run_step_id = set_run_step_status(&root_path, &running.id, &plan.id, "running");
    forge_plan_execution_contract(&root_path, &plan, schema_version, corrupt_budget);

    assert_eq!(
      recover_interrupted_runs(&root_path).expect("recovery should isolate invalid run"),
      0
    );
    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    let stopped = get_task_run(&connection, &running.id).expect("run should load");
    let stopped_task = get_task_by_id(&connection, &task.id).expect("task should load");
    let plan_state = connection
      .query_row(
        "SELECT validation_status, confirmed_by_user FROM collection_plan WHERE id = ?1",
        params![plan.id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
      )
      .expect("plan state should load");
    let run_step = connection
      .query_row(
        "SELECT status, stop_reason, completed_at
         FROM task_run_step WHERE id = ?1",
        params![run_step_id],
        |row| {
          Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
          ))
        },
      )
      .expect("run step should load");
    assert_eq!(stopped.status, "failed");
    assert_eq!(
      stopped.error_code.as_deref(),
      Some("RUN_SNAPSHOT_REQUIRES_REVIEW")
    );
    assert!(!stopped.retryable);
    assert_eq!(stopped_task.status, "failed");
    assert!(stopped_task.confirmed_at.is_some());
    assert_eq!(plan_state, ("needs_review".to_string(), 0));
    assert_eq!(run_step.0, "failed");
    assert_eq!(run_step.1.as_deref(), Some("terminal_error"));
    assert!(run_step.2.is_some());

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn requesting_checkpoint_becomes_uncertain_before_invalid_plan_quarantine() {
  let (root_path, task, plan) = prepared_task_workspace("execution-invalid-plan-requesting");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let run_step_id = set_run_step_status(&root_path, &running.id, &plan.id, "running");
  let checkpoint_id = insert_requesting_checkpoint(&root_path, &run_step_id);
  forge_plan_execution_contract(&root_path, &plan, 2, true);

  assert_eq!(
    recover_interrupted_runs(&root_path).expect("recovery should preserve uncertain evidence"),
    0
  );
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let recovered = get_task_run(&connection, &running.id).expect("run should load");
  let checkpoint = connection
    .query_row(
      "SELECT status, last_error_code FROM collection_page_checkpoint WHERE id = ?1",
      params![checkpoint_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .expect("checkpoint should load");
  let run_step = connection
    .query_row(
      "SELECT status, stop_reason FROM task_run_step WHERE id = ?1",
      params![run_step_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .expect("run step should load");

  assert_eq!(recovered.status, "failed");
  assert_eq!(
    recovered.error_code.as_deref(),
    Some("UNCERTAIN_REQUEST_AFTER_CRASH")
  );
  assert_eq!(checkpoint.0, "uncertain");
  assert_eq!(
    checkpoint.1.as_deref(),
    Some("UNCERTAIN_REQUEST_AFTER_CRASH")
  );
  assert_eq!(
    run_step,
    ("failed".to_string(), Some("uncertain_request".to_string()))
  );

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
