use super::*;
use sha2::{Digest, Sha256};

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

  let run_step_id = mark_run_steps_success(&root_path, &running.id)
    .into_iter()
    .next()
    .expect("single run step should exist");
  insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);

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
fn stale_worker_fence_rejects_success_and_failure_terminals() {
  let (root_path, task, _) = prepared_task_workspace("execution-stale-terminal-fence");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let run_step_id = mark_run_steps_success(&root_path, &running.id)
    .into_iter()
    .next()
    .expect("single run step should exist");
  insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);

  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO task_worker_lease (
         id, owner_id, lease_expires_at, created_at, updated_at, generation
       ) VALUES (
         'task_worker', 'replacement-owner', ?1,
         '2026-07-24T00:00:00+00:00', '2026-07-24T00:00:00+00:00', 2
       )",
      params![i64::MAX],
    )
    .expect("replacement worker lease should insert");
  drop(connection);

  let stale_fence =
    WorkerFence::new("stale-owner".to_string(), 1).expect("stale fence should construct");
  super::super::execution::complete_task_run_with_fence(
    &root_path,
    &running.id,
    Value::Null,
    &stale_fence,
  )
  .expect_err("stale worker must not complete the run");
  super::super::execution::fail_task_run_with_safe_details_with_fence(
    &root_path,
    &running.id,
    "TEST_STALE_FENCE",
    "stale worker failure",
    true,
    &std::collections::BTreeMap::new(),
    &stale_fence,
  )
  .expect_err("stale worker must not fail the run");
  assert_completion_remains_running(&root_path, &running.id, &task.id);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn complete_accepts_a_valid_multi_page_checkpoint_chain() {
  let (root_path, task, _) =
    prepared_completion_task_workspace("execution-complete-multi-page", 2, 1_200, 35_000_000);
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let run_step_id = mark_run_steps_success(&root_path, &running.id)
    .into_iter()
    .next()
    .expect("single run step should exist");
  let cursor = serde_json::json!({
    "endpoint_key": "tiktok.keyword_search",
    "value": 20
  });
  insert_completed_checkpoint(
    &root_path,
    &run_step_id,
    0,
    None,
    true,
    Some(cursor.clone()),
  );
  insert_completed_checkpoint(&root_path, &run_step_id, 1, Some(cursor), false, None);

  let completed = complete_task_run(
    &root_path,
    &running.id,
    serde_json::json!({ "request_count": 2 }),
  )
  .expect("closed multi-page evidence should complete");
  assert_eq!(completed.status, "success");
  assert_eq!(
    get_task(&root_path, &task.id)
      .expect("task should load")
      .status,
    "success"
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn complete_accepts_terminal_evidence_for_every_plan_step() {
  let (root_path, task, _) = prepared_multi_step_task_workspace("execution-complete-multi-step");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let run_step_ids = mark_run_steps_success(&root_path, &running.id);
  assert_eq!(run_step_ids.len(), 2);
  for run_step_id in run_step_ids {
    insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);
  }

  let completed = complete_task_run(
    &root_path,
    &running.id,
    serde_json::json!({ "request_count": 2 }),
  )
  .expect("every completed plan step should allow completion");
  assert_eq!(completed.status, "success");
  assert_eq!(
    get_task(&root_path, &task.id)
      .expect("task should load")
      .status,
    "success"
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn complete_rejects_missing_or_non_success_run_steps_without_mutation() {
  let mut accepted_states = Vec::new();
  for step_state in ["missing", "pending", "running", "failed", "cancelled"] {
    let (root_path, task, _) =
      prepared_task_workspace(&format!("execution-complete-step-{step_state}"));
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    let changed = if step_state == "missing" {
      connection
        .execute(
          "DELETE FROM task_run_step WHERE task_run_id = ?1",
          params![running.id],
        )
        .expect("run step should delete")
    } else if step_state == "pending" {
      1
    } else {
      connection
        .execute(
          "UPDATE task_run_step
           SET status = ?1, started_at = '2026-07-13T08:00:00+00:00',
               completed_at = CASE WHEN ?1 IN ('failed', 'cancelled')
                                   THEN '2026-07-13T08:01:00+00:00' END,
               updated_at = '2026-07-13T08:01:00+00:00'
           WHERE task_run_id = ?2",
          params![step_state, running.id],
        )
        .expect("run step state should update")
    };
    assert_eq!(changed, 1);
    drop(connection);

    if complete_task_run(
      &root_path,
      &running.id,
      serde_json::json!({ "request_count": 1 }),
    )
    .is_ok()
    {
      accepted_states.push(step_state);
    } else {
      let connection = open_workspace_connection(&root_path).expect("database should reopen");
      let stored_run = get_task_run(&connection, &running.id).expect("run should load");
      let stored_task = get_task_by_id(&connection, &task.id).expect("task should load");
      assert_eq!(stored_run.status, "running");
      assert!(stored_run.ended_at.is_none());
      assert_eq!(stored_task.status, "running");
      assert!(stored_task.completed_at.is_none());
    }

    std::fs::remove_dir_all(root_path).ok();
  }

  assert!(
    accepted_states.is_empty(),
    "不完整运行步骤不应完成成功，实际接受了 {accepted_states:?}"
  );
}

#[test]
fn complete_rejects_incomplete_checkpoint_evidence_without_mutation() {
  let mut accepted_evidence = Vec::new();
  for evidence in [
    "missing_checkpoint",
    "prepared_checkpoint",
    "requesting_checkpoint",
    "response_received_checkpoint",
    "failed_checkpoint",
    "uncertain_checkpoint",
    "completed_without_response_evidence",
    "unterminated_pagination",
  ] {
    let (root_path, task, _) =
      prepared_task_workspace(&format!("execution-complete-evidence-{evidence}"));
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    let run_step_id = connection
      .query_row(
        "SELECT id FROM task_run_step WHERE task_run_id = ?1",
        params![running.id],
        |row| row.get::<_, String>(0),
      )
      .expect("run step should load");
    assert_eq!(
      connection
        .execute(
          "UPDATE task_run_step
           SET status = 'success', started_at = '2026-07-13T08:00:00+00:00',
               completed_at = '2026-07-13T08:02:00+00:00',
               updated_at = '2026-07-13T08:02:00+00:00'
           WHERE id = ?1",
          params![run_step_id],
        )
        .expect("run step should become success"),
      1
    );
    if evidence == "unterminated_pagination" {
      insert_completed_checkpoint(
        &root_path,
        &run_step_id,
        0,
        None,
        true,
        Some(serde_json::json!({
          "endpoint_key": "tiktok.keyword_search",
          "value": 20
        })),
      );
    } else if evidence != "missing_checkpoint" {
      let (status, has_more) = match evidence {
        "prepared_checkpoint" => ("prepared", None),
        "requesting_checkpoint" => ("requesting", None),
        "response_received_checkpoint" => ("response_received", Some(0_i64)),
        "failed_checkpoint" => ("failed", None),
        "uncertain_checkpoint" => ("uncertain", None),
        "completed_without_response_evidence" => ("completed", Some(0_i64)),
        _ => unreachable!("evidence case should be known"),
      };
      assert_eq!(
        connection
          .execute(
            "INSERT INTO collection_page_checkpoint (
               id, task_run_step_id, page_index, idempotency_key, status, has_more,
               created_at, updated_at
             ) VALUES (?1, ?2, 0, ?3, ?4, ?5,
                       '2026-07-13T08:01:00+00:00', '2026-07-13T08:01:00+00:00')",
            params![
              Uuid::new_v4().to_string(),
              run_step_id,
              Uuid::new_v4().to_string(),
              status,
              has_more
            ],
          )
          .expect("checkpoint fixture should insert"),
        1
      );
    }
    drop(connection);

    if complete_task_run(
      &root_path,
      &running.id,
      serde_json::json!({ "request_count": 1 }),
    )
    .is_ok()
    {
      accepted_evidence.push(evidence);
    } else {
      let connection = open_workspace_connection(&root_path).expect("database should reopen");
      let stored_run = get_task_run(&connection, &running.id).expect("run should load");
      let stored_task = get_task_by_id(&connection, &task.id).expect("task should load");
      assert_eq!(stored_run.status, "running");
      assert!(stored_run.ended_at.is_none());
      assert_eq!(stored_task.status, "running");
      assert!(stored_task.completed_at.is_none());
    }

    std::fs::remove_dir_all(root_path).ok();
  }

  assert!(
    accepted_evidence.is_empty(),
    "不完整检查点证据不应完成成功，实际接受了 {accepted_evidence:?}"
  );
}

#[test]
fn complete_rejects_partially_missing_run_step_snapshot_without_mutation() {
  let (root_path, task, _) =
    prepared_multi_step_task_workspace("execution-complete-partial-snapshot");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let run_step_ids = mark_run_steps_success(&root_path, &running.id);
  assert_eq!(run_step_ids.len(), 2);
  for run_step_id in &run_step_ids {
    insert_completed_checkpoint(&root_path, run_step_id, 0, None, false, None);
  }
  let connection = open_workspace_connection(&root_path).expect("database should open");
  assert_eq!(
    connection
      .execute(
        "DELETE FROM task_run_step WHERE id = ?1",
        params![run_step_ids[1]],
      )
      .expect("one run step should delete"),
    1
  );
  drop(connection);

  complete_task_run(
    &root_path,
    &running.id,
    serde_json::json!({ "request_count": 2 }),
  )
  .expect_err("partially missing run-step snapshot must block completion");
  assert_completion_remains_running(&root_path, &running.id, &task.id);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn complete_rejects_tampered_terminal_evidence_and_step_metadata() {
  let mut accepted_tampering = Vec::new();
  for tampering in [
    "response_body",
    "response_hash",
    "response_size",
    "persisted_count",
    "cost",
    "requested_at",
    "committed_at",
    "retryable",
    "last_error",
    "step_stop_reason",
    "step_started_at",
    "step_completed_at",
    "step_time_reversed",
  ] {
    let (root_path, task, _) =
      prepared_task_workspace(&format!("execution-complete-tamper-{tampering}"));
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    let run_step_id = mark_run_steps_success(&root_path, &running.id)
      .into_iter()
      .next()
      .expect("single run step should exist");
    let checkpoint_id = insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);
    let connection = open_workspace_connection(&root_path).expect("database should open");
    let changed = match tampering {
      "response_body" => connection.execute(
        "UPDATE collection_page_checkpoint SET provider_response_json = '{}'
         WHERE id = ?1",
        params![checkpoint_id],
      ),
      "response_hash" => connection.execute(
        "UPDATE collection_page_checkpoint SET provider_response_hash = 'bad-hash'
         WHERE id = ?1",
        params![checkpoint_id],
      ),
      "response_size" => connection.execute(
        "UPDATE collection_page_checkpoint
         SET provider_response_size = provider_response_size + 1 WHERE id = ?1",
        params![checkpoint_id],
      ),
      "persisted_count" => connection.execute(
        "UPDATE collection_page_checkpoint SET record_count_persisted = 0 WHERE id = ?1",
        params![checkpoint_id],
      ),
      "cost" => connection.execute(
        "UPDATE collection_page_checkpoint SET cost_actual_json = '{}' WHERE id = ?1",
        params![checkpoint_id],
      ),
      "requested_at" => connection.execute(
        "UPDATE collection_page_checkpoint SET requested_at = NULL WHERE id = ?1",
        params![checkpoint_id],
      ),
      "committed_at" => connection.execute(
        "UPDATE collection_page_checkpoint SET committed_at = NULL WHERE id = ?1",
        params![checkpoint_id],
      ),
      "retryable" => connection.execute(
        "UPDATE collection_page_checkpoint SET retryable = 1 WHERE id = ?1",
        params![checkpoint_id],
      ),
      "last_error" => connection.execute(
        "UPDATE collection_page_checkpoint SET last_error_code = 'EVIDENCE_ERROR' WHERE id = ?1",
        params![checkpoint_id],
      ),
      "step_stop_reason" => connection.execute(
        "UPDATE task_run_step SET stop_reason = 'terminal_error' WHERE id = ?1",
        params![run_step_id],
      ),
      "step_started_at" => connection.execute(
        "UPDATE task_run_step SET started_at = NULL WHERE id = ?1",
        params![run_step_id],
      ),
      "step_completed_at" => connection.execute(
        "UPDATE task_run_step SET completed_at = NULL WHERE id = ?1",
        params![run_step_id],
      ),
      "step_time_reversed" => connection.execute(
        "UPDATE task_run_step
         SET started_at = '2026-07-13T09:00:00+00:00',
             completed_at = '2026-07-13T08:30:00+00:00'
         WHERE id = ?1",
        params![run_step_id],
      ),
      _ => unreachable!("tampering case should be known"),
    }
    .expect("terminal evidence should be tampered");
    assert_eq!(changed, 1);
    drop(connection);

    if complete_task_run(
      &root_path,
      &running.id,
      serde_json::json!({ "request_count": 1 }),
    )
    .is_ok()
    {
      accepted_tampering.push(tampering);
    } else {
      assert_completion_remains_running(&root_path, &running.id, &task.id);
    }

    std::fs::remove_dir_all(root_path).ok();
  }

  assert!(
    accepted_tampering.is_empty(),
    "被篡改的终态证据不应完成成功，实际接受了 {accepted_tampering:?}"
  );
}

#[test]
fn complete_rejects_broken_completed_checkpoint_chains_without_mutation() {
  let mut accepted_chains = Vec::new();
  for chain_case in ["page_gap", "cursor_mismatch", "premature_terminal"] {
    let (root_path, task, _) =
      prepared_task_workspace(&format!("execution-complete-chain-{chain_case}"));
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    let run_step_id = mark_run_steps_success(&root_path, &running.id)
      .into_iter()
      .next()
      .expect("single run step should exist");
    let cursor_20 = serde_json::json!({
      "endpoint_key": "tiktok.keyword_search",
      "value": 20
    });
    match chain_case {
      "page_gap" => {
        insert_completed_checkpoint(
          &root_path,
          &run_step_id,
          0,
          None,
          true,
          Some(cursor_20.clone()),
        );
        insert_completed_checkpoint(&root_path, &run_step_id, 2, Some(cursor_20), false, None);
      }
      "cursor_mismatch" => {
        insert_completed_checkpoint(&root_path, &run_step_id, 0, None, true, Some(cursor_20));
        insert_completed_checkpoint(
          &root_path,
          &run_step_id,
          1,
          Some(serde_json::json!({
            "endpoint_key": "tiktok.keyword_search",
            "value": 40
          })),
          false,
          None,
        );
      }
      "premature_terminal" => {
        insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);
        insert_completed_checkpoint(&root_path, &run_step_id, 1, Some(cursor_20), false, None);
      }
      _ => unreachable!("chain case should be known"),
    }

    if complete_task_run(
      &root_path,
      &running.id,
      serde_json::json!({ "request_count": 2 }),
    )
    .is_ok()
    {
      accepted_chains.push(chain_case);
    } else {
      let connection = open_workspace_connection(&root_path).expect("database should reopen");
      assert_eq!(
        get_task_run(&connection, &running.id)
          .expect("run should load")
          .status,
        "running"
      );
      assert_eq!(
        get_task_by_id(&connection, &task.id)
          .expect("task should load")
          .status,
        "running"
      );
    }

    std::fs::remove_dir_all(root_path).ok();
  }

  assert!(
    accepted_chains.is_empty(),
    "损坏的检查点页链不应完成成功，实际接受了 {accepted_chains:?}"
  );
}

#[test]
fn complete_terminal_updates_and_log_are_atomic() {
  let mut accepted_mutations = Vec::new();
  for failure_mode in [
    "run_update_ignored",
    "task_update_ignored",
    "log_insert_ignored",
    "log_failure",
  ] {
    let (root_path, task, _) =
      prepared_task_workspace(&format!("execution-complete-atomic-{failure_mode}"));
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    let run_step_id = mark_run_steps_success(&root_path, &running.id)
      .into_iter()
      .next()
      .expect("single run step should exist");
    insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);
    let connection = open_workspace_connection(&root_path).expect("database should open");
    let trigger = match failure_mode {
      "run_update_ignored" => {
        "CREATE TRIGGER ignore_terminal_run_update
         BEFORE UPDATE OF status ON task_run
         WHEN OLD.id = NEW.id AND NEW.status = 'success'
         BEGIN SELECT RAISE(IGNORE); END;"
      }
      "task_update_ignored" => {
        "CREATE TRIGGER ignore_terminal_task_update
         BEFORE UPDATE OF status ON collection_task
         WHEN OLD.id = NEW.id AND NEW.status = 'success'
         BEGIN SELECT RAISE(IGNORE); END;"
      }
      "log_insert_ignored" => {
        "CREATE TRIGGER ignore_terminal_success_log
         BEFORE INSERT ON task_log
         WHEN NEW.stage = '已完成'
         BEGIN SELECT RAISE(IGNORE); END;"
      }
      "log_failure" => {
        "CREATE TRIGGER fail_terminal_success_log
         BEFORE INSERT ON task_log
         WHEN NEW.stage = '已完成'
         BEGIN SELECT RAISE(ABORT, 'test terminal log failure'); END;"
      }
      _ => unreachable!("failure mode should be known"),
    };
    connection
      .execute_batch(trigger)
      .expect("terminal failure trigger should install");
    drop(connection);

    if complete_task_run(
      &root_path,
      &running.id,
      serde_json::json!({ "request_count": 1 }),
    )
    .is_ok()
    {
      accepted_mutations.push(failure_mode);
    } else {
      let connection = open_workspace_connection(&root_path).expect("database should reopen");
      let state = connection
        .query_row(
          "SELECT run.status, task.status,
                  (SELECT COUNT(*) FROM task_log
                   WHERE task_run_id = run.id AND stage = '已完成')
           FROM task_run AS run
           JOIN collection_task AS task ON task.id = run.task_id
           WHERE run.id = ?1",
          params![running.id],
          |row| {
            Ok((
              row.get::<_, String>(0)?,
              row.get::<_, String>(1)?,
              row.get::<_, i64>(2)?,
            ))
          },
        )
        .expect("terminal rollback state should load");
      assert_eq!(state, ("running".to_string(), "running".to_string(), 0));
    }

    std::fs::remove_dir_all(root_path).ok();
  }

  assert!(
    accepted_mutations.is_empty(),
    "终态写入异常不应提交，实际接受了 {accepted_mutations:?}"
  );
}

#[test]
fn complete_rejects_cascaded_plan_step_deletion_without_mutation() {
  let (root_path, task, plan) =
    prepared_multi_step_task_workspace("execution-complete-plan-cascade");
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  for run_step_id in mark_run_steps_success(&root_path, &running.id) {
    insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);
  }
  let connection = open_workspace_connection(&root_path).expect("database should open");
  assert_eq!(
    connection
      .execute(
        "DELETE FROM api_call_step
         WHERE id = (
           SELECT id FROM api_call_step WHERE plan_id = ?1 ORDER BY step_order DESC LIMIT 1
         )",
        params![plan.id],
      )
      .expect("one persisted plan step should delete"),
    1
  );
  assert_eq!(
    connection
      .query_row(
        "SELECT COUNT(*) FROM task_run_step WHERE task_run_id = ?1",
        params![running.id],
        |row| row.get::<_, i64>(0),
      )
      .expect("remaining run-step count should load"),
    1
  );
  drop(connection);

  complete_task_run(
    &root_path,
    &running.id,
    serde_json::json!({ "request_count": 2 }),
  )
  .expect_err("cascaded plan-step deletion must block completion");
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  assert_eq!(
    get_task_run(&connection, &running.id)
      .expect("run should load")
      .status,
    "running"
  );
  assert_eq!(
    get_task_by_id(&connection, &task.id)
      .expect("task should load")
      .status,
    "running"
  );

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn complete_rejects_evidence_outside_the_current_run_lifecycle() {
  let mut accepted_timelines = Vec::new();
  for timeline in ["before_claim", "future", "claimed_before_run"] {
    let (root_path, task, _) =
      prepared_task_workspace(&format!("execution-complete-time-{timeline}"));
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    let run_step_id = mark_run_steps_success(&root_path, &running.id)
      .into_iter()
      .next()
      .expect("single run step should exist");
    let checkpoint_id = insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);
    let connection = open_workspace_connection(&root_path).expect("database should open");
    match timeline {
      "before_claim" => {
        assert_eq!(
          connection
            .execute(
              "UPDATE task_run_step
               SET started_at = '2000-01-01T00:00:00+00:00',
                   completed_at = '2000-01-01T00:00:00+00:00'
               WHERE id = ?1",
              params![run_step_id],
            )
            .expect("step time should move before claim"),
          1
        );
        assert_eq!(
          connection
            .execute(
              "UPDATE collection_page_checkpoint
               SET requested_at = '2000-01-01T00:00:00+00:00',
                   response_received_at = '2000-01-01T00:00:00+00:00',
                   committed_at = '2000-01-01T00:00:00+00:00'
               WHERE id = ?1",
              params![checkpoint_id],
            )
            .expect("checkpoint time should move before claim"),
          1
        );
      }
      "future" => {
        assert_eq!(
          connection
            .execute(
              "UPDATE task_run_step
               SET started_at = '2999-01-01T00:00:00+00:00',
                   completed_at = '2999-01-01T00:00:00+00:00'
               WHERE id = ?1",
              params![run_step_id],
            )
            .expect("step time should move into future"),
          1
        );
        assert_eq!(
          connection
            .execute(
              "UPDATE collection_page_checkpoint
               SET requested_at = '2999-01-01T00:00:00+00:00',
                   response_received_at = '2999-01-01T00:00:00+00:00',
                   committed_at = '2999-01-01T00:00:00+00:00'
               WHERE id = ?1",
              params![checkpoint_id],
            )
            .expect("checkpoint time should move into future"),
          1
        );
      }
      "claimed_before_run" => {
        assert_eq!(
          connection
            .execute(
              "UPDATE task_run SET claimed_at = '2000-01-01T00:00:00+00:00'
               WHERE id = ?1",
              params![running.id],
            )
            .expect("claim time should move before run creation"),
          1
        );
      }
      _ => unreachable!("timeline should be known"),
    }
    drop(connection);

    if complete_task_run(
      &root_path,
      &running.id,
      serde_json::json!({ "request_count": 1 }),
    )
    .is_ok()
    {
      accepted_timelines.push(timeline);
    } else {
      assert_completion_remains_running(&root_path, &running.id, &task.id);
    }

    std::fs::remove_dir_all(root_path).ok();
  }

  assert!(
    accepted_timelines.is_empty(),
    "不属于本次运行生命周期的证据不应完成成功，实际接受了 {accepted_timelines:?}"
  );
}

#[test]
fn complete_rejects_confirmed_runtime_limit_overruns() {
  let mut accepted_overruns = Vec::new();
  for (limit_case, request_limit, record_limit, budget_micros) in [
    ("request_limit", 1, 10, 1_000),
    ("record_limit", 2, 1, 1_000),
    ("budget_limit", 2, 10, 100),
  ] {
    let (root_path, task, _) = prepared_completion_task_workspace(
      &format!("execution-complete-limit-{limit_case}"),
      request_limit,
      record_limit,
      budget_micros,
    );
    enqueue_task(&root_path, &task.id).expect("task should enqueue");
    let running = claim_next_task(&root_path)
      .expect("claim should succeed")
      .expect("queued task should be claimed");
    let run_step_id = mark_run_steps_success(&root_path, &running.id)
      .into_iter()
      .next()
      .expect("single run step should exist");
    let cursor = serde_json::json!({
      "endpoint_key": "tiktok.keyword_search",
      "value": 20
    });
    insert_completed_checkpoint(
      &root_path,
      &run_step_id,
      0,
      None,
      true,
      Some(cursor.clone()),
    );
    insert_completed_checkpoint(&root_path, &run_step_id, 1, Some(cursor), false, None);

    if complete_task_run(
      &root_path,
      &running.id,
      serde_json::json!({ "request_count": 2 }),
    )
    .is_ok()
    {
      accepted_overruns.push(limit_case);
    } else {
      assert_completion_remains_running(&root_path, &running.id, &task.id);
    }

    std::fs::remove_dir_all(root_path).ok();
  }

  assert!(
    accepted_overruns.is_empty(),
    "超过确认运行上限的证据不应完成成功，实际接受了 {accepted_overruns:?}"
  );
}

#[test]
fn complete_persists_cost_derived_from_checkpoint_evidence() {
  let (root_path, task, _) =
    prepared_completion_task_workspace("execution-complete-derived-cost", 1, 10, 1_000);
  enqueue_task(&root_path, &task.id).expect("task should enqueue");
  let running = claim_next_task(&root_path)
    .expect("claim should succeed")
    .expect("queued task should be claimed");
  let run_step_id = mark_run_steps_success(&root_path, &running.id)
    .into_iter()
    .next()
    .expect("single run step should exist");
  insert_completed_checkpoint(&root_path, &run_step_id, 0, None, false, None);

  complete_task_run(
    &root_path,
    &running.id,
    serde_json::json!({
      "currency": "USD",
      "amount_micros": 999_999,
      "request_count": 999,
      "record_count": 999
    }),
  )
  .expect("valid checkpoint evidence should complete the run");

  let connection = open_workspace_connection(&root_path).expect("database should open");
  let persisted = connection
    .query_row(
      "SELECT cost_actual_json FROM task_run WHERE id = ?1",
      params![running.id],
      |row| row.get::<_, String>(0),
    )
    .expect("actual cost should be persisted");
  assert_eq!(
    serde_json::from_str::<Value>(&persisted).expect("persisted cost should be JSON"),
    serde_json::json!({
      "currency": "USD",
      "billing_status": "quoted_not_final",
      "quoted_cost_micros": 100,
      "amount_micros": 100,
      "request_count": 1,
      "record_count": 1
    })
  );

  std::fs::remove_dir_all(root_path).ok();
}

fn prepared_completion_task_workspace(
  label: &str,
  request_limit: i64,
  record_limit: i64,
  budget_micros: i64,
) -> (std::path::PathBuf, CollectionTaskView, CollectionPlanView) {
  let root_path = unique_temp_workspace(label);
  create_workspace("完成门禁测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "完成门禁任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("completion task should create");
  let mut input = execution_plan_input(&task.id);
  input.plan_json["request_limit"] = serde_json::json!(request_limit);
  input.plan_json["record_limit"] = serde_json::json!(record_limit);
  input.plan_json["budget_limit"] = serde_json::json!({
    "currency": "USD",
    "amount_micros": budget_micros
  });
  let plan = save_collection_plan(&root_path, input).expect("completion plan should save");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("completion plan should confirm");
  (root_path, task, plan)
}

fn mark_run_steps_success(root_path: &Path, run_id: &str) -> Vec<String> {
  let connection = open_workspace_connection(root_path).expect("database should open");
  let claimed_at = connection
    .query_row(
      "SELECT claimed_at FROM task_run WHERE id = ?1",
      params![run_id],
      |row| row.get::<_, String>(0),
    )
    .expect("claimed run time should load");
  let run_step_ids = {
    let mut statement = connection
      .prepare(
        "SELECT run_step.id
         FROM task_run_step AS run_step
         JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
         WHERE run_step.task_run_id = ?1
         ORDER BY api_step.step_order, api_step.id",
      )
      .expect("run steps should prepare");
    statement
      .query_map(params![run_id], |row| row.get::<_, String>(0))
      .expect("run steps should query")
      .collect::<rusqlite::Result<Vec<_>>>()
      .expect("run steps should load")
  };
  assert!(!run_step_ids.is_empty());
  assert_eq!(
    connection
      .execute(
        "UPDATE task_run_step
         SET status = 'success', stop_reason = NULL,
             started_at = ?1, completed_at = ?1, updated_at = ?1
         WHERE task_run_id = ?2",
        params![claimed_at, run_id],
      )
      .expect("run steps should become success"),
    run_step_ids.len()
  );
  run_step_ids
}

fn assert_completion_remains_running(root_path: &Path, run_id: &str, task_id: &str) {
  let connection = open_workspace_connection(root_path).expect("database should reopen");
  let run = get_task_run(&connection, run_id).expect("run should load");
  let task = get_task_by_id(&connection, task_id).expect("task should load");
  assert_eq!(run.status, "running");
  assert!(run.ended_at.is_none());
  assert_eq!(task.status, "running");
  assert!(task.completed_at.is_none());
}

fn insert_completed_checkpoint(
  root_path: &Path,
  run_step_id: &str,
  page_index: i64,
  input_cursor: Option<Value>,
  has_more: bool,
  next_cursor: Option<Value>,
) -> String {
  let connection = open_workspace_connection(root_path).expect("database should open");
  let (endpoint_key, data_type, evidence_at) = connection
    .query_row(
      "SELECT api_step.endpoint_key, api_step.data_type, run.claimed_at
       FROM task_run_step AS run_step
       JOIN api_call_step AS api_step ON api_step.id = run_step.api_call_step_id
       JOIN task_run AS run ON run.id = run_step.task_run_id
       WHERE run_step.id = ?1",
      params![run_step_id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
        ))
      },
    )
    .expect("checkpoint api step should load");
  let record = serde_json::json!({ "id": format!("record-{page_index}") });
  let mut data = if data_type == "keyword_search" {
    serde_json::json!({ "aweme_list": [record], "has_more": has_more })
  } else {
    serde_json::json!({ "record": record, "has_more": has_more })
  };
  if has_more {
    data["cursor"] = next_cursor
      .as_ref()
      .and_then(|cursor| cursor.get("value"))
      .cloned()
      .expect("continuing checkpoint should provide next cursor value");
  }
  let response = serde_json::json!({ "code": 200, "data": data }).to_string();
  let response_hash = format!("{:x}", Sha256::digest(response.as_bytes()));
  let response_size = i64::try_from(response.len()).expect("response size should fit");
  let checkpoint_id = Uuid::new_v4().to_string();
  assert_eq!(
    connection
      .execute(
        "INSERT INTO collection_page_checkpoint (
           id, task_run_step_id, page_index, idempotency_key, input_cursor_json,
           status, request_attempt_count, final_endpoint_key, provider_response_json,
           provider_response_hash, provider_response_size, has_more, next_cursor_json,
           record_count_received, record_count_persisted, cost_actual_json,
           requested_at, response_received_at, committed_at, created_at, updated_at
         ) VALUES (
           ?1, ?2, ?3, ?4, ?5, 'completed', 1, ?6, ?7, ?8, ?9, ?10, ?11,
           1, 1, '{\"currency\":\"USD\",\"amount_micros\":100}',
           ?12, ?12, ?12, ?12, ?12
         )",
        params![
          checkpoint_id,
          run_step_id,
          page_index,
          Uuid::new_v4().to_string(),
          input_cursor.map(|cursor| cursor.to_string()),
          endpoint_key,
          response,
          response_hash,
          response_size,
          i64::from(has_more),
          next_cursor.map(|cursor| cursor.to_string()),
          evidence_at
        ],
      )
      .expect("completed checkpoint should insert"),
    1
  );
  checkpoint_id
}
