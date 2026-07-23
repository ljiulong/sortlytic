use super::*;
use crate::workspace::{create_workspace, open_workspace_database, DATABASE_FILE_NAME};

#[test]
fn natural_task_creation_atomically_persists_the_complete_initial_intent() {
  let root_path = unique_temp_workspace("natural-initial-intent");
  create_workspace("自然语言原子草稿", &root_path).expect("workspace should be created");
  let intent_text = "用中文查找英国 TikTok 宠物用品账号，保留完整原始输入并在进程中断后恢复，最多 10 个，预算 0.1 美元。";

  let task = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "用中文查找英国 TikTok 宠物用品账号".to_string(),
      source_type: "natural_language".to_string(),
      platforms: Vec::new(),
      data_types: Vec::new(),
    },
    Some(intent_text),
  )
  .expect("task and initial intent should commit together");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  let attempt = connection
    .query_row(
      "SELECT intent_text, parse_status, parse_phase FROM task_intent WHERE task_id = ?1",
      [&task.id],
      |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
        ))
      },
    )
    .expect("initial attempt should exist before model invocation");

  assert_eq!(attempt.0, intent_text);
  assert_eq!(attempt.1, "running");
  assert_eq!(attempt.2, "preparing");
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn natural_task_creation_rejects_intent_over_the_character_limit_before_writing() {
  let root_path = unique_temp_workspace("natural-intent-character-limit");
  create_workspace("自然语言字符上限", &root_path).expect("workspace should be created");
  let oversized = "a".repeat(10_001);

  let error = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "拒绝超长自然语言输入".to_string(),
      source_type: "natural_language".to_string(),
      platforms: Vec::new(),
      data_types: Vec::new(),
    },
    Some(&oversized),
  )
  .expect_err("oversized natural input must fail before task creation");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("10000"));
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  assert_eq!(
    count_rows_for(
      &connection,
      "collection_task",
      "source_type",
      "natural_language"
    ),
    0,
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn natural_task_creation_rejects_intent_over_the_utf8_byte_limit_before_writing() {
  let root_path = unique_temp_workspace("natural-intent-byte-limit");
  create_workspace("自然语言字节上限", &root_path).expect("workspace should be created");
  let four_byte_character = "\u{10000}";
  let oversized = four_byte_character.repeat((32_000 / four_byte_character.len()) + 1);

  let error = create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "拒绝超大多字节输入".to_string(),
      source_type: "natural_language".to_string(),
      platforms: Vec::new(),
      data_types: Vec::new(),
    },
    Some(&oversized),
  )
  .expect_err("oversized UTF-8 input must fail before task creation");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert!(error.message.contains("32000"));
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  assert_eq!(
    count_rows_for(
      &connection,
      "collection_task",
      "source_type",
      "natural_language"
    ),
    0,
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn initial_intent_failure_rolls_back_task_and_audit_log() {
  let root_path = unique_temp_workspace("natural-initial-intent-rollback");
  create_workspace("自然语言原子回滚", &root_path).expect("workspace should be created");
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  connection
    .execute_batch(
      "CREATE TRIGGER fail_initial_intent BEFORE INSERT ON task_intent
       BEGIN SELECT RAISE(ABORT, 'forced initial intent failure'); END;",
    )
    .unwrap();
  drop(connection);

  create_collection_task_with_initial_intent(
    &root_path,
    CreateCollectionTaskInput {
      name: "必须完整回滚".to_string(),
      source_type: "natural_language".to_string(),
      platforms: Vec::new(),
      data_types: Vec::new(),
    },
    Some("这段完整输入不能只留下任务壳"),
  )
  .expect_err("failed initial attempt must roll back every related row");

  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  assert_eq!(
    count_rows_for(
      &connection,
      "collection_task",
      "source_type",
      "natural_language"
    ),
    0
  );
  assert_eq!(
    count_rows_for(&connection, "task_intent", "parse_status", "running"),
    0
  );
  assert_eq!(
    count_rows_for(&connection, "audit_log", "action", "create_collection_task"),
    0
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn form_task_rejects_an_initial_natural_language_intent_before_writing() {
  let root_path = unique_temp_workspace("form-initial-intent-rejected");
  create_workspace("表单任务原文边界", &root_path).expect("workspace should be created");

  let error = create_collection_task_with_initial_intent(
    &root_path,
    create_task_input(),
    Some("表单任务不得创建自然语言解析 attempt"),
  )
  .expect_err("form task must reject natural intent text");

  assert!(error.message.contains("自然语言"));
  let connection = open_workspace_database(root_path.join(DATABASE_FILE_NAME)).unwrap();
  assert_eq!(
    count_rows_for(&connection, "collection_task", "source_type", "form"),
    0
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn natural_language_draft_can_start_without_a_guessed_scope() {
  let root_path = unique_temp_workspace("natural-empty-scope");
  create_workspace("自然语言空范围草稿", &root_path).expect("workspace should be created");

  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "等待 AI 解析".to_string(),
      source_type: "natural_language".to_string(),
      platforms: Vec::new(),
      data_types: Vec::new(),
    },
  )
  .expect("natural language draft must not require local inference");

  assert_eq!(task.platforms_json, serde_json::json!([]));
  assert_eq!(task.data_types_json, serde_json::json!([]));
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn form_draft_still_requires_an_explicit_scope() {
  let root_path = unique_temp_workspace("form-empty-scope");
  create_workspace("表单范围校验", &root_path).expect("workspace should be created");

  let error = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "表单任务".to_string(),
      source_type: "form".to_string(),
      platforms: Vec::new(),
      data_types: Vec::new(),
    },
  )
  .expect_err("form task must keep explicit scope validation");

  assert!(error.message.contains("平台不能为空"));
  std::fs::remove_dir_all(root_path).ok();
}

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
fn task_diagnostic_views_expose_stable_codes_for_known_chinese_values() {
  let root_path = unique_temp_workspace("task-diagnostic-codes");
  create_workspace("任务诊断代码测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");

  let run = enqueue_task(&root_path, &task.id).expect("task enqueued");
  let logs = list_task_logs(&root_path, &run.id).expect("logs should list");

  assert_eq!(run.current_stage.as_deref(), Some("等待执行"));
  assert_eq!(run.current_stage_code, "WAITING_EXECUTION");
  assert_eq!(logs[0].stage_code, "WAITING_EXECUTION");
  assert_eq!(logs[0].message_code, "TASK_ENQUEUED");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn task_diagnostic_codes_cover_current_runtime_and_migration_vocabulary() {
  for (value, expected) in [
    ("等待执行", "WAITING_EXECUTION"),
    ("执行采集", "COLLECTING"),
    ("持久化采集结果", "PERSISTING_RESULTS"),
    ("已完成", "COMPLETED"),
    ("部分成功", "PARTIAL_SUCCESS"),
    ("执行失败", "EXECUTION_FAILED"),
    ("用户取消", "USER_CANCELLED"),
    ("恢复响应入库", "RECOVERY_PERSIST_RESPONSE"),
    ("恢复重试", "RECOVERY_RETRY"),
    ("恢复待发送", "RECOVERY_READY_TO_SEND"),
    ("恢复续页", "RECOVERY_NEXT_PAGE"),
    ("恢复收尾", "RECOVERY_FINALIZE"),
    ("恢复等待", "RECOVERY_WAITING"),
    ("请求状态不确定", "REQUEST_STATE_UNCERTAIN"),
    ("运行快照不完整", "RUN_SNAPSHOT_INCOMPLETE"),
    ("检查点状态冲突", "CHECKPOINT_STATE_CONFLICT"),
    ("运行步骤状态冲突", "RUN_STEP_STATE_CONFLICT"),
    ("检查点证据不完整", "CHECKPOINT_EVIDENCE_INCOMPLETE"),
    ("检查点终止失败", "CHECKPOINT_TERMINAL_FAILURE"),
    ("恢复指令冲突", "RECOVERY_INSTRUCTION_CONFLICT"),
    ("请求证据需要人工处理", "REQUEST_EVIDENCE_REQUIRES_REVIEW"),
    ("运行快照需要人工处理", "RUN_SNAPSHOT_REQUIRES_REVIEW"),
    ("需要重新确认计划", "PLAN_RECONFIRMATION_REQUIRED"),
    ("活动运行冲突", "ACTIVE_RUN_CONFLICT"),
    ("活动运行冲突迁移", "ACTIVE_RUN_CONFLICT_MIGRATION"),
    ("活动步骤冲突迁移", "ACTIVE_STEP_CONFLICT_MIGRATION"),
    (
      "请求检查点冲突迁移",
      "REQUEST_CHECKPOINT_CONFLICT_MIGRATION",
    ),
  ] {
    assert_eq!(task_stage_code(Some(value)), expected, "stage: {value}");
  }
  assert_eq!(task_stage_code(None), "STAGE_PENDING");

  for (value, expected) in [
    ("任务已加入本地队列", "TASK_ENQUEUED"),
    ("本地执行器已领取任务", "TASK_CLAIMED"),
    ("本地执行器已领取恢复任务", "RECOVERY_TASK_CLAIMED"),
    ("失败任务已重新排队", "FAILED_TASK_REQUEUED"),
    (
      "任务部分目标失败，合格数据已保留",
      "TASK_PARTIALLY_SUCCEEDED",
    ),
    ("全部采集目标失败", "ALL_TARGETS_FAILED"),
    ("任务执行成功", "TASK_SUCCEEDED"),
    ("任务已由用户取消", "TASK_CANCELLED_BY_USER"),
    (
      "队列中存在可能已发送的 TikHub 请求，远端副作用无法确认，禁止自动重发",
      "QUEUED_REQUEST_UNCERTAIN",
    ),
    (
      "运行步骤快照不完整，可能丢失远端请求证据，已停止自动执行",
      "RUN_SNAPSHOT_INCOMPLETE",
    ),
    (
      "任务包含状态不确定的 TikHub 请求，必须人工确认后再处理",
      "UNCERTAIN_REQUEST_REQUIRES_REVIEW",
    ),
    (
      "运行步骤快照不完整，或运行中步骤缺少检查点，禁止自动重发",
      "RUN_SNAPSHOT_INCOMPLETE",
    ),
    (
      "队列恢复指令与运行步骤及检查点证据不一致，已停止自动执行",
      "RECOVERY_INSTRUCTION_CONFLICT",
    ),
    (
      "进程在 TikHub 请求完成前中断，无法确认远端是否已计费或返回，禁止自动重发",
      "INTERRUPTED_REQUEST_UNCERTAIN",
    ),
    (
      "任务存在多个冲突的恢复前沿，无法安全判断下一执行位置",
      "CHECKPOINT_STATE_CONFLICT",
    ),
    (
      "检查点页码或游标链不连续，无法安全判断恢复位置",
      "CHECKPOINT_CURSOR_CHAIN_INVALID",
    ),
    (
      "运行步骤状态与检查点证据不相容，已停止自动恢复",
      "RUN_STEP_STATE_CONFLICT",
    ),
    (
      "已接收或已提交的检查点缺少可验证响应、提交时间或续页游标",
      "CHECKPOINT_EVIDENCE_INCOMPLETE",
    ),
    (
      "任务包含不可重试的失败检查点，已停止自动恢复",
      "CHECKPOINT_TERMINAL_FAILURE",
    ),
    (
      "TikHub 响应已保存，恢复时只继续本地入库，不重新发送请求",
      "RECOVERY_PERSIST_SAVED_RESPONSE",
    ),
    (
      "失败检查点仍在请求、记录和预算限制内，等待安全重试",
      "RECOVERY_RETRY_SAFE",
    ),
    (
      "检查点仍处于 prepared，可从尚未发送的请求继续",
      "RECOVERY_PREPARED_REQUEST",
    ),
    (
      "从已提交检查点的 next_cursor 继续下一页",
      "RECOVERY_CONTINUE_NEXT_PAGE",
    ),
    (
      "已完成步骤没有续页，继续下一个尚未发送的运行步骤",
      "RECOVERY_CONTINUE_NEXT_STEP",
    ),
    (
      "最后一个检查点已提交且没有续页，等待完成本地收尾",
      "RECOVERY_FINALIZE_LOCAL",
    ),
    (
      "运行步骤尚未发送请求，可从待执行步骤继续",
      "RECOVERY_PENDING_STEP",
    ),
    (
      "未发现已发送请求的检查点，任务已重新排队",
      "RECOVERY_REQUEUED_WITHOUT_SENT_REQUEST",
    ),
    (
      "检测到同一任务存在多个活动运行，所有活动运行已停止并要求人工复核",
      "ACTIVE_RUN_CONFLICT_REQUIRES_REVIEW",
    ),
    (
      "活动运行冲突迁移已终止未完成的运行步骤",
      "ACTIVE_STEP_CONFLICT_MIGRATION",
    ),
    (
      "活动运行冲突迁移已将 requesting 检查点转为 uncertain",
      "REQUEST_CHECKPOINT_CONFLICT_MIGRATION",
    ),
    (
      "采集计划不可执行，且运行记录包含已发送请求证据，禁止重新入队，必须人工处理：测试原因",
      "REQUEST_EVIDENCE_REQUIRES_REVIEW",
    ),
    (
      "采集计划不可执行，且运行快照无法证明请求从未发送，禁止重新入队，必须人工处理：测试原因",
      "RUN_SNAPSHOT_REQUIRES_REVIEW",
    ),
    (
      "采集计划不可执行，任务已停止，请重新确认有效的 v2 计划：测试原因",
      "PLAN_RECONFIRMATION_REQUIRED",
    ),
  ] {
    assert_eq!(task_message_code(value), expected, "message: {value}");
  }
}

#[test]
fn task_diagnostic_views_return_explicit_unknown_codes_for_legacy_values() {
  let root_path = unique_temp_workspace("unknown-task-diagnostic-codes");
  create_workspace("未知任务诊断代码测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
  let run = enqueue_task(&root_path, &task.id).expect("task enqueued");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE task_run SET current_stage = '历史自定义阶段' WHERE id = ?1",
      params![run.id],
    )
    .expect("legacy stage should update");
  connection
    .execute(
      "INSERT INTO task_log (
         id, task_run_id, stage, level, message, safe_details_json, created_at
       ) VALUES ('legacy-log', ?1, '历史日志阶段', 'warning', '历史日志正文', '{}',
                 '2026-07-18T00:00:00+00:00')",
      params![run.id],
    )
    .expect("legacy log should insert");
  drop(connection);

  let latest_runs = list_latest_task_runs(&root_path).expect("runs should list");
  let logs = list_task_logs(&root_path, &run.id).expect("logs should list");
  let legacy_log = logs
    .iter()
    .find(|log| log.id == "legacy-log")
    .expect("legacy log should remain readable");

  assert_eq!(latest_runs[0].current_stage_code, "UNKNOWN_STAGE");
  assert_eq!(legacy_log.stage_code, "UNKNOWN_STAGE");
  assert_eq!(legacy_log.message_code, "UNKNOWN_MESSAGE");

  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn latest_task_runs_include_terminal_warning_safe_details_without_n_plus_one_queries() {
  let root_path = unique_temp_workspace("latest-run-safe-details");
  create_workspace("运行错误详情测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
  let run = enqueue_task(&root_path, &task.id).expect("task enqueued");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE task_run
       SET status = 'partial_success', ended_at = ?1, current_stage = '部分成功',
           error_code = 'COST_LIMIT_ERROR', error_message = '已按预算停止'
       WHERE id = ?2",
      params![Utc::now().to_rfc3339(), run.id],
    )
    .expect("run should fail");
  connection
    .execute(
      "INSERT INTO task_log (
         id, task_run_id, stage, level, message, safe_details_json, created_at
       ) VALUES ('terminal-details-log', ?1, '部分成功', 'warning', '已按预算停止',
                 '{\"retry_after\":\"17\",\"retry_attempts\":\"3\"}', ?2)",
      params![run.id, Utc::now().to_rfc3339()],
    )
    .expect("terminal log should insert");
  drop(connection);

  let latest_runs = list_latest_task_runs(&root_path).expect("latest runs should list");

  assert_eq!(latest_runs[0].error_safe_details_json["retry_after"], "17");
  assert_eq!(
    latest_runs[0].error_safe_details_json["retry_attempts"],
    "3"
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn latest_task_runs_follow_monotonic_sequence_when_the_clock_moves_backwards() {
  let root_path = unique_temp_workspace("latest-run-monotonic-sequence");
  create_workspace("运行单调序号测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, plan_id, attempt_number, status, started_at)
       VALUES ('run-before-clock-change', ?1, ?2, 1, 'failed', '2026-07-21T02:00:00Z')",
      params![task.id, plan.id],
    )
    .unwrap();
  connection
    .execute(
      "INSERT INTO task_run (id, task_id, plan_id, attempt_number, status, started_at)
       VALUES ('run-after-clock-change', ?1, ?2, 2, 'failed', '2026-07-21T01:00:00Z')",
      params![task.id, plan.id],
    )
    .unwrap();
  drop(connection);

  let latest_runs = list_latest_task_runs(&root_path).expect("latest runs should list");

  assert_eq!(latest_runs.len(), 1);
  assert_eq!(latest_runs[0].id, "run-after-clock-change");
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn enqueue_and_retry_persist_monotonic_run_sequences() {
  let root_path = unique_temp_workspace("persist-monotonic-run-sequence");
  create_workspace("运行序号写入测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
  let first = enqueue_task(&root_path, &task.id).expect("first run should enqueue");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE task_run SET status = 'failed', retryable = 1 WHERE id = ?1",
      [&first.id],
    )
    .unwrap();
  connection
    .execute(
      "UPDATE collection_task SET status = 'failed' WHERE id = ?1",
      [&task.id],
    )
    .unwrap();
  drop(connection);

  let second = retry_task(&root_path, &task.id, None).expect("retry should enqueue");
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  connection
    .execute(
      "UPDATE task_run
       SET started_at = CASE id
         WHEN ?1 THEN '2026-07-21T02:00:00Z'
         WHEN ?2 THEN '2026-07-21T01:00:00Z'
       END,
       status = 'failed', retryable = CASE id WHEN ?1 THEN 0 ELSE 1 END
       WHERE id IN (?1, ?2)",
      params![first.id, second.id],
    )
    .unwrap();
  connection
    .execute(
      "UPDATE collection_task SET status = 'failed' WHERE id = ?1",
      [&task.id],
    )
    .unwrap();
  drop(connection);
  let third = retry_task(&root_path, &task.id, None)
    .expect("latest monotonic failure should remain retryable after clock rollback");
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  let sequences = connection
    .prepare("SELECT id, run_sequence FROM task_run WHERE task_id = ?1 ORDER BY run_sequence")
    .unwrap()
    .query_map([&task.id], |row| {
      Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })
    .unwrap()
    .collect::<Result<Vec<_>, _>>()
    .unwrap();

  assert_eq!(
    sequences,
    vec![(first.id, 1), (second.id, 2), (third.id, 3)]
  );
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
fn missing_latest_plan_exposes_a_structured_empty_state_reason() {
  let root_path = unique_temp_workspace("missing-latest-task-plan");
  create_workspace("空计划任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");

  let error = get_latest_collection_plan(&root_path, &task.id)
    .expect_err("task without a plan should return a structured empty-state error");

  assert_eq!(error.code, AppErrorCode::ValidationError);
  assert_eq!(error.stage, AppErrorStage::Validation);
  assert_eq!(
    error.safe_details.get("reason").map(String::as_str),
    Some("no_plan")
  );
  assert_eq!(
    error.safe_details.get("entity").map(String::as_str),
    Some("collection_plan")
  );
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
fn version_four_account_plan_saves_scope_cost_and_confirmation() {
  let root_path = unique_temp_workspace("tasks-v4-account");
  create_workspace("v4 账号任务测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "TikTok 账号搜索".to_string(),
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
      selected_fields: vec!["avatar_url".to_string(), "country_region".to_string()],
      enrichment_policy: "auto_costed".to_string(),
      params: serde_json::json!({ "keyword": "electric car" }),
      age_range: None,
      gender_filter: None,
      request_limit: Some(1),
      record_limit: Some(20),
      budget_limit_micros: Some(10_000_000),
    },
  )
  .expect("v4 account draft should generate");
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
  .expect("v4 account plan should save");

  assert_eq!(plan.schema_version, 4);
  assert_eq!(
    plan.validation_status, "valid",
    "{:?}",
    plan.validation_errors_json
  );
  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 21);
  let confirmed =
    confirm_collection_plan(&root_path, &task.id, &plan.id).expect("v4 plan should confirm");
  assert_eq!(confirmed.account_source.as_deref(), Some("user_search"));
  assert_eq!(
    confirmed.selected_fields_json,
    serde_json::json!(["avatar_url", "country_region"])
  );
  let connection = open_workspace_connection(&root_path).unwrap();
  let scope = connection
    .query_row(
      "SELECT account_source, selected_fields_json FROM collection_task WHERE id = ?1",
      [task.id],
      |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .unwrap();
  assert_eq!(scope.0, "user_search");
  assert_eq!(
    serde_json::from_str::<Value>(&scope.1).unwrap(),
    serde_json::json!(["avatar_url", "country_region"])
  );
  assert_eq!(
    connection
      .query_row(
        "SELECT COUNT(*) FROM api_call_step WHERE plan_id = ?1 AND status = 'planned'",
        [plan.id],
        |row| row.get::<_, i64>(0),
      )
      .unwrap(),
    2
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn version_four_confirmation_rejects_endpoint_params_outside_the_runtime_allowlist() {
  let root_path = unique_temp_workspace("tasks-v4-endpoint-allowlist");
  create_workspace("v4 白名单测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "TikTok 用户搜索".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["tiktok".to_string()],
      data_types: vec!["account".to_string()],
    },
  )
  .expect("v4 account task should create");
  let mut draft = crate::collection::generate_account_collection_plan(
    crate::collection::AccountFormCollectionPlanRequest {
      platform: "tiktok".to_string(),
      account_source: "user_search".to_string(),
      selected_fields: Vec::new(),
      enrichment_policy: "auto_costed".to_string(),
      params: serde_json::json!({ "keyword": "pet supplies", "region": "GB", "time_range": "7" }),
      age_range: None,
      gender_filter: None,
      request_limit: Some(1),
      record_limit: Some(1),
      budget_limit_micros: Some(1_000_000),
    },
  )
  .expect("v4 account draft should generate");
  draft.plan_json["steps"][0]["params"]["region"] = serde_json::json!("GB");
  draft.plan_json["steps"][0]["params"]["time_range"] = serde_json::json!("7");

  let plan = save_collection_plan(
    &root_path,
    SaveCollectionPlanInput {
      task_id: task.id.clone(),
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: "valid".to_string(),
      validation_errors_json: Some(serde_json::json!([])),
      cost_estimate_json: Some(draft.cost_estimate_json),
    },
  )
  .expect("invalid plan should remain available for editing");

  assert_eq!(plan.validation_status, "needs_review");
  assert!(plan
    .validation_errors_json
    .as_array()
    .is_some_and(|errors| errors.iter().any(|error| {
      error
        .as_str()
        .is_some_and(|message| message.contains("参数 region 不在 endpoint 白名单内"))
    })));
  let error = confirm_collection_plan(&root_path, &task.id, &plan.id)
    .expect_err("allowlist-invalid plan must not confirm");
  assert_eq!(error.code, AppErrorCode::ValidationError);

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
fn persisted_cost_estimate_ignores_forged_embedded_totals() {
  let root_path = unique_temp_workspace("forged-cost-estimate");
  create_workspace("成本重算测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let mut input = plan_input(&task.id);
  input.plan_json["request_limit"] = serde_json::json!(5);
  input.plan_json["cost_estimate"] = serde_json::json!({
    "request_count_estimate": 999_999,
    "requires_confirmation": false
  });
  input.cost_estimate_json = Some(serde_json::json!({
    "request_count_estimate": 777_777
  }));

  let plan = save_collection_plan(&root_path, input).expect("plan should save");
  let estimate = estimate_task_cost(&root_path, Some(task.id), None).expect("cost should load");

  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 5);
  assert_eq!(estimate.request_count_estimate, 5);
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn persisted_cost_estimate_keeps_dependency_fanout() {
  let root_path = unique_temp_workspace("dependency-fanout-cost");
  create_workspace("依赖扇出成本测试", &root_path).expect("workspace should be created");
  let data_types = vec![
    "keyword_search".to_string(),
    "item_detail".to_string(),
    "account_profile".to_string(),
    "comments".to_string(),
  ];
  let task = create_collection_task(
    &root_path,
    CreateCollectionTaskInput {
      name: "宠物园区".to_string(),
      source_type: "form".to_string(),
      platforms: vec!["xiaohongshu".to_string()],
      data_types: data_types.clone(),
    },
  )
  .expect("task should create");
  let draft = crate::collection::generate_form_collection_plan(
    crate::collection::FormCollectionPlanRequest {
      platform: "xiaohongshu".to_string(),
      data_type: None,
      data_types,
      params: serde_json::json!({
        "keyword": "宠物园区",
        "region": "CN",
        "time_range": "7"
      }),
      age_range: None,
      request_limit: Some(20),
      record_limit: Some(1000),
      budget_limit_micros: Some(2_000_000),
    },
  )
  .expect("plan draft should generate");
  assert_eq!(draft.cost_estimate_json["request_count_estimate"], 22_020);

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
  .expect("plan should save");
  let estimate =
    estimate_task_cost(&root_path, Some(task.id.clone()), None).expect("cost should load");

  assert_eq!(plan.cost_estimate_json["request_count_estimate"], 22_020);
  assert_eq!(estimate.request_count_estimate, 22_020);

  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "UPDATE collection_task SET cost_estimate_json = '{\"request_count_estimate\":80}'
       WHERE id = ?1",
      params![task.id],
    )
    .expect("legacy estimate should be simulated");
  drop(connection);
  let listed = list_tasks(&root_path, None).expect("tasks should list");
  assert_eq!(
    listed[0].cost_estimate_json["request_count_estimate"],
    22_020
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
fn delete_task_removes_managed_raw_report_and_export_files() {
  let root_path = unique_temp_workspace("delete-task-files");
  create_workspace("任务文件删除测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let raw_name = format!("{}.json", "a".repeat(64));
  let raw_relative_path = format!("raw/tikhub/{raw_name}");
  let raw_path = root_path.join(&raw_relative_path);
  std::fs::write(&raw_path, br#"{"aweme_id":"managed-raw"}"#)
    .expect("managed raw snapshot should exist");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO raw_record (
         id, task_id, task_run_id, platform, data_type, platform_record_id,
         raw_file_path, raw_hash, summary_json, collected_at, created_at
       ) VALUES ('managed-raw', ?1, NULL, 'tiktok', 'keyword_search', 'managed-record',
                 ?2, ?3, '{}', ?4, ?4)",
      params![
        task.id,
        raw_relative_path,
        "b".repeat(64),
        "2026-07-20T00:00:00+00:00"
      ],
    )
    .expect("raw snapshot index should insert");
  drop(connection);
  let report = crate::exports::build_report_model(&root_path, &task.id, "summary")
    .expect("report should build");
  let report_path = root_path.join("reports").join(&report.id);
  let export = crate::exports::create_export_job(&root_path, &report.id, "xlsx", None)
    .expect("managed export should build");
  let export_path = export.file_path.expect("managed export path should exist");
  let external_dir = unique_temp_workspace("delete-task-external-export");
  std::fs::create_dir(&external_dir).expect("external export directory should exist");
  let external_export_path = external_dir.join("user-copy.xlsx");
  crate::exports::create_export_job(
    &root_path,
    &report.id,
    "xlsx",
    Some(external_export_path.to_string_lossy().to_string()),
  )
  .expect("explicit external export should build");

  delete_task(&root_path, &task.id).expect("task and managed files should delete");

  assert!(!raw_path.exists(), "raw snapshot must be removed");
  assert!(!report_path.exists(), "report snapshot must be removed");
  assert!(!export_path.exists(), "managed export must be removed");
  assert!(
    external_export_path.exists(),
    "explicit user export outside the workspace must remain"
  );
  let delete_quarantines = std::fs::read_dir(root_path.join("temp"))
    .expect("workspace temp directory should remain readable")
    .filter_map(Result::ok)
    .filter(|entry| {
      entry
        .file_name()
        .to_string_lossy()
        .starts_with("task-delete-")
    })
    .count();
  assert_eq!(
    delete_quarantines, 0,
    "completed deletion must not leave quarantine data"
  );
  std::fs::remove_dir_all(root_path).ok();
  std::fs::remove_dir_all(external_dir).ok();
}

#[test]
fn delete_task_restores_staged_files_when_database_transaction_fails() {
  let root_path = unique_temp_workspace("delete-task-file-rollback");
  create_workspace("任务文件回滚测试", &root_path).expect("workspace should be created");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let raw_name = format!("{}.json", "c".repeat(64));
  let raw_relative_path = format!("raw/tikhub/{raw_name}");
  let raw_path = root_path.join(&raw_relative_path);
  std::fs::write(&raw_path, br#"{"aweme_id":"rollback-raw"}"#)
    .expect("rollback raw snapshot should exist");
  let connection = open_workspace_connection(&root_path).expect("database should open");
  connection
    .execute(
      "INSERT INTO raw_record (
         id, task_id, task_run_id, platform, data_type, platform_record_id,
         raw_file_path, raw_hash, summary_json, collected_at, created_at
       ) VALUES ('rollback-raw', ?1, NULL, 'tiktok', 'keyword_search', 'rollback-record',
                 ?2, ?3, '{}', ?4, ?4)",
      params![
        task.id,
        raw_relative_path,
        "d".repeat(64),
        "2026-07-20T00:00:00+00:00"
      ],
    )
    .expect("rollback raw index should insert");
  connection
    .execute_batch(
      "CREATE TRIGGER fail_delete_audit
       BEFORE INSERT ON audit_log
       WHEN NEW.action = 'delete_task'
       BEGIN
         SELECT RAISE(ABORT, 'forced delete audit failure');
       END;",
    )
    .expect("failure trigger should install");
  drop(connection);

  delete_task(&root_path, &task.id).expect_err("audit failure should roll back deletion");

  assert!(
    raw_path.is_file(),
    "rolled-back raw snapshot must return to its original path"
  );
  assert!(
    get_task(&root_path, &task.id).is_ok(),
    "rolled-back task must remain"
  );
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  assert_eq!(
    count_rows_for(&connection, "raw_record", "task_id", &task.id),
    1,
    "rolled-back raw index must remain"
  );
  let delete_quarantines = std::fs::read_dir(root_path.join("temp"))
    .expect("workspace temp directory should remain readable")
    .filter_map(Result::ok)
    .filter(|entry| {
      entry
        .file_name()
        .to_string_lossy()
        .starts_with("task-delete-")
    })
    .count();
  assert_eq!(
    delete_quarantines, 0,
    "rollback must remove the quarantine directory"
  );
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
fn worker_owner_lock_prevents_another_instance_from_recovering_an_active_run() {
  use std::fs::OpenOptions;
  use std::os::unix::fs::PermissionsExt;

  use fs4::FileExt;

  let root_path = unique_temp_workspace("worker-owner-lock");
  create_workspace("多实例执行器所有权", &root_path).expect("workspace should be created");
  crate::tasks::test_support::install_successful_tikhub_profile(&root_path)
    .expect("TikHub profile should install");
  let task = create_collection_task(&root_path, create_task_input()).expect("task created");
  let plan = save_collection_plan(&root_path, plan_input(&task.id)).expect("plan saved");
  confirm_collection_plan(&root_path, &task.id, &plan.id).expect("plan confirmed");
  enqueue_task(&root_path, &task.id).expect("task enqueued");
  let run = claim_next_task(&root_path)
    .expect("task claim should succeed")
    .expect("queued task should exist");
  let lock_file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .truncate(false)
    .open(root_path.join("temp/task-worker.lock"))
    .expect("worker owner lock file should open");
  FileExt::try_lock(&lock_file).expect("first worker should own the lock");

  assert_eq!(
    recover_interrupted_runs(&root_path).expect("busy worker should be skipped"),
    0
  );
  assert!(execute_next_task(&root_path)
    .expect("busy worker should not execute")
    .is_none());
  let connection = open_workspace_connection(&root_path).expect("database should reopen");
  assert_eq!(
    get_task_run(&connection, &run.id)
      .expect("active run should remain")
      .status,
    "running"
  );
  assert_eq!(
    get_task(&root_path, &task.id)
      .expect("active task should remain")
      .status,
    "running"
  );
  assert_eq!(
    lock_file.metadata().unwrap().permissions().mode() & 0o7777,
    0o600
  );
  drop(connection);
  drop(lock_file);
  assert_eq!(
    recover_interrupted_runs(&root_path).expect("released owner should allow recovery"),
    1
  );
  std::fs::remove_dir_all(root_path).ok();
}

#[test]
fn worker_owner_lock_rejects_a_symlink_without_touching_its_target() {
  use std::os::unix::fs::symlink;

  let root_path = unique_temp_workspace("worker-owner-symlink");
  create_workspace("执行器锁路径安全", &root_path).expect("workspace should be created");
  let sentinel = root_path.join("sentinel.txt");
  std::fs::write(&sentinel, "preserve").expect("sentinel should be created");
  symlink(&sentinel, root_path.join("temp/task-worker.lock"))
    .expect("malicious lock symlink should be created");

  let error = execute_next_task(&root_path).expect_err("worker lock symlink must fail closed");

  assert_eq!(error.code, AppErrorCode::PermissionError);
  assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), "preserve");
  std::fs::remove_dir_all(root_path).ok();
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
