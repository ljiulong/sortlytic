use std::path::{Path, PathBuf};

use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::super::{
  claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task, get_task,
  get_task_run, open_workspace_connection, save_collection_plan, CollectionPlanView,
  CollectionTaskView, CreateCollectionTaskInput, SaveCollectionPlanInput, TaskRunView,
};
use super::*;
use crate::workspace::create_workspace;

const T0: &str = "2026-07-13T08:00:00+00:00";
const T1: &str = "2026-07-13T08:01:00+00:00";

struct RunningFixture {
  root_path: PathBuf,
  task: CollectionTaskView,
  plan: CollectionPlanView,
  run: TaskRunView,
}

#[test]
fn requesting_checkpoint_becomes_uncertain_and_is_not_requeued() {
  let fixture = running_fixture("recover-requesting", 3, 100, 1_000);
  let checkpoint_id = insert_checkpoint(
    &fixture,
    "requesting",
    false,
    1,
    0,
    serde_json::json!({}),
    None,
  );

  assert_eq!(
    recover_interrupted_runs(&fixture.root_path).expect("recovery should run"),
    0
  );
  let run = load_run(&fixture);
  let task = get_task(&fixture.root_path, &fixture.task.id).expect("task should load");
  let checkpoint = checkpoint_state(&fixture.root_path, &checkpoint_id);
  let run_step = run_step_state(&fixture);

  assert_eq!(run.status, "failed");
  assert_eq!(
    run.error_code.as_deref(),
    Some("UNCERTAIN_REQUEST_AFTER_CRASH")
  );
  assert!(run.claimed_at.is_none());
  assert_eq!(task.status, "failed");
  assert_eq!(checkpoint.0, "uncertain");
  assert_eq!(
    checkpoint.1.as_deref(),
    Some("UNCERTAIN_REQUEST_AFTER_CRASH")
  );
  assert!(!checkpoint.2);
  assert_eq!(
    run_step,
    ("failed".to_string(), Some("uncertain_request".to_string()))
  );

  std::fs::remove_dir_all(fixture.root_path).ok();
}

#[test]
fn recoverable_checkpoint_states_resume_at_their_safe_stage() {
  for (label, checkpoint_status, attempts, has_more, expected_stage) in [
    ("recover-prepared", "prepared", 0, None, "恢复待发送"),
    (
      "recover-response",
      "response_received",
      1,
      None,
      "恢复响应入库",
    ),
    ("recover-completed", "completed", 1, Some(true), "恢复续页"),
  ] {
    let fixture = running_fixture(label, 3, 100, 1_000);
    let checkpoint_id = insert_checkpoint(
      &fixture,
      checkpoint_status,
      false,
      attempts,
      if attempts == 0 { 0 } else { 5 },
      if attempts == 0 {
        serde_json::json!({})
      } else {
        serde_json::json!({ "currency": "USD", "amount_micros": 100 })
      },
      has_more,
    );

    assert_eq!(
      recover_interrupted_runs(&fixture.root_path).expect("recovery should run"),
      1
    );
    let run = load_run(&fixture);
    let task = get_task(&fixture.root_path, &fixture.task.id).expect("task should load");
    let checkpoint = checkpoint_state(&fixture.root_path, &checkpoint_id);

    assert_eq!(run.status, "queued");
    assert_eq!(run.current_stage.as_deref(), Some(expected_stage));
    assert!(run.claimed_at.is_none());
    assert_eq!(task.status, "queued");
    assert_eq!(checkpoint.0, checkpoint_status);

    std::fs::remove_dir_all(fixture.root_path).ok();
  }
}

#[test]
fn uncertain_or_terminal_checkpoint_stops_automatic_recovery() {
  for (label, status, expected_code, expected_stop_reason) in [
    (
      "recover-existing-uncertain",
      "uncertain",
      "UNCERTAIN_REQUEST_AFTER_CRASH",
      "uncertain_request",
    ),
    (
      "recover-terminal-failure",
      "failed",
      "CHECKPOINT_TERMINAL_FAILURE",
      "terminal_error",
    ),
  ] {
    let fixture = running_fixture(label, 3, 100, 1_000);
    insert_checkpoint(
      &fixture,
      status,
      false,
      1,
      0,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      None,
    );

    assert_eq!(
      recover_interrupted_runs(&fixture.root_path).expect("recovery should stop safely"),
      0
    );
    let run = load_run(&fixture);
    let task = get_task(&fixture.root_path, &fixture.task.id).expect("task should load");
    assert_eq!(run.status, "failed");
    assert_eq!(run.error_code.as_deref(), Some(expected_code));
    assert!(!run.retryable);
    assert_eq!(task.status, "failed");
    assert_eq!(
      run_step_state(&fixture),
      ("failed".to_string(), Some(expected_stop_reason.to_string()))
    );

    std::fs::remove_dir_all(fixture.root_path).ok();
  }
}

#[test]
fn retryable_failure_respects_request_record_and_budget_limits() {
  let cases = [
    (
      "recover-retry-within-limits",
      3,
      100,
      1_000,
      1,
      0,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      Some("恢复重试"),
      None,
    ),
    (
      "recover-request-limit",
      1,
      100,
      1_000,
      1,
      0,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      None,
      Some("REQUEST_LIMIT_REACHED"),
    ),
    (
      "recover-record-limit",
      3,
      5,
      1_000,
      1,
      5,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      None,
      Some("RECORD_LIMIT_REACHED"),
    ),
    (
      "recover-budget-limit",
      3,
      100,
      100,
      1,
      0,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      None,
      Some("BUDGET_LIMIT_REACHED"),
    ),
    (
      "recover-budget-unknown",
      3,
      100,
      1_000,
      1,
      0,
      serde_json::json!({}),
      None,
      Some("BUDGET_ACCOUNTING_INCOMPLETE"),
    ),
  ];

  for (
    label,
    request_limit,
    record_limit,
    budget_limit,
    attempts,
    records,
    cost,
    expected_stage,
    expected_code,
  ) in cases
  {
    let fixture = running_fixture(label, request_limit, record_limit, budget_limit);
    if records == 0 {
      insert_checkpoint(&fixture, "failed", true, attempts, 0, cost, None);
    } else {
      let completed = insert_checkpoint(
        &fixture,
        "completed",
        false,
        1,
        records,
        serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
        Some(true),
      );
      insert_followup_failed_checkpoint(&fixture.root_path, &completed, attempts, cost);
    }

    let recovered =
      recover_interrupted_runs(&fixture.root_path).expect("recovery decision should persist");
    let run = load_run(&fixture);
    let task = get_task(&fixture.root_path, &fixture.task.id).expect("task should load");
    if let Some(expected_stage) = expected_stage {
      assert_eq!(recovered, 1);
      assert_eq!(run.status, "queued");
      assert_eq!(run.current_stage.as_deref(), Some(expected_stage));
      assert_eq!(task.status, "queued");
    } else {
      assert_eq!(recovered, 0);
      assert_eq!(run.status, "failed");
      assert_eq!(run.error_code.as_deref(), expected_code);
      assert!(!run.retryable);
      assert_eq!(task.status, "failed");
    }

    std::fs::remove_dir_all(fixture.root_path).ok();
  }
}

#[test]
fn completed_recovery_uses_the_latest_committed_page() {
  let fixture = running_fixture("recover-latest-completed", 3, 100, 1_000);
  let first_checkpoint = insert_checkpoint(
    &fixture,
    "completed",
    false,
    1,
    5,
    serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
    Some(true),
  );
  insert_followup_completed_checkpoint(&fixture.root_path, &first_checkpoint);

  assert_eq!(
    recover_interrupted_runs(&fixture.root_path).expect("recovery should run"),
    1
  );
  let run = load_run(&fixture);
  assert_eq!(run.status, "queued");
  assert_eq!(run.current_stage.as_deref(), Some("恢复收尾"));

  std::fs::remove_dir_all(fixture.root_path).ok();
}

#[test]
fn incomplete_response_or_cursor_evidence_stops_recovery() {
  for (label, status, damage_sql) in [
    (
      "recover-response-evidence-missing",
      "response_received",
      "UPDATE collection_page_checkpoint
       SET provider_response_json = NULL, provider_response_hash = NULL,
           provider_response_size = NULL WHERE id = ?1",
    ),
    (
      "recover-next-cursor-missing",
      "completed",
      "UPDATE collection_page_checkpoint SET next_cursor_json = NULL WHERE id = ?1",
    ),
  ] {
    let fixture = running_fixture(label, 3, 100, 1_000);
    let checkpoint_id = insert_checkpoint(
      &fixture,
      status,
      false,
      1,
      5,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      (status == "completed").then_some(true),
    );
    open_workspace_connection(&fixture.root_path)
      .expect("database should open")
      .execute(damage_sql, params![checkpoint_id])
      .expect("checkpoint evidence should be damaged");

    assert_eq!(
      recover_interrupted_runs(&fixture.root_path).expect("recovery should stop safely"),
      0
    );
    let run = load_run(&fixture);
    assert_eq!(run.status, "failed");
    assert_eq!(
      run.error_code.as_deref(),
      Some("CHECKPOINT_EVIDENCE_INCOMPLETE")
    );

    std::fs::remove_dir_all(fixture.root_path).ok();
  }
}

#[test]
fn every_recovery_path_that_can_send_a_request_respects_runtime_limits() {
  for (label, request_limit, record_limit, budget_limit, expected_code) in [
    (
      "recover-completed-request-limit",
      1,
      100,
      1_000,
      "REQUEST_LIMIT_REACHED",
    ),
    (
      "recover-completed-record-limit",
      3,
      5,
      1_000,
      "RECORD_LIMIT_REACHED",
    ),
    (
      "recover-completed-budget-limit",
      3,
      100,
      100,
      "BUDGET_LIMIT_REACHED",
    ),
  ] {
    let fixture = running_fixture(label, request_limit, record_limit, budget_limit);
    insert_checkpoint(
      &fixture,
      "completed",
      false,
      1,
      5,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      Some(true),
    );

    assert_eq!(
      recover_interrupted_runs(&fixture.root_path).expect("recovery should enforce limits"),
      0
    );
    let run = load_run(&fixture);
    assert_eq!(run.status, "failed");
    assert_eq!(run.error_code.as_deref(), Some(expected_code));

    std::fs::remove_dir_all(fixture.root_path).ok();

    let prepared_fixture = running_fixture(
      &format!("{label}-prepared"),
      request_limit,
      record_limit,
      budget_limit,
    );
    let completed = insert_checkpoint(
      &prepared_fixture,
      "completed",
      false,
      1,
      5,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      Some(true),
    );
    insert_followup_prepared_checkpoint(&prepared_fixture.root_path, &completed);

    assert_eq!(
      recover_interrupted_runs(&prepared_fixture.root_path)
        .expect("prepared recovery should enforce limits"),
      0
    );
    let run = load_run(&prepared_fixture);
    assert_eq!(run.status, "failed");
    assert_eq!(run.error_code.as_deref(), Some(expected_code));

    std::fs::remove_dir_all(prepared_fixture.root_path).ok();
  }
}

#[test]
fn request_limit_is_scoped_to_the_step_that_will_send_next() {
  let checkpoints = vec![
    limit_checkpoint("finished-step", "completed", false, 3),
    limit_checkpoint("retry-step", "failed", true, 1),
  ];
  let limits = RecoveryLimits {
    request_limit: 3,
    record_limit: 100,
    budget_micros: 1_000,
  };

  assert!(retry_limit_stop(&checkpoints, &limits, "retry-step").is_none());
  let stop = retry_limit_stop(&checkpoints, &limits, "finished-step")
    .expect("finished step should be at its own request limit");
  assert!(matches!(
    stop,
    RecoveryAction::Stop {
      code: "REQUEST_LIMIT_REACHED",
      ..
    }
  ));
}

#[test]
fn completed_checkpoint_requires_a_valid_provider_response_and_full_persistence() {
  for (label, damage_sql) in [
    (
      "recover-invalid-provider-response",
      "UPDATE collection_page_checkpoint
       SET provider_response_json = '{\"data\":[]}',
           provider_response_hash = '8fe32e407a1038ee38753b70e5374b3a46d6ae9d5f16cd5b73c53abaca8f5ed0',
           provider_response_size = 11
       WHERE id = ?1",
    ),
    (
      "recover-partially-persisted-response",
      "UPDATE collection_page_checkpoint
       SET record_count_received = 5, record_count_persisted = 4
       WHERE id = ?1",
    ),
  ] {
    let fixture = running_fixture(label, 3, 100, 1_000);
    let checkpoint_id = insert_checkpoint(
      &fixture,
      "completed",
      false,
      1,
      5,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      Some(false),
    );
    open_workspace_connection(&fixture.root_path)
      .expect("database should open")
      .execute(damage_sql, params![checkpoint_id])
      .expect("checkpoint evidence should be damaged");

    assert_eq!(
      recover_interrupted_runs(&fixture.root_path).expect("recovery should reject bad evidence"),
      0
    );
    let run = load_run(&fixture);
    assert_eq!(run.status, "failed");
    assert_eq!(
      run.error_code.as_deref(),
      Some("CHECKPOINT_EVIDENCE_INCOMPLETE")
    );

    std::fs::remove_dir_all(fixture.root_path).ok();
  }
}

#[test]
fn completed_checkpoint_requires_explicit_pagination_and_a_contiguous_cursor_chain() {
  for (label, damage_sql) in [
    (
      "recover-completed-has-more-null",
      "UPDATE collection_page_checkpoint SET has_more = NULL WHERE id = ?1",
    ),
    (
      "recover-completed-page-gap",
      "UPDATE collection_page_checkpoint SET page_index = 2 WHERE id = ?1",
    ),
    (
      "recover-completed-cursor-mismatch",
      "UPDATE collection_page_checkpoint
       SET input_cursor_json = '{\"endpoint_key\":\"tiktok.keyword_search\",\"value\":999}'
       WHERE id = ?1",
    ),
  ] {
    let fixture = running_fixture(label, 3, 100, 1_000);
    let first_checkpoint = insert_checkpoint(
      &fixture,
      "completed",
      false,
      1,
      5,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      Some(true),
    );
    let damaged_checkpoint = if label == "recover-completed-has-more-null" {
      first_checkpoint
    } else {
      insert_followup_completed_checkpoint(&fixture.root_path, &first_checkpoint)
    };
    open_workspace_connection(&fixture.root_path)
      .expect("database should open")
      .execute(damage_sql, params![damaged_checkpoint])
      .expect("checkpoint chain should be damaged");

    assert_eq!(
      recover_interrupted_runs(&fixture.root_path).expect("recovery should reject broken chain"),
      0
    );
    let run = load_run(&fixture);
    assert_eq!(run.status, "failed");
    assert!(matches!(
      run.error_code.as_deref(),
      Some("CHECKPOINT_EVIDENCE_INCOMPLETE" | "CHECKPOINT_STATE_CONFLICT")
    ));

    std::fs::remove_dir_all(fixture.root_path).ok();
  }
}

#[test]
fn sent_states_require_request_evidence_and_prepared_rejects_request_traces() {
  for (label, status, damage_sql, expected_code) in [
    (
      "recover-response-without-request",
      "response_received",
      "UPDATE collection_page_checkpoint
       SET request_attempt_count = 0, requested_at = NULL WHERE id = ?1",
      "CHECKPOINT_EVIDENCE_INCOMPLETE",
    ),
    (
      "recover-prepared-with-request-time",
      "prepared",
      "UPDATE collection_page_checkpoint SET requested_at = '2026-07-13T08:00:00+00:00'
       WHERE id = ?1",
      "CHECKPOINT_STATE_CONFLICT",
    ),
  ] {
    let fixture = running_fixture(label, 3, 100, 1_000);
    let checkpoint_id = insert_checkpoint(
      &fixture,
      status,
      false,
      i64::from(status != "prepared"),
      0,
      serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
      None,
    );
    open_workspace_connection(&fixture.root_path)
      .expect("database should open")
      .execute(damage_sql, params![checkpoint_id])
      .expect("checkpoint state should be damaged");

    assert_eq!(
      recover_interrupted_runs(&fixture.root_path).expect("recovery should fail closed"),
      0
    );
    let run = load_run(&fixture);
    assert_eq!(run.status, "failed");
    assert_eq!(run.error_code.as_deref(), Some(expected_code));

    std::fs::remove_dir_all(fixture.root_path).ok();
  }
}

#[test]
fn retryable_failure_rejects_request_traces_without_an_attempt() {
  let fixture = running_fixture("recover-failed-request-trace", 3, 100, 1_000);
  let checkpoint_id =
    insert_checkpoint(&fixture, "failed", true, 0, 0, serde_json::json!({}), None);
  open_workspace_connection(&fixture.root_path)
    .expect("database should open")
    .execute(
      "UPDATE collection_page_checkpoint SET requested_at = ?1 WHERE id = ?2",
      params![T0, checkpoint_id],
    )
    .expect("failed checkpoint should be made contradictory");

  assert_eq!(
    recover_interrupted_runs(&fixture.root_path).expect("recovery should fail closed"),
    0
  );
  let run = load_run(&fixture);
  assert_eq!(run.status, "failed");
  assert_eq!(run.error_code.as_deref(), Some("CHECKPOINT_STATE_CONFLICT"));

  std::fs::remove_dir_all(fixture.root_path).ok();
}

#[test]
fn pending_run_step_cannot_already_own_a_completed_checkpoint() {
  let fixture = running_fixture("recover-pending-completed", 3, 100, 1_000);
  insert_checkpoint(
    &fixture,
    "completed",
    false,
    1,
    5,
    serde_json::json!({ "currency": "USD", "amount_micros": 100 }),
    Some(false),
  );
  open_workspace_connection(&fixture.root_path)
    .expect("database should open")
    .execute(
      "UPDATE task_run_step SET status = 'pending' WHERE task_run_id = ?1",
      params![fixture.run.id],
    )
    .expect("run step should be made contradictory");

  assert_eq!(
    recover_interrupted_runs(&fixture.root_path).expect("recovery should fail closed"),
    0
  );
  let run = load_run(&fixture);
  assert_eq!(run.status, "failed");
  assert_eq!(run.error_code.as_deref(), Some("CHECKPOINT_STATE_CONFLICT"));

  std::fs::remove_dir_all(fixture.root_path).ok();
}

fn running_fixture(
  label: &str,
  request_limit: i64,
  record_limit: i64,
  budget_micros: i64,
) -> RunningFixture {
  let root_path = unique_temp_workspace(label);
  create_workspace("恢复测试", &root_path).expect("workspace should create");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: label.to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("task should create");
  let plan = save_collection_plan(
    &root_path,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
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
        "record_limit": record_limit,
        "request_limit": request_limit,
        "budget_limit": {
          "currency": "USD",
          "amount_micros": budget_micros
        },
        "missing_fields": [],
        "requires_user_confirmation": true
      }),
      validation_status: "valid".to_string(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )
  .expect("plan should save");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan should confirm");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let run = claim_next_task(&root_path)
    .expect("claim should run")
    .expect("task should be claimed");

  RunningFixture {
    root_path,
    task,
    plan,
    run,
  }
}

fn insert_checkpoint(
  fixture: &RunningFixture,
  status: &str,
  retryable: bool,
  request_attempt_count: i64,
  record_count: i64,
  cost_actual_json: Value,
  has_more: Option<bool>,
) -> String {
  let connection = open_workspace_connection(&fixture.root_path).expect("database should open");
  let run_step_id = connection
    .query_row(
      "SELECT run_step.id
       FROM task_run_step AS run_step
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       WHERE run_step.task_run_id = ?1 AND api_step.plan_id = ?2
       ORDER BY api_step.step_order, api_step.id
       LIMIT 1",
      params![fixture.run.id, fixture.plan.id],
      |row| row.get::<_, String>(0),
    )
    .expect("materialized run step should load");
  let changed = connection
    .execute(
      "UPDATE task_run_step
       SET status = 'running', started_at = ?1, updated_at = ?1
       WHERE id = ?2 AND task_run_id = ?3",
      params![T0, run_step_id, fixture.run.id],
    )
    .expect("run step should start");
  assert_eq!(changed, 1);
  let checkpoint_id = Uuid::new_v4().to_string();
  let requested_at = (request_attempt_count > 0).then_some(T0);
  let response_received_at = matches!(status, "response_received" | "completed").then_some(T1);
  let committed_at = (status == "completed").then_some(T1);
  let stored_has_more = response_received_at.map(|_| has_more.unwrap_or(false));
  let provider_response_json = stored_has_more
    .map(|has_more| provider_response(record_count, has_more, has_more.then_some(20)));
  let provider_response_hash = provider_response_json.as_deref().map(response_hash);
  let provider_response_size = provider_response_json
    .as_deref()
    .and_then(|response| i64::try_from(response.len()).ok());
  let next_cursor_json = (stored_has_more == Some(true)).then(|| {
    serde_json::json!({
      "endpoint_key": "tiktok.keyword_search",
      "value": 20
    })
    .to_string()
  });
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, status,
         request_attempt_count, record_count_received, record_count_persisted,
         cost_actual_json, retryable, provider_response_json, provider_response_hash,
         provider_response_size, has_more, next_cursor_json, requested_at,
         response_received_at, committed_at, created_at, updated_at
       ) VALUES (
         ?1, ?2, 0, ?3, ?4, ?5, ?6, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
         ?14, ?15, ?16, ?17, ?17
       )",
      params![
        checkpoint_id,
        run_step_id,
        Uuid::new_v4().to_string(),
        status,
        request_attempt_count,
        record_count,
        cost_actual_json.to_string(),
        i64::from(retryable),
        provider_response_json,
        provider_response_hash,
        provider_response_size,
        stored_has_more.map(i64::from),
        next_cursor_json,
        requested_at,
        response_received_at,
        committed_at,
        T0
      ],
    )
    .expect("checkpoint should insert");
  checkpoint_id
}

fn insert_followup_completed_checkpoint(root_path: &Path, first_checkpoint_id: &str) -> String {
  let connection = open_workspace_connection(root_path).expect("database should open");
  let (run_step_id, input_cursor_json) = connection
    .query_row(
      "SELECT task_run_step_id, next_cursor_json
       FROM collection_page_checkpoint WHERE id = ?1",
      params![first_checkpoint_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .expect("run step should load");
  let checkpoint_id = Uuid::new_v4().to_string();
  let response = provider_response(5, false, None);
  let response_size = i64::try_from(response.len()).expect("response size should fit");
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, input_cursor_json, status,
         request_attempt_count, provider_response_json, provider_response_hash,
         provider_response_size, has_more, record_count_received, record_count_persisted,
         cost_actual_json, requested_at, response_received_at, committed_at,
         created_at, updated_at
       ) VALUES (
         ?1, ?2, 1, ?3, ?4, 'completed', 1, ?5, ?6,
         ?7, 0, 5, 5, '{\"currency\":\"USD\",\"amount_micros\":100}',
         ?8, ?8, ?8, ?8, ?8
       )",
      params![
        checkpoint_id,
        run_step_id,
        Uuid::new_v4().to_string(),
        input_cursor_json,
        response,
        response_hash(&response),
        response_size,
        T1
      ],
    )
    .expect("follow-up completed checkpoint should insert");
  checkpoint_id
}

fn insert_followup_prepared_checkpoint(root_path: &Path, first_checkpoint_id: &str) -> String {
  let connection = open_workspace_connection(root_path).expect("database should open");
  let (run_step_id, input_cursor_json) = connection
    .query_row(
      "SELECT task_run_step_id, next_cursor_json
       FROM collection_page_checkpoint WHERE id = ?1",
      params![first_checkpoint_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .expect("run step should load");
  let checkpoint_id = Uuid::new_v4().to_string();
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, input_cursor_json,
         status, created_at, updated_at
       ) VALUES (?1, ?2, 1, ?3, ?4, 'prepared', ?5, ?5)",
      params![
        checkpoint_id,
        run_step_id,
        Uuid::new_v4().to_string(),
        input_cursor_json,
        T1
      ],
    )
    .expect("follow-up prepared checkpoint should insert");
  checkpoint_id
}

fn insert_followup_failed_checkpoint(
  root_path: &Path,
  first_checkpoint_id: &str,
  attempts: i64,
  cost_actual_json: Value,
) -> String {
  let connection = open_workspace_connection(root_path).expect("database should open");
  let (run_step_id, input_cursor_json) = connection
    .query_row(
      "SELECT task_run_step_id, next_cursor_json
       FROM collection_page_checkpoint WHERE id = ?1",
      params![first_checkpoint_id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .expect("run step should load");
  let checkpoint_id = Uuid::new_v4().to_string();
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, input_cursor_json,
         status, request_attempt_count, cost_actual_json, retryable, requested_at,
         created_at, updated_at
       ) VALUES (?1, ?2, 1, ?3, ?4, 'failed', ?5, ?6, 1, ?7, ?8, ?8)",
      params![
        checkpoint_id,
        run_step_id,
        Uuid::new_v4().to_string(),
        input_cursor_json,
        attempts,
        cost_actual_json.to_string(),
        (attempts > 0).then_some(T1),
        T1
      ],
    )
    .expect("follow-up failed checkpoint should insert");
  checkpoint_id
}

fn limit_checkpoint(
  step_id: &str,
  status: &str,
  retryable: bool,
  attempts: i64,
) -> CheckpointState {
  CheckpointState {
    step_id: step_id.to_string(),
    status: status.to_string(),
    request_attempt_count: attempts,
    cost_actual_json: serde_json::json!({
      "currency": "USD",
      "amount_micros": 100
    })
    .to_string(),
    retryable,
    ..CheckpointState::default()
  }
}

fn load_run(fixture: &RunningFixture) -> TaskRunView {
  get_task_run(
    &open_workspace_connection(&fixture.root_path).expect("database should open"),
    &fixture.run.id,
  )
  .expect("run should load")
}

fn checkpoint_state(root_path: &Path, checkpoint_id: &str) -> (String, Option<String>, bool) {
  open_workspace_connection(root_path)
    .expect("database should open")
    .query_row(
      "SELECT status, last_error_code, retryable
       FROM collection_page_checkpoint WHERE id = ?1",
      params![checkpoint_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, Option<String>>(1)?,
          row.get::<_, i64>(2)? != 0,
        ))
      },
    )
    .expect("checkpoint state should load")
}

fn run_step_state(fixture: &RunningFixture) -> (String, Option<String>) {
  open_workspace_connection(&fixture.root_path)
    .expect("database should open")
    .query_row(
      "SELECT status, stop_reason FROM task_run_step
       WHERE task_run_id = ?1 ORDER BY id LIMIT 1",
      params![fixture.run.id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .expect("run step state should load")
}

fn response_hash(response: &str) -> String {
  format!("{:x}", Sha256::digest(response.as_bytes()))
}

fn provider_response(record_count: i64, has_more: bool, cursor: Option<i64>) -> String {
  let records = (0..record_count)
    .map(|index| serde_json::json!({ "id": index }))
    .collect::<Vec<_>>();
  let mut data = serde_json::json!({
    "aweme_list": records,
    "has_more": has_more
  });
  if let Some(cursor) = cursor {
    data["cursor"] = serde_json::json!(cursor);
  }
  serde_json::json!({ "code": 200, "data": data }).to_string()
}

fn unique_temp_workspace(label: &str) -> PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
