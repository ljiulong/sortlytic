use super::*;
use crate::workspace::create_workspace;

#[test]
fn task_plan_confirm_enqueue_and_logs_round_trip() {
  let root_path = unique_temp_workspace("tasks");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  let confirmed = confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
  let run = enqueue_task(&root_path, &task.id).expect("task enqueued");
  let logs = list_task_logs(&root_path, &run.id).expect("logs should list");

  assert_eq!(task.status, "draft");
  assert_eq!(plan.schema_version, 2);
  assert_eq!(plan.validation_status, "valid");
  assert_eq!(confirmed.status, "waiting_confirmation");
  assert!(confirmed.confirmed_at.is_some());
  assert_eq!(run.status, "queued");
  assert_eq!(logs.len(), 1);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn latest_persisted_plan_can_be_loaded_for_queue_actions() {
  let root_path = unique_temp_workspace("latest-task-plan");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let first = save_collection_plan(&root_path, plan_input(&task.id)).expect("first plan saved");
  let mut replacement_input = plan_input(&task.id);
  replacement_input.plan_json["keywords"] = serde_json::json!(["第二版计划"]);
  let replacement = save_collection_plan(&root_path, replacement_input).expect("replacement saved");

  let latest = get_latest_collection_plan(&root_path, &task.id)
    .expect("persisted latest plan should load for queue action");

  assert_ne!(first.id, replacement.id);
  assert_eq!(latest.id, replacement.id);
  assert_eq!(latest.task_id, task.id);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn version_three_multi_target_plan_saves_confirms_and_persists_step_limits() {
  let root_path = unique_temp_workspace("tasks-v3");
  create_workspace("v3 任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "小红书多目标账号".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["xiaohongshu".to_string()],
      data_types: vec!["item_detail".to_string(), "comments".to_string()],
    },
  )
  .expect("v3 任务应创建");
  let draft = crate::collection::generate_form_collection_plan(
    crate::collection::FormCollectionPlanRequest {
      platform: "xiaohongshu".to_string(),
      data_type: None,
      data_types: vec!["item_detail".to_string(), "comments".to_string()],
      params: serde_json::json!({ "keyword": "新能源汽车", "time_range": "近 180 天" }),
      age_range: Some(crate::collection::AgeRangeInput { min: 18, max: 35 }),
      request_limit: Some(4),
      record_limit: Some(1200),
      budget_limit_micros: Some(35_000_000),
    },
  )
  .expect("v3 草案应生成");
  let plan = save_collection_plan(
    &root_path,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: draft.validation_status,
      validation_errors_json: Some(draft.validation_errors_json),
      cost_estimate_json: Some(draft.cost_estimate_json),
    },
  )
  .expect("v3 计划应保存");

  assert_eq!(plan.schema_version, 3);
  assert_eq!(
    plan.validation_status, "valid",
    "{:?}",
    plan.validation_errors_json
  );
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("v3 计划应确认");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  let limits = {
    let mut statement = connection
      .prepare(
        "SELECT request_count_estimate FROM api_call_step WHERE plan_id = ?1 ORDER BY step_order",
      )
      .expect("step query should prepare");
    statement
      .query_map([plan.id], |row| row.get::<_, i64>(0))
      .expect("step limits should query")
      .collect::<Result<Vec<_>, _>>()
      .expect("step limits should parse")
  };
  assert_eq!(limits, vec![4, 1, 4]);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn persisted_cost_estimate_counts_the_confirmed_request_limit() {
  let root_path = unique_temp_workspace("request-limit-cost");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let mut input = plan_input(&task.id);
  input.plan_json["request_limit"] = serde_json::json!(5);

  let plan = save_collection_plan(&root_path, input).expect("plan should save");
  let estimate = estimate_task_cost(&root_path, Some(task.id), None).expect("cost should load");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  let stored_step = connection
    .query_row(
      "SELECT platform, data_type, endpoint_key, request_count_estimate
       FROM api_call_step WHERE plan_id = ?1",
      params![plan.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
          row.get::<_, i64>(3)?,
        ))
      },
    )
    .expect("confirmed request step should be stored");

  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 5);
  assert_eq!(estimate.request_count_estimate, 5);
  assert_eq!(
    stored_step,
    (
      "tiktok".to_string(),
      "keyword_search".to_string(),
      "tiktok.keyword_search".to_string(),
      5
    )
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn backend_validation_overrides_client_supplied_status() {
  let root_path = unique_temp_workspace("authoritative-plan-validation");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let mut valid_input = plan_input(&task.id);
  valid_input.validation_status = "invalid".to_string();
  let valid_plan = save_collection_plan(&root_path, valid_input).expect("valid plan saved");

  let mut invalid_input = plan_input(&task.id);
  invalid_input.plan_json = invalid_plan_json();
  invalid_input.validation_status = "valid".to_string();
  invalid_input.validation_errors_json = Some(serde_json::json!([]));
  let invalid_plan = save_collection_plan(&root_path, invalid_input).expect("invalid plan saved");
  let task_after_invalid_plan =
    get_task(&root_path, &task.id).expect("task should remain available after invalid plan");

  assert_eq!(valid_plan.schema_version, 2);
  assert_eq!(valid_plan.validation_status, "valid");
  assert_eq!(invalid_plan.schema_version, 2);
  assert_eq!(invalid_plan.validation_status, "needs_review");
  assert_eq!(task_after_invalid_plan.status, "draft");
  assert!(task_after_invalid_plan.confirmed_at.is_none());
  assert!(invalid_plan
    .validation_errors_json
    .as_array()
    .is_some_and(|errors| !errors.is_empty()));

  let error = confirm_collection_plan(&root_path, &task.id, &invalid_plan.id)
    .expect_err("backend-invalid plan should fail");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn partial_v2_envelope_is_not_downgraded_to_v1() {
  for (label, missing_field) in [
    ("partial-v2-missing-budget", "budget_limit"),
    ("partial-v2-missing-record-limit", "record_limit"),
  ] {
    let root_path = unique_temp_workspace(label);
    create_workspace("任务测试", &root_path).expect("workspace should be created");
    let task = create_collection_task(&root_path, create_task_input()).expect("task created");
    let mut input = plan_input(&task.id);
    input
      .plan_json
      .as_object_mut()
      .expect("plan fixture should be an object")
      .remove(missing_field);

    let plan = save_collection_plan(&root_path, input).expect("partial v2 plan should save");
    let errors = plan
      .validation_errors_json
      .as_array()
      .expect("validation errors should be an array")
      .iter()
      .filter_map(Value::as_str)
      .map(ToString::to_string)
      .collect::<Vec<_>>();
    let mut sorted_errors = errors.clone();
    sorted_errors.sort();
    sorted_errors.dedup();

    assert_eq!(plan.schema_version, 2);
    assert_eq!(plan.validation_status, "needs_review");
    assert!(errors.iter().any(|error| error.contains(missing_field)));
    assert_eq!(errors, sorted_errors);

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn legacy_v1_plan_is_readable_but_cannot_be_confirmed() {
  let root_path = unique_temp_workspace("legacy-plan");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, legacy_plan_input(&task.id))
    .expect("legacy plan should remain readable");

  assert_eq!(plan.schema_version, 1);
  assert_eq!(plan.validation_status, "needs_review");
  assert!(plan
    .validation_errors_json
    .as_array()
    .is_some_and(|errors| errors.iter().any(|error| {
      error
        .as_str()
        .is_some_and(|error| error.contains("v1") && error.contains("兼容读取"))
    })));

  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_plan
       SET validation_status = 'valid', validation_errors_json = '[]', confirmed_by_user = 1
       WHERE id = ?1",
      params![plan.id],
    )
    .expect("test should forge a legacy confirmation");
  connection
    .execute(
      "UPDATE collection_task SET confirmed_at = '2026-07-13T08:00:00+00:00' WHERE id = ?1",
      params![task.id],
    )
    .expect("test should forge the task confirmation marker");
  drop(connection);

  let error = confirm_collection_plan(&root_path, &task.id, &plan.id)
    .expect_err("legacy plans must not be confirmable");
  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("v1") && error.message.contains("不能确认"));

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
    .expect("legacy rejection should persist");
  assert_eq!(persisted.0, "needs_review");
  assert!(persisted.1.contains("v1") && persisted.1.contains("兼容读取"));
  assert_eq!(persisted.2, 0);
  assert!(persisted.3.is_none());

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn confirmation_revalidates_persisted_v2_limits() {
  for (label, mutate, expected_error) in [
    ("missing-budget", "missing_budget", "budget_limit"),
    (
      "invalid-record-limit",
      "invalid_record_limit",
      "record_limit",
    ),
    (
      "invalid-request-limit",
      "invalid_request_limit",
      "request_limit",
    ),
  ] {
    let root_path = unique_temp_workspace(label);
    create_workspace("任务测试", &root_path).expect("workspace should be created");
    let task = create_collection_task(&root_path, create_task_input()).expect("task created");
    let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
    let mut corrupted = plan.plan_json.clone();
    match mutate {
      "missing_budget" => {
        corrupted
          .as_object_mut()
          .expect("plan should be an object")
          .remove("budget_limit");
      }
      "invalid_record_limit" => corrupted["record_limit"] = serde_json::json!(0),
      "invalid_request_limit" => corrupted["request_limit"] = serde_json::json!(1.5),
      _ => unreachable!("test case should be known"),
    }
    let connection = open_workspace_connection(&root_path).expect("database should open");
    connection
      .execute(
        "UPDATE collection_plan
         SET plan_json = ?1, validation_status = 'valid', validation_errors_json = '[]',
             confirmed_by_user = 1
         WHERE id = ?2",
        params![corrupted.to_string(), plan.id],
      )
      .expect("test should corrupt persisted v2 limits");
    connection
      .execute(
        "UPDATE collection_task SET confirmed_at = '2026-07-13T08:00:00+00:00' WHERE id = ?1",
        params![task.id],
      )
      .expect("test should forge the task confirmation marker");
    drop(connection);

    let error = confirm_collection_plan(&root_path, &task.id, &plan.id)
      .expect_err("confirmation must revalidate persisted v2 limits");
    assert_eq!(error.code, AppErrorCode::ValidationError);

    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    let persisted = connection
      .query_row(
        "SELECT schema_version, validation_status, validation_errors_json, confirmed_by_user,
                (SELECT confirmed_at FROM collection_task WHERE id = ?2),
                (SELECT status FROM collection_task WHERE id = ?2)
         FROM collection_plan WHERE id = ?1",
        params![plan.id, task.id],
        |row| {
          Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
          ))
        },
      )
      .expect("failed v2 confirmation state should persist");
    assert_eq!(persisted.0, 2);
    assert_eq!(persisted.1, "needs_review");
    assert!(persisted.2.contains(expected_error));
    assert_eq!(persisted.3, 0);
    assert!(persisted.4.is_none());
    assert_eq!(persisted.5, "draft");

    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn confirmation_rejects_a_task_that_is_no_longer_waiting() {
  let root_path = unique_temp_workspace("confirmation-state-gate");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_task SET status = 'queued' WHERE id = ?1",
      params![task.id],
    )
    .expect("test should move the task out of the confirmation state");
  drop(connection);

  let error = confirm_collection_plan(&root_path, &task.id, &plan.id)
    .expect_err("queued tasks must not be confirmed");
  assert_eq!(error.code, AppErrorCode::ValidationError);

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let persisted = connection
    .query_row(
      "SELECT confirmed_by_user,
              (SELECT confirmed_at FROM collection_task WHERE id = ?2)
       FROM collection_plan WHERE id = ?1",
      params![plan.id, task.id],
      |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
    )
    .expect("task confirmation state should be readable");
  assert_eq!(persisted, (0, None));

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn changing_confirmed_scope_revokes_confirmation() {
  let root_path = unique_temp_workspace("confirmation-invalidation");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");

  let updated = update_collection_task(
    &root_path,
    &task.id,
    UpdateCollectionTaskInput {
      platforms: Some(vec!["douyin".to_string()]),
      ..UpdateCollectionTaskInput::default()
    },
  )
  .expect("task scope updated");

  assert!(updated.confirmed_at.is_none());
  let error = enqueue_task(&root_path, &task.id)
    .expect_err("scope changes must require a fresh confirmation");
  assert_eq!(error.code, AppErrorCode::ValidationError);

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn saving_a_new_plan_revokes_confirmation_and_rejects_stale_plan() {
  let root_path = unique_temp_workspace("new-plan-invalidation");
  create_workspace("任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let first_plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &first_plan.id).expect("plan confirmed");

  let mut replacement_input = plan_input(&task.id);
  replacement_input.source = "user_edited".to_string();
  let replacement =
    save_collection_plan(&root_path, replacement_input).expect("replacement plan saved");
  let updated_task = get_task(&root_path, &task.id).expect("task should load");

  assert!(updated_task.confirmed_at.is_none());
  let stale_error = confirm_collection_plan(&root_path, &task.id, &first_plan.id)
    .expect_err("only the latest plan can be confirmed");
  assert_eq!(stale_error.code, AppErrorCode::ValidationError);

  confirm_collection_plan(&root_path, &task.id, &replacement.id)
    .expect("latest valid plan should confirm");
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn delete_task_removes_draft_and_keeps_only_a_safe_audit_summary() {
  let root_path = unique_temp_workspace("delete-draft-task");
  create_workspace("任务删除测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");

  delete_task(&root_path, &task.id).expect("draft task should delete");

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let remaining = count_rows_for(&connection, "collection_task", "id", &task.id);
  let audit = connection
    .query_row(
      "SELECT action, safe_details_json FROM audit_log
       WHERE entity_type = 'collection_task' AND entity_id = ?1 AND action = 'delete_task'",
      params![task.id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .expect("delete audit should remain");

  assert_eq!(remaining, 0);
  assert_eq!(audit.0, "delete_task");
  assert!(audit.1.contains("draft"));
  assert!(!audit.1.contains(&task.name));
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn delete_task_accepts_every_terminal_status() {
  for status in ["success", "partial_success", "failed", "cancelled"] {
    let root_path = unique_temp_workspace(&format!("delete-{status}-task"));
    create_workspace("终态任务删除测试", &root_path).expect("workspace should be created");
    let task = create_collection_task(&root_path, create_task_input()).expect("task created");
    let connection = open_workspace_connection(&root_path).expect("database should open");
    connection
      .execute(
        "UPDATE collection_task SET status = ?1 WHERE id = ?2",
        params![status, task.id],
      )
      .expect("test should set terminal status");
    drop(connection);

    delete_task(&root_path, &task.id).expect("terminal task should delete");
    let connection = open_workspace_connection(&root_path).expect("database should reopen");
    assert_eq!(
      count_rows_for(&connection, "collection_task", "id", &task.id),
      0
    );
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn delete_task_removes_plan_run_snapshot_and_child_rows_without_orphans() {
  let root_path = unique_temp_workspace("delete-task-graph");
  create_workspace("任务图删除测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
  super::test_support::install_successful_tikhub_profile(&root_path)
    .expect("active TikHub profile should install");
  let queued = enqueue_task(&root_path, &task.id).expect("task enqueued");
  let run = claim_next_task(&root_path)
    .expect("task claim should succeed")
    .expect("queued task should be claimed");
  assert_eq!(run.id, queued.id);
  cancel_task(&root_path, &task.id).expect("running task should cancel before deletion");

  let connection = open_workspace_connection(&root_path).expect("database should open");
  assert_eq!(
    count_rows_for(
      &connection,
      "collection_runtime_snapshot",
      "task_run_id",
      &run.id,
    ),
    1,
  );
  drop(connection);

  delete_task(&root_path, &task.id).expect("cancelled task graph should delete");

  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  for (table, column, value) in [
    ("collection_task", "id", task.id.as_str()),
    ("collection_plan", "task_id", task.id.as_str()),
    ("task_run", "task_id", task.id.as_str()),
    ("api_call_step", "plan_id", plan.id.as_str()),
    ("task_run_step", "task_run_id", run.id.as_str()),
    ("task_log", "task_run_id", run.id.as_str()),
    (
      "collection_runtime_snapshot",
      "task_run_id",
      run.id.as_str(),
    ),
  ] {
    assert_eq!(
      count_rows_for(&connection, table, column, value),
      0,
      "{table} should not retain deleted task data",
    );
  }
  let foreign_key_violations = connection
    .prepare("PRAGMA foreign_key_check")
    .expect("foreign key check should prepare")
    .query_map([], |_| Ok(()))
    .expect("foreign key check should run")
    .count();
  assert_eq!(foreign_key_violations, 0);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn delete_task_rejects_queued_and_running_tasks_until_they_are_cancelled() {
  for status in ["queued", "running"] {
    let root_path = unique_temp_workspace(&format!("reject-delete-{status}"));
    create_workspace("活动任务删除测试", &root_path).expect("workspace should be created");
    let task = create_collection_task(&root_path, create_task_input()).expect("task created");
    let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
    confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
    let run = enqueue_task(&root_path, &task.id).expect("task enqueued");
    if status == "running" {
      let connection = open_workspace_connection(&root_path).expect("database should open");
      connection
        .execute(
          "UPDATE task_run SET status = 'running' WHERE id = ?1",
          params![run.id],
        )
        .expect("test run should enter running state");
      connection
        .execute(
          "UPDATE collection_task SET status = 'running' WHERE id = ?1",
          params![task.id],
        )
        .expect("test task should enter running state");
    }

    let error = delete_task(&root_path, &task.id)
      .expect_err("queued and running tasks must be cancelled before deletion");
    assert_eq!(error.code, AppErrorCode::ValidationError);
    assert!(error.message.contains("先取消"));
    assert_eq!(
      get_task(&root_path, &task.id)
        .expect("task should remain")
        .status,
      status
    );

    cancel_task(&root_path, &task.id).expect("active task should cancel");
    delete_task(&root_path, &task.id).expect("cancelled task should delete");
    std::fs::remove_dir_all(root_path).ok();
  }
}

#[test]
fn delete_task_reports_a_missing_task() {
  let root_path = unique_temp_workspace("delete-missing-task");
  create_workspace("缺失任务删除测试", &root_path).expect("workspace should be created");

  let error = delete_task(&root_path, "missing-task").expect_err("missing task should fail");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("任务不存在"));
  std::fs::remove_dir_all(root_path).ok();
}

fn count_rows_for(connection: &Connection, table: &str, column: &str, value: &str) -> i64 {
  connection
    .query_row(
      &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
      params![value],
      |row| row.get(0),
    )
    .expect("row count should query")
}

fn create_task_input() -> CreateCollectionTaskInput {
  CreateCollectionTaskInput {
    name: "采集 TikTok 关键词结果".to_string(),
    source_type: "form".to_string(),
    platforms: vec!["tiktok".to_string()],
    data_types: vec!["keyword_search".to_string()],
  }
}

fn plan_input(task_id: &str) -> SaveCollectionPlanInput {
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
    validation_errors_json: Some(serde_json::json!([])),
    cost_estimate_json: None,
  }
}

fn legacy_plan_input(task_id: &str) -> SaveCollectionPlanInput {
  let mut input = plan_input(task_id);
  let plan = input
    .plan_json
    .as_object_mut()
    .expect("plan fixture should be an object");
  plan.remove("record_limit");
  plan.remove("budget_limit");
  input
}

fn invalid_plan_json() -> Value {
  serde_json::json!({
    "platforms": ["tiktok"],
    "data_types": ["keyword_search"],
    "region": "US",
    "time_range": null,
    "steps": [{
      "endpoint_key": "tiktok.keyword_search",
      "platform": "tiktok",
      "data_type": "keyword_search",
      "params": {
        "keyword": "",
        "region": "US"
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
  })
}

fn unique_temp_workspace(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!("sortlytic-{label}-{}", Uuid::new_v4()))
}
