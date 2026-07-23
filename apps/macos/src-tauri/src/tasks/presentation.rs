pub(super) fn task_stage_code(stage: Option<&str>) -> &'static str {
  match stage {
    None => "STAGE_PENDING",
    Some("等待执行") => "WAITING_EXECUTION",
    Some("执行采集") => "COLLECTING",
    Some("持久化采集结果") => "PERSISTING_RESULTS",
    Some("已完成") => "COMPLETED",
    Some("部分成功") => "PARTIAL_SUCCESS",
    Some("执行失败") => "EXECUTION_FAILED",
    Some("用户取消") => "USER_CANCELLED",
    Some("恢复响应入库") => "RECOVERY_PERSIST_RESPONSE",
    Some("恢复重试") => "RECOVERY_RETRY",
    Some("恢复待发送") => "RECOVERY_READY_TO_SEND",
    Some("恢复续页") => "RECOVERY_NEXT_PAGE",
    Some("恢复收尾") => "RECOVERY_FINALIZE",
    Some("恢复等待") => "RECOVERY_WAITING",
    Some("请求状态不确定") => "REQUEST_STATE_UNCERTAIN",
    Some("运行快照不完整") => "RUN_SNAPSHOT_INCOMPLETE",
    Some("检查点状态冲突") => "CHECKPOINT_STATE_CONFLICT",
    Some("运行步骤状态冲突") => "RUN_STEP_STATE_CONFLICT",
    Some("检查点证据不完整") => "CHECKPOINT_EVIDENCE_INCOMPLETE",
    Some("检查点终止失败") => "CHECKPOINT_TERMINAL_FAILURE",
    Some("恢复指令冲突") => "RECOVERY_INSTRUCTION_CONFLICT",
    Some("请求证据需要人工处理") => "REQUEST_EVIDENCE_REQUIRES_REVIEW",
    Some("运行快照需要人工处理") => "RUN_SNAPSHOT_REQUIRES_REVIEW",
    Some("需要重新确认计划") => "PLAN_RECONFIRMATION_REQUIRED",
    Some("活动运行冲突") => "ACTIVE_RUN_CONFLICT",
    Some("活动运行冲突迁移") => "ACTIVE_RUN_CONFLICT_MIGRATION",
    Some("活动步骤冲突迁移") => "ACTIVE_STEP_CONFLICT_MIGRATION",
    Some("请求检查点冲突迁移") => "REQUEST_CHECKPOINT_CONFLICT_MIGRATION",
    Some(_) => "UNKNOWN_STAGE",
  }
}

pub(super) fn task_message_code(message: &str) -> &'static str {
  match message {
    "任务已加入本地队列" => "TASK_ENQUEUED",
    "本地执行器已领取任务" => "TASK_CLAIMED",
    "本地执行器已领取恢复任务" => "RECOVERY_TASK_CLAIMED",
    "失败任务已重新排队" => "FAILED_TASK_REQUEUED",
    "任务部分目标失败，合格数据已保留" => "TASK_PARTIALLY_SUCCEEDED",
    "全部采集目标失败" => "ALL_TARGETS_FAILED",
    "任务执行成功" => "TASK_SUCCEEDED",
    "任务已由用户取消" => "TASK_CANCELLED_BY_USER",
    "队列中存在可能已发送的 TikHub 请求，远端副作用无法确认，禁止自动重发" => {
      "QUEUED_REQUEST_UNCERTAIN"
    }
    "运行步骤快照不完整，可能丢失远端请求证据，已停止自动执行"
    | "运行步骤快照不完整，或运行中步骤缺少检查点，禁止自动重发" => {
      "RUN_SNAPSHOT_INCOMPLETE"
    }
    "队列恢复指令与运行步骤及检查点证据不一致，已停止自动执行" => {
      "RECOVERY_INSTRUCTION_CONFLICT"
    }
    "进程在 TikHub 请求完成前中断，无法确认远端是否已计费或返回，禁止自动重发" => {
      "INTERRUPTED_REQUEST_UNCERTAIN"
    }
    "任务包含状态不确定的 TikHub 请求，必须人工确认后再处理" => {
      "UNCERTAIN_REQUEST_REQUIRES_REVIEW"
    }
    "任务存在多个冲突的恢复前沿，无法安全判断下一执行位置" => {
      "CHECKPOINT_STATE_CONFLICT"
    }
    "检查点页码或游标链不连续，无法安全判断恢复位置" => {
      "CHECKPOINT_CURSOR_CHAIN_INVALID"
    }
    "运行步骤状态与检查点证据不相容，已停止自动恢复" => {
      "RUN_STEP_STATE_CONFLICT"
    }
    "已接收或已提交的检查点缺少可验证响应、提交时间或续页游标" => {
      "CHECKPOINT_EVIDENCE_INCOMPLETE"
    }
    "任务包含不可重试的失败检查点，已停止自动恢复" => {
      "CHECKPOINT_TERMINAL_FAILURE"
    }
    "TikHub 响应已保存，恢复时只继续本地入库，不重新发送请求" => {
      "RECOVERY_PERSIST_SAVED_RESPONSE"
    }
    "失败检查点仍在请求、记录和预算限制内，等待安全重试" => {
      "RECOVERY_RETRY_SAFE"
    }
    "检查点仍处于 prepared，可从尚未发送的请求继续" => {
      "RECOVERY_PREPARED_REQUEST"
    }
    "从已提交检查点的 next_cursor 继续下一页" => "RECOVERY_CONTINUE_NEXT_PAGE",
    "已完成步骤没有续页，继续下一个尚未发送的运行步骤" => {
      "RECOVERY_CONTINUE_NEXT_STEP"
    }
    "最后一个检查点已提交且没有续页，等待完成本地收尾" => {
      "RECOVERY_FINALIZE_LOCAL"
    }
    "运行步骤尚未发送请求，可从待执行步骤继续" => "RECOVERY_PENDING_STEP",
    "未发现已发送请求的检查点，任务已重新排队" => {
      "RECOVERY_REQUEUED_WITHOUT_SENT_REQUEST"
    }
    "检测到同一任务存在多个活动运行，所有活动运行已停止并要求人工复核" => {
      "ACTIVE_RUN_CONFLICT_REQUIRES_REVIEW"
    }
    "活动运行冲突迁移已终止未完成的运行步骤" => "ACTIVE_STEP_CONFLICT_MIGRATION",
    "活动运行冲突迁移已将 requesting 检查点转为 uncertain" => {
      "REQUEST_CHECKPOINT_CONFLICT_MIGRATION"
    }
    value
      if value.starts_with(
        "采集计划不可执行，且运行记录包含已发送请求证据，禁止重新入队，必须人工处理：",
      ) =>
    {
      "REQUEST_EVIDENCE_REQUIRES_REVIEW"
    }
    value
      if value.starts_with(
        "采集计划不可执行，且运行快照无法证明请求从未发送，禁止重新入队，必须人工处理：",
      ) =>
    {
      "RUN_SNAPSHOT_REQUIRES_REVIEW"
    }
    value if value.starts_with("采集计划不可执行，任务已停止，请重新确认有效的 v2 计划：") => {
      "PLAN_RECONFIRMATION_REQUIRED"
    }
    _ => "UNKNOWN_MESSAGE",
  }
}
