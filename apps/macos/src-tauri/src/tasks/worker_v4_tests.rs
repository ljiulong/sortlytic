use super::*;
use crate::tasks::test_support::install_successful_tikhub_profile;
use crate::tasks::{
  claim_next_task, complete_task_run, confirm_collection_plan, create_collection_task,
  enqueue_task, save_collection_plan, CreateCollectionTaskInput, SaveCollectionPlanInput,
};
use crate::workspace::create_workspace;
use serde_json::json;
use uuid::Uuid;

#[test]
fn version_four_worker_enriches_discovered_account_country_by_handle() {
  let root = std::env::temp_dir().join(format!("worker-v4-country-{}", Uuid::new_v4()));
  create_workspace("v4 国家地区补全测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "TikTok 账号国家地区".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["account".to_string()],
    },
  )
  .expect("v4 account task should create");
  let draft = crate::collection::generate_account_collection_plan(
    crate::collection::AccountFormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      account_source: "user_search".to_string(),
      selected_fields: vec!["country_region".to_string()],
      enrichment_policy: "auto_costed".to_string(),
      params: json!({ "keyword": "electric car" }),
      age_range: None,
      gender_filter: None,
      request_limit: Some(1),
      record_limit: Some(1),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("v4 account plan should generate");
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
  .expect("v4 account plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("v4 account plan should confirm");
  enqueue_task(&root, &task.id).expect("v4 account task should enqueue");
  let run = claim_next_task(&root)
    .expect("worker should claim v4 account task")
    .expect("v4 account task should exist");
  let calls = std::cell::RefCell::new(Vec::new());

  execute_claimed_run_with_fetcher(&root, &run, |request| {
    if request.source_params().get("keyword").is_some() {
      calls.borrow_mut().push("discover".to_string());
      let records = (1..=20)
        .map(|index| {
          json!({
            "uid": format!("user-{index:02}"),
            "unique_id": format!("account-handle-{index:02}"),
            "nickname": format!("账号 {index:02}")
          })
        })
        .collect::<Vec<_>>();
      return Ok(CollectionPage {
        records: records.clone(),
        next_cursor: None,
        has_more: false,
        raw_response: json!({"code": 200, "data": {"user_list": records}}),
      });
    }
    let account_id = request
      .source_params()
      .get("account_id")
      .and_then(Value::as_str)
      .expect("country enrichment should receive account_id");
    calls.borrow_mut().push(format!("country:{account_id}"));
    let record = json!({
      "uid": "user-01",
      "unique_id": account_id,
      "country": "US"
    });
    Ok(CollectionPage {
      records: vec![record.clone()],
      next_cursor: None,
      has_more: false,
      raw_response: json!({"code": 200, "data": record}),
    })
  })
  .expect("v4 discovery and country enrichment should execute");
  let completed = complete_task_run(&root, &run.id, Value::Null)
    .expect("v4 account evidence should complete the run");

  assert_eq!(completed.status, "success");
  assert_eq!(
    calls.into_inner(),
    vec!["discover", "country:account-handle-01"]
  );
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let persisted = connection
    .query_row(
      "SELECT account_fields_json, field_evidence_json
       FROM collected_account
       WHERE task_run_id = ?1 AND output_included = 1",
      [&run.id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .expect("enriched account should persist");
  let fields: Value = serde_json::from_str(&persisted.0).expect("fields should be JSON");
  let evidence: Value = serde_json::from_str(&persisted.1).expect("evidence should be JSON");
  assert_eq!(fields["country_region"], "US");
  assert_eq!(
    evidence["country_region"]["endpoint_key"],
    "tiktok.account_country"
  );
  std::fs::remove_dir_all(root).ok();
}

#[test]
fn version_four_worker_rejects_non_empty_discovery_without_account_identity() {
  let root = std::env::temp_dir().join(format!("worker-v4-identity-{}", Uuid::new_v4()));
  create_workspace("v4 账号身份契约测试", &root).expect("workspace should be created");
  install_successful_tikhub_profile(&root).expect("TikHub profile should install");
  let task = create_collection_task(
    &root,
    CreateCollectionTaskInput {
      name: "TikTok 账号身份契约".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["account".to_string()],
    },
  )
  .expect("v4 account task should create");
  let draft = crate::collection::generate_account_collection_plan(
    crate::collection::AccountFormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      account_source: "user_search".to_string(),
      selected_fields: Vec::new(),
      enrichment_policy: "auto_costed".to_string(),
      params: json!({ "keyword": "identity contract" }),
      age_range: None,
      gender_filter: None,
      request_limit: Some(1),
      record_limit: Some(1),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("v4 account plan should generate");
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
  .expect("v4 account plan should save");
  confirm_collection_plan(&root, &task.id, &plan.id).expect("v4 account plan should confirm");
  enqueue_task(&root, &task.id).expect("v4 account task should enqueue");
  let run = claim_next_task(&root)
    .expect("worker should claim v4 account task")
    .expect("v4 account task should exist");

  let error = execute_claimed_run_with_fetcher(&root, &run, |_| {
    let record = json!({
      "id": "provider-row-without-account-identity",
      "nickname": "缺少身份的账号"
    });
    Ok(CollectionPage {
      records: vec![record.clone()],
      next_cursor: None,
      has_more: false,
      raw_response: json!({ "code": 200, "data": { "user_list": [record] } }),
    })
  })
  .expect_err("non-empty discovery without identity must fail");

  assert_eq!(
    error.safe_details["worker_code"],
    "ACCOUNT_IDENTITY_CONTRACT_FAILED"
  );
  let connection = super::open_workspace_connection(&root).expect("database should open");
  let checkpoint = connection
    .query_row(
      "SELECT status, retryable, last_error_code
       FROM collection_page_checkpoint
       WHERE task_run_step_id = (
         SELECT id FROM task_run_step WHERE task_run_id = ?1 LIMIT 1
       )",
      [&run.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, i64>(1)?,
          row.get::<_, Option<String>>(2)?,
        ))
      },
    )
    .expect("failed discovery checkpoint should persist");
  assert_eq!(
    checkpoint,
    (
      "failed".to_string(),
      0,
      Some("ACCOUNT_IDENTITY_CONTRACT_FAILED".to_string())
    )
  );
  let account_count = connection
    .query_row(
      "SELECT COUNT(*) FROM collected_account WHERE task_run_id = ?1",
      [&run.id],
      |row| row.get::<_, i64>(0),
    )
    .expect("account count should query");
  assert_eq!(account_count, 0);
  drop(connection);
  std::fs::remove_dir_all(root).ok();
}
