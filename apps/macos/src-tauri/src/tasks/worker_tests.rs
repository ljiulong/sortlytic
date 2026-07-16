use super::*;
use crate::tasks::{
  claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task, get_task,
  retry_task, save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::create_workspace;
use serde_json::json;
use uuid::Uuid;

#[test]
fn version_three_worker_executes_each_materialized_dependency_target() {
  let root = std::env::temp_dir().join(format!("worker-v3-targets-{}", Uuid::new_v4()));
  create_workspace("v3 依赖目标测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "搜索串联作品详情".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["item_detail".to_string()],
    },
  )
  .expect("task should create");
  let draft = crate::collection::generate_form_collection_plan(
    crate::collection::FormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      data_type: None,
      data_types: vec!["item_detail".to_string()],
      params: json!({
        "keyword": "car",
        "region": "US",
        "time_range": "30",
        "page_size": 20
      }),
      age_range: None,
      request_limit: Some(2),
      record_limit: Some(2),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("v3 dependency plan should generate");
  let plan = save_collection_plan(
    &root,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: draft.validation_status,
      validation_errors_json: Some(draft.validation_errors_json),
      cost_estimate_json: Some(draft.cost_estimate_json),
    },
  )
  .expect("v3 dependency plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("v3 dependency plan should confirm");
  enqueue_task(&root, &task.id).expect("v3 dependency task should enqueue");
  let run = claim_next_task(&root)
    .expect("worker should claim dependency task")
    .expect("dependency task should exist");
  let calls = std::cell::RefCell::new(Vec::new());

  execute_claimed_run_with_fetcher(&root, &run, |request| {
    let connection = super::open_workspace_connection(&root).expect("database should open");
    let running_target_count: i64 = connection
      .query_row(
        "SELECT COUNT(*) FROM collection_pipeline_target
         WHERE task_run_id = ?1 AND status = 'running'",
        [&run.id],
        |row| row.get(0),
      )
      .expect("running target state should be readable");
    assert_eq!(
      running_target_count, 1,
      "每次远端请求前必须先持久化唯一运行中目标"
    );
    if let Some(keyword) = request.source_params().get("keyword") {
      calls.borrow_mut().push(format!("search:{keyword}"));
      let records = vec![
        json!({
          "aweme_id": "video-a",
          "author": { "user_id": "account-a", "nickname": "账号 A" }
        }),
        json!({
          "aweme_id": "video-b",
          "author": { "user_id": "account-b", "nickname": "账号 B" }
        }),
      ];
      return Ok(CollectionPage {
        records: records.clone(),
        next_cursor: None,
        has_more: false,
        raw_response: json!({
          "code": 200,
          "data": { "aweme_list": records, "has_more": false }
        }),
      });
    }
    let item_id = request
      .source_params()
      .get("item_id")
      .and_then(Value::as_str)
      .expect("target request should contain resolved item_id");
    calls.borrow_mut().push(format!("detail:{item_id}"));
    let record = json!({
      "aweme_id": item_id,
      "author": {
        "user_id": format!("author-{item_id}"),
        "nickname": format!("作者 {item_id}")
      }
    });
    Ok(CollectionPage {
      records: vec![record.clone()],
      next_cursor: None,
      has_more: false,
      raw_response: json!({ "code": 200, "data": record, "has_more": false }),
    })
  })
  .expect("worker should execute every materialized detail target");
  let completed = complete_task_run(&root, &run.id, Value::Null)
    .expect("v3 multi-target evidence should complete the run");

  assert_eq!(completed.status, "success");
  assert_eq!(
    calls.into_inner(),
    vec!["search:\"car\"", "detail:video-a", "detail:video-b"]
  );
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let state = connection
    .query_row(
      "SELECT
         (SELECT COUNT(*) FROM collection_pipeline_target WHERE task_run_id = ?1),
         (SELECT COUNT(*) FROM collection_pipeline_target
          WHERE task_run_id = ?1 AND status = 'success'),
         (SELECT COUNT(*) FROM collected_account
          WHERE task_run_id = ?1 AND output_included = 1)",
      [&run.id],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, i64>(2)?,
        ))
      },
    )
    .expect("pipeline state should load");
  assert_eq!(state, (3, 3, 2));
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn version_three_worker_records_one_target_failure_and_continues() {
  let root = std::env::temp_dir().join(format!("worker-v3-partial-{}", Uuid::new_v4()));
  create_workspace("v3 逐目标失败测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "部分目标失败".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["item_detail".to_string()],
    },
  )
  .expect("task should create");
  let draft = crate::collection::generate_form_collection_plan(
    crate::collection::FormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      data_type: None,
      data_types: vec!["item_detail".to_string()],
      params: json!({
        "keyword": "car",
        "region": "US",
        "time_range": "30",
        "page_size": 20
      }),
      age_range: None,
      request_limit: Some(1),
      record_limit: Some(2),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("v3 plan should generate");
  let plan = save_collection_plan(
    &root,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: draft.validation_status,
      validation_errors_json: Some(draft.validation_errors_json),
      cost_estimate_json: Some(draft.cost_estimate_json),
    },
  )
  .expect("v3 plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("v3 plan should confirm");
  enqueue_task(&root, &task.id).expect("task should enqueue");
  let run = claim_next_task(&root)
    .expect("worker should claim task")
    .expect("queued task should exist");

  execute_claimed_run_with_fetcher(&root, &run, |request| {
    if request.source_params().get("keyword").is_some() {
      let records = vec![
        json!({"aweme_id": "bad-video", "author": {"user_id": "bad-account"}}),
        json!({"aweme_id": "good-video", "author": {"user_id": "good-account"}}),
      ];
      return Ok(CollectionPage {
        records: records.clone(),
        next_cursor: None,
        has_more: false,
        raw_response: json!({"code": 200, "data": {"aweme_list": records}}),
      });
    }
    if request
      .source_params()
      .get("item_id")
      .and_then(Value::as_str)
      == Some("bad-video")
    {
      return Err(crate::domain::AppError::new(
        crate::domain::AppErrorCode::TikhubRequestError,
        "目标不存在",
        crate::domain::AppErrorStage::Collection,
        false,
      ));
    }
    let record = json!({
      "aweme_id": "good-video",
      "author": {"user_id": "qualified-account", "nickname": "有效账号"}
    });
    Ok(CollectionPage {
      records: vec![record.clone()],
      next_cursor: None,
      has_more: false,
      raw_response: json!({"code": 200, "data": record}),
    })
  })
  .expect("one target failure should not terminate remaining targets");

  let completed = complete_task_run(&root, &run.id, Value::Null)
    .expect("qualified output with target failures should reach a partial terminal state");
  assert_eq!(completed.status, "partial_success");
  assert_eq!(completed.current_stage.as_deref(), Some("部分成功"));

  let connection = super::open_workspace_connection(&root).expect("database should open");
  let state = connection
    .query_row(
      "SELECT
         (SELECT COUNT(*) FROM collection_pipeline_target
          WHERE task_run_id = ?1 AND status = 'failed'),
         (SELECT COUNT(*) FROM collection_pipeline_target
          WHERE task_run_id = ?1 AND status = 'success'),
         (SELECT COUNT(*) FROM collection_failure_evidence WHERE task_run_id = ?1),
         (SELECT COUNT(*) FROM collected_account
          WHERE task_run_id = ?1 AND output_included = 1)",
      [&run.id],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, i64>(2)?,
          row.get::<_, i64>(3)?,
        ))
      },
    )
    .expect("partial target state should load");
  assert_eq!(state, (1, 2, 1, 1));
  let task_status: String = connection
    .query_row(
      "SELECT status FROM collection_task WHERE id = ?1",
      [&task.id],
      |row| row.get(0),
    )
    .expect("task status should load");
  assert_eq!(task_status, "partial_success");

  std::fs::remove_dir_all(root).ok();
}

#[test]
fn version_three_worker_fails_when_every_target_fails() {
  let root = std::env::temp_dir().join(format!("worker-v3-all-failed-{}", Uuid::new_v4()));
  create_workspace("v3 全目标失败测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "全部目标失败".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["keyword_search".to_string()],
    },
  )
  .expect("task should create");
  let draft = crate::collection::generate_form_collection_plan(
    crate::collection::FormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      data_type: None,
      data_types: vec!["keyword_search".to_string()],
      params: json!({
        "keyword": "car",
        "region": "US",
        "time_range": "30",
        "page_size": 20
      }),
      age_range: None,
      request_limit: Some(1),
      record_limit: Some(1),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("v3 plan should generate");
  let plan = save_collection_plan(
    &root,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: draft.validation_status,
      validation_errors_json: Some(draft.validation_errors_json),
      cost_estimate_json: Some(draft.cost_estimate_json),
    },
  )
  .expect("v3 plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("v3 plan should confirm");
  enqueue_task(&root, &task.id).expect("task should enqueue");
  let run = claim_next_task(&root)
    .expect("worker should claim task")
    .expect("queued task should exist");

  execute_claimed_run_with_fetcher(&root, &run, |_| {
    Err(crate::domain::AppError::new(
      crate::domain::AppErrorCode::TikhubRequestError,
      "目标不可用",
      crate::domain::AppErrorStage::Collection,
      false,
    ))
  })
  .expect("isolated target failure should form deterministic evidence");
  let completed = complete_task_run(&root, &run.id, Value::Null)
    .expect("all failed targets should settle the run");
  assert_eq!(completed.status, "failed");
  assert_eq!(completed.error_code.as_deref(), Some("ALL_TARGETS_FAILED"));
  assert_eq!(
    get_task(&root, &task.id).expect("task should load").status,
    "failed"
  );

  std::fs::remove_dir_all(root).ok();
}

#[test]
fn version_three_worker_counts_only_merged_age_qualified_accounts() {
  let root = std::env::temp_dir().join(format!("worker-v3-age-{}", Uuid::new_v4()));
  create_workspace("v3 年龄分页测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "合格账号硬上限".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["comments".to_string()],
    },
  )
  .expect("task should create");
  let draft = crate::collection::generate_form_collection_plan(
    crate::collection::FormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      data_type: None,
      data_types: vec!["comments".to_string()],
      params: json!({ "item_id": "video-1" }),
      age_range: Some(crate::collection::AgeRangeInput { min: 18, max: 30 }),
      request_limit: Some(2),
      record_limit: Some(1),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("v3 plan should generate");
  let plan = save_collection_plan(
    &root,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: draft.validation_status,
      validation_errors_json: Some(draft.validation_errors_json),
      cost_estimate_json: Some(draft.cost_estimate_json),
    },
  )
  .expect("v3 plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("v3 plan should confirm");
  enqueue_task(&root, &task.id).expect("v3 task should enqueue");
  let run = claim_next_task(&root)
    .expect("worker should claim v3 task")
    .expect("v3 queued task should exist");
  let calls = std::cell::Cell::new(0);

  execute_claimed_run_with_fetcher(&root, &run, |_request| {
    let call = calls.get();
    calls.set(call + 1);
    Ok(if call == 0 {
      CollectionPage {
        records: vec![
          json!({ "cid": "c-1", "user": { "user_id": "u-1", "nickname": "未知年龄" } }),
          json!({ "cid": "c-2", "user": { "user_id": "u-2", "nickname": "超龄", "age": 40 } }),
        ],
        next_cursor: Some(json!({ "endpoint_key": "tiktok.comments", "value": 20 })),
        has_more: true,
        raw_response: json!({ "comments": [], "has_more": true, "cursor": "next-page" }),
      }
    } else {
      CollectionPage {
        records: vec![json!({
          "cid": "c-3",
          "user": { "user_id": "u-3", "nickname": "合格账号", "age": "25" }
        })],
        next_cursor: None,
        has_more: false,
        raw_response: json!({ "comments": [], "has_more": false }),
      }
    })
  })
  .expect("v3 worker should continue until a qualified account is collected");

  assert_eq!(calls.get(), 2);
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let counts = connection
    .query_row(
      "SELECT
         (SELECT COUNT(*) FROM raw_record WHERE task_run_id = ?1),
         (SELECT COUNT(*) FROM collected_account WHERE task_run_id = ?1),
         (SELECT COUNT(*) FROM collected_account
          WHERE task_run_id = ?1 AND output_included = 1),
         (SELECT age FROM collected_account
          WHERE task_run_id = ?1 AND output_included = 1)",
      [&run.id],
      |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, i64>(2)?,
          row.get::<_, i64>(3)?,
        ))
      },
    )
    .expect("v3 collection counts should load");
  assert_eq!(counts, (3, 3, 1, 25));
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_tick_does_not_leave_a_queued_task_unprocessed() {
  let root = std::env::temp_dir().join(format!("worker-{}", Uuid::new_v4()));
  create_workspace("执行器测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "无连接器任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["item_detail".to_string()],
    },
  )
  .expect("task should be created");
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
          "params": {"item_id": "video-1"}
        }],
        "record_limit": 1,
        "request_limit": 1,
        "budget_limit": {"currency": "USD", "amount_micros": 35000000},
        "missing_fields": [],
        "requires_user_confirmation": true
      }),
      validation_status: "valid".to_string(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )
  .expect("plan should be saved");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should be confirmed");
  enqueue_task(&root, &task.id).expect("task should be queued");

  let run = execute_next_task(&root)
    .expect("worker tick should complete its state transition")
    .expect("worker should claim the queued task");

  assert_eq!(run.status, "failed");
  assert_eq!(
    run.error_code.as_deref(),
    Some("RUNTIME_SNAPSHOT_NOT_READY")
  );
  assert!(run.retryable);
  let retry =
    retry_task(&root, &task.id, None).expect("connector setup failure should be retryable");
  assert_eq!(retry.status, "queued");
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_persists_a_page_and_completes_the_run() {
  let root = std::env::temp_dir().join(format!("worker-success-{}", Uuid::new_v4()));
  create_workspace("执行器成功测试", &root).expect("workspace should be created");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "单页任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["item_detail".to_string()],
    },
  )
  .expect("task should be created");
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
          "params": {"item_id": "video-1"}
        }],
        "record_limit": 1,
        "request_limit": 1,
        "budget_limit": {"currency": "USD", "amount_micros": 35000000},
        "missing_fields": [],
        "requires_user_confirmation": true
      }),
      validation_status: "valid".to_string(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )
  .expect("plan should be saved");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("plan should be confirmed");
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");

  execute_claimed_run_with_fetcher(&root, &run, |request| {
    assert!(request.idempotency_key().is_some());
    Ok(CollectionPage {
      records: vec![json!({"aweme_id": "video-1", "desc": "test"})],
      next_cursor: None,
      has_more: false,
      raw_response: json!({
        "code": 200,
        "data": {"aweme_id": "video-1", "desc": "test"}
      }),
    })
  })
  .expect("page should execute");
  let completed = complete_task_run(&root, &run.id, Value::Null)
    .expect("run should complete from checkpoint evidence");

  assert_eq!(completed.status, "success");
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let checkpoint: (String, i64, i64) = connection
    .query_row(
      "SELECT status, record_count_received, record_count_persisted
         FROM collection_page_checkpoint",
      [],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("checkpoint should be persisted");
  assert_eq!(checkpoint, ("completed".to_string(), 1, 1));
  let task_status: String = connection
    .query_row(
      "SELECT status FROM collection_task WHERE id = ?1",
      [&task.id],
      |row| row.get(0),
    )
    .expect("task should be readable");
  assert_eq!(task_status, "success");
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_marks_checkpoint_uncertain_when_record_persistence_fails() {
  let root = std::env::temp_dir().join(format!("worker-persist-failure-{}", Uuid::new_v4()));
  create_workspace("执行器落库失败测试", &root).expect("workspace should be created");
  let (task, plan) = create_confirmed_item_detail_task(&root);
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");

  execute_claimed_run_with_fetcher(&root, &run, |_request| {
    Ok(CollectionPage {
      records: vec![json!({"desc": "missing id"})],
      next_cursor: None,
      has_more: false,
      raw_response: json!({
        "code": 200,
        "data": {"desc": "missing id"}
      }),
    })
  })
  .expect_err("invalid records must fail the worker");

  let connection = super::open_workspace_connection(&root).expect("database should open");
  let checkpoint_status: String = connection
    .query_row(
      "SELECT status FROM collection_page_checkpoint
         WHERE task_run_step_id IN (SELECT id FROM task_run_step WHERE task_run_id = ?1)",
      [&run.id],
      |row| row.get(0),
    )
    .expect("checkpoint should be persisted");
  assert_eq!(checkpoint_status, "uncertain");
  let _ = plan;
  std::fs::remove_dir_all(root).ok();
}

fn create_confirmed_item_detail_task(
  root: &std::path::Path,
) -> (
  crate::tasks::CollectionTaskView,
  crate::tasks::CollectionPlanView,
) {
  let task = create_collection_task(
    root,
    CreateCollectionTaskInput {
      name: "单页任务".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["item_detail".to_string()],
    },
  )
  .expect("task should be created");
  let plan = save_collection_plan(
    root,
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
          "params": {"item_id": "video-1"}
        }],
        "record_limit": 1,
        "request_limit": 1,
        "budget_limit": {"currency": "USD", "amount_micros": 35000000},
        "missing_fields": [],
        "requires_user_confirmation": true
      }),
      validation_status: "valid".to_string(),
      validation_errors_json: None,
      cost_estimate_json: None,
    },
  )
  .expect("plan should be saved");
  confirm_collection_plan(root, &task.id, &plan.id).expect("plan should be confirmed");
  (task, plan)
}
