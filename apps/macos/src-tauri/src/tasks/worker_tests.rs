use super::*;
use crate::tasks::test_support::install_successful_tikhub_profile;
use crate::tasks::{
  cancel_task, claim_next_task, confirm_collection_plan, create_collection_task, enqueue_task,
  get_task, get_task_run, save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::create_workspace;
use serde_json::json;
use uuid::Uuid;

#[test]
fn version_three_worker_executes_each_materialized_dependency_target() {
  let root = std::env::temp_dir().join(format!("worker-v3-targets-{}", Uuid::new_v4()));
  create_workspace("v3 依赖目标测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
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
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
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
  cancel_task(&root, &task.id).expect_err("部分成功任务已是终态，不得取消");
  assert_eq!(
    get_task(&root, &task.id)
      .expect("task should remain readable")
      .status,
    "partial_success"
  );
  assert_eq!(
    get_task_run(&connection, &run.id)
      .expect("run should remain readable")
      .status,
    "partial_success"
  );

  std::fs::remove_dir_all(root).ok();
}

#[test]
fn version_three_worker_fails_when_every_target_fails() {
  let root = std::env::temp_dir().join(format!("worker-v3-all-failed-{}", Uuid::new_v4()));
  create_workspace("v3 全目标失败测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
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
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
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
fn worker_tick_keeps_task_queued_without_an_active_tikhub_profile() {
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
  let queued = enqueue_task(&root, &task.id).expect("task should be queued");

  let error = execute_next_task(&root)
    .expect_err("worker tick must fail closed without an active TikHub profile");
  assert!(error.message.contains("尚未选择当前 TikHub API 配置"));
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let run = get_task_run(&connection, &queued.id).expect("queued run should remain readable");
  drop(connection);
  let task = get_task(&root, &task.id).expect("queued task should remain readable");
  assert_eq!(run.status, "queued");
  assert!(run.claimed_at.is_none());
  assert_eq!(task.status, "queued");
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_persists_a_page_and_completes_the_run() {
  let root = std::env::temp_dir().join(format!("worker-success-{}", Uuid::new_v4()));
  create_workspace("执行器成功测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
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
fn worker_rejects_cost_before_recording_an_outbound_request() {
  let root = std::env::temp_dir().join(format!("worker-cost-gate-{}", Uuid::new_v4()));
  create_workspace("执行器成本门禁测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let (task, _plan) = create_confirmed_item_detail_task(&root);
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");
  let fetch_called = std::cell::Cell::new(false);

  let error = execute_claimed_run_with_guard(
    &root,
    &run,
    |_request| {
      Err(AppError::new(
        AppErrorCode::CostLimitError,
        "本次报价超过预算",
        AppErrorStage::Collection,
        false,
      ))
    },
    |_request| {
      fetch_called.set(true);
      unreachable!("成本门禁失败后不得调用供应商")
    },
  )
  .expect_err("成本门禁应拒绝请求");

  assert_eq!(error.code, AppErrorCode::CostLimitError);
  assert!(!fetch_called.get());
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let checkpoint_count: i64 = connection
    .query_row(
      "SELECT COUNT(*) FROM collection_page_checkpoint
       WHERE task_run_step_id IN (SELECT id FROM task_run_step WHERE task_run_id = ?1)",
      [&run.id],
      |row| row.get(0),
    )
    .expect("checkpoint count should be readable");
  assert_eq!(checkpoint_count, 0, "未调用供应商时不得留下请求检查点");
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_preserves_definitive_provider_errors_after_request_dispatch() {
  let cases = [
    (AppErrorCode::TikhubAuthError, "401", false, None),
    (AppErrorCode::CostLimitError, "402", false, None),
    (AppErrorCode::TikhubRateLimit, "429", true, Some("17")),
  ];

  for (code, http_status, retryable, retry_after) in cases {
    let root = std::env::temp_dir().join(format!(
      "worker-provider-error-{http_status}-{}",
      Uuid::new_v4()
    ));
    create_workspace("执行器供应商错误测试", &root).expect("workspace should be created");
    install_successful_tikhub_profile(&root).expect("TikHub profile should install");
    let (task, _) = create_confirmed_item_detail_task(&root);
    enqueue_task(&root, &task.id).expect("task should be queued");
    let run = claim_next_task(&root)
      .expect("worker should claim the task")
      .expect("queued task should exist");
    let mut provider_error = AppError::new(
      code.clone(),
      format!("TikHub 请求失败，HTTP {http_status}：响应正文已隐藏"),
      AppErrorStage::Collection,
      retryable,
    )
    .with_safe_detail("response_state", "received")
    .with_safe_detail("http_status", http_status);
    if let Some(retry_after) = retry_after {
      provider_error = provider_error.with_safe_detail("retry_after", retry_after);
    }

    let execution_error =
      execute_claimed_run_with_fetcher(&root, &run, |_request| Err(provider_error.clone()))
        .expect_err("definitive provider failures must stop the worker");

    assert_eq!(execution_error.code, code);
    assert_eq!(execution_error.retryable, retryable);
    assert_eq!(
      execution_error
        .safe_details
        .get("retry_after")
        .map(String::as_str),
      retry_after
    );
    let failed = finalize_claimed_run(&root, &run, Err(execution_error))
      .expect("provider failure should persist as the original terminal error");
    let expected_code = serialized_error_code(&code);
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.error_code.as_deref(), Some(expected_code.as_str()));
    assert_eq!(failed.retryable, retryable);
    assert_eq!(
      failed.error_safe_details_json["retry_after"].as_str(),
      retry_after
    );

    let connection = super::open_workspace_connection(&root).expect("database should open");
    let checkpoint: (String, String, i64) = connection
      .query_row(
        "SELECT status, last_error_code, retryable
         FROM collection_page_checkpoint
         WHERE task_run_step_id IN (
           SELECT id FROM task_run_step WHERE task_run_id = ?1
         )",
        [&run.id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
      )
      .expect("definitive checkpoint should be readable");
    assert_eq!(checkpoint.0, "failed");
    assert_eq!(checkpoint.1, expected_code);
    assert_eq!(checkpoint.2, i64::from(retryable));
    std::fs::remove_dir_all(root).ok();
  }
}

#[test]
fn pipeline_worker_preserves_rate_limit_details_after_request_dispatch() {
  let root = std::env::temp_dir().join(format!("worker-v3-rate-limit-{}", Uuid::new_v4()));
  create_workspace("多目标执行器限流测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "多目标限流".to_string(),
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
      record_limit: Some(1),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("pipeline plan should generate");
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
  .expect("pipeline plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("pipeline plan should confirm");
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");
  let provider_error = AppError::new(
    AppErrorCode::TikhubRateLimit,
    "TikHub 请求失败，HTTP 429：响应正文已隐藏",
    AppErrorStage::Collection,
    true,
  )
  .with_safe_detail("response_state", "received")
  .with_safe_detail("http_status", "429")
  .with_safe_detail("retry_after", "23");

  let execution_error =
    execute_claimed_run_with_fetcher(&root, &run, |_request| Err(provider_error.clone()))
      .expect_err("pipeline provider limit must stop the worker");

  assert_eq!(execution_error.code, AppErrorCode::TikhubRateLimit);
  assert_eq!(
    execution_error
      .safe_details
      .get("retry_after")
      .map(String::as_str),
    Some("23")
  );
  let failed = finalize_claimed_run(&root, &run, Err(execution_error))
    .expect("pipeline failure should preserve provider semantics");
  assert_eq!(failed.error_code.as_deref(), Some("TIKHUB_RATE_LIMIT"));
  assert!(failed.retryable);
  assert_eq!(failed.error_safe_details_json["retry_after"], "23");
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let checkpoint: (String, String, i64) = connection
    .query_row(
      "SELECT status, last_error_code, retryable
       FROM collection_page_checkpoint
       WHERE task_run_step_id IN (
         SELECT id FROM task_run_step WHERE task_run_id = ?1
       )",
      [&run.id],
      |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )
    .expect("pipeline checkpoint should be readable");
  assert_eq!(
    checkpoint,
    ("failed".to_string(), "TIKHUB_RATE_LIMIT".to_string(), 1)
  );
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_persists_sanitized_terminal_error_details() {
  let root = std::env::temp_dir().join(format!("worker-error-details-{}", Uuid::new_v4()));
  create_workspace("执行器错误详情测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let (task, _) = create_confirmed_item_detail_task(&root);
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");
  let connection = super::open_workspace_connection(&root).expect("database should open");
  connection
    .execute(
      "INSERT INTO collected_account (
         id, task_run_id, platform, identity_key, username, data_source, collected_at,
         output_included, created_at, updated_at
       ) VALUES ('partial-account', ?1, 'tiktok', 'id:partial-account', '预算内结果',
                 'TikHub API', ?2, 1, ?2, ?2)",
      rusqlite::params![run.id, "2026-07-21T00:00:00+00:00"],
    )
    .expect("partial result should persist");
  let run_step_id: String = connection
    .query_row(
      "SELECT id FROM task_run_step WHERE task_run_id = ?1 ORDER BY created_at, id LIMIT 1",
      [&run.id],
      |row| row.get(0),
    )
    .expect("run step should exist");
  connection
    .execute(
      "INSERT INTO collection_page_checkpoint (
         id, task_run_step_id, page_index, idempotency_key, status,
         request_attempt_count, record_count_received, record_count_persisted,
         cost_actual_json, requested_at, response_received_at, committed_at,
         created_at, updated_at
       ) VALUES (?1, ?2, 0, ?3, 'completed', 1, 1, 1, ?4, ?5, ?5, ?5, ?5, ?5)",
      rusqlite::params![
        Uuid::new_v4().to_string(),
        run_step_id,
        Uuid::new_v4().to_string(),
        serde_json::json!({
          "currency": "USD",
          "amount_micros": 100_000,
          "billing_status": "quoted_not_final"
        })
        .to_string(),
        "2026-07-21T00:00:00+00:00"
      ],
    )
    .expect("settled cost evidence should persist");
  drop(connection);
  let error = AppError::new(
    AppErrorCode::CostLimitError,
    "TikHub 本次报价将超过任务预算",
    AppErrorStage::Collection,
    false,
  )
  .with_safe_detail("retry_after", "17")
  .with_safe_detail("retry_attempts", "3")
  .with_safe_detail("api_token", "provider-secret");

  let failed = finalize_claimed_run(&root, &run, Err(error))
    .expect("worker should atomically persist a partial-success terminal state");

  assert_eq!(failed.status, "partial_success");
  assert_eq!(failed.error_safe_details_json["retry_after"], "17");
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let safe_details_json: String = connection
    .query_row(
      "SELECT safe_details_json FROM task_log
       WHERE task_run_id = ?1 AND level = 'warning'
       ORDER BY created_at DESC, id DESC LIMIT 1",
      [&run.id],
      |row| row.get(0),
    )
    .expect("terminal safe details should load");
  let safe_details: Value = serde_json::from_str(&safe_details_json).unwrap();
  assert_eq!(safe_details["retry_after"], "17");
  assert_eq!(safe_details["retry_attempts"], "3");
  assert!(safe_details.get("api_token").is_none());
  assert!(!safe_details_json.contains("provider-secret"));
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_treats_cancellation_during_an_inflight_request_as_cancelled() {
  let root = std::env::temp_dir().join(format!("worker-inflight-cancel-{}", Uuid::new_v4()));
  create_workspace("执行中取消测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let (task, _) = create_confirmed_item_detail_task(&root);
  enqueue_task(&root, &task.id).expect("task should be queued");
  let run = claim_next_task(&root)
    .expect("worker should claim the task")
    .expect("queued task should exist");

  let error = execute_claimed_run_with_fetcher(&root, &run, |_request| {
    cancel_task(&root, &task.id).expect("in-flight task should cancel");
    Ok(CollectionPage {
      records: vec![json!({"aweme_id": "video-1", "desc": "late response"})],
      next_cursor: None,
      has_more: false,
      raw_response: json!({"code": 200, "data": {"aweme_id": "video-1"}}),
    })
  })
  .expect_err("a cancelled run must not persist or complete the late response");

  assert_eq!(error.code, AppErrorCode::Cancelled);
  let connection = super::open_workspace_connection(&root).expect("database should open");
  assert_eq!(
    get_task_run(&connection, &run.id).unwrap().status,
    "cancelled"
  );
  assert_eq!(get_task(&root, &task.id).unwrap().status, "cancelled");
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn production_worker_does_not_claim_after_lease_takeover() {
  let root = std::env::temp_dir().join(format!("worker-claim-fence-{}", Uuid::new_v4()));
  create_workspace("执行器领取栅栏测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let (task, _) = create_confirmed_item_detail_task(&root);
  let queued = enqueue_task(&root, &task.id).expect("task should be queued");
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let now = chrono::Utc::now();
  connection
    .execute(
      "INSERT INTO task_worker_lease (
         id, owner_id, lease_expires_at, created_at, updated_at, generation
       ) VALUES ('task_worker', 'stale-owner', ?1, ?2, ?2, 1)",
      rusqlite::params![now.timestamp_millis() + 120_000, now.to_rfc3339()],
    )
    .expect("stale lease should be installed");
  let stale = crate::tasks::WorkerFence::new("stale-owner".to_string(), 1)
    .expect("stale fence should be valid");
  let first_check = std::cell::Cell::new(true);

  execute_next_task_with_owner(&root, &stale, || {
    let connection = super::open_workspace_connection(&root)?;
    stale.ensure_current(&connection)?;
    if first_check.replace(false) {
      connection
        .execute(
          "UPDATE task_worker_lease
           SET owner_id = 'replacement-owner', generation = 2, lease_expires_at = ?1
           WHERE id = 'task_worker'",
          [chrono::Utc::now().timestamp_millis() + 120_000],
        )
        .map_err(database_error)?;
    }
    Ok(())
  })
  .expect_err("a stale production worker must fail before claiming the queued run");

  let status: String = connection
    .query_row(
      "SELECT status FROM task_run WHERE id = ?1",
      [&queued.id],
      |row| row.get(0),
    )
    .expect("queued run should remain readable");
  assert_eq!(
    status, "queued",
    "lease takeover between the owner check and claim must fence the claim transaction"
  );
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn worker_marks_checkpoint_uncertain_when_record_persistence_fails() {
  let root = std::env::temp_dir().join(format!("worker-persist-failure-{}", Uuid::new_v4()));
  create_workspace("执行器落库失败测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
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
