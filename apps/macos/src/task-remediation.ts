export type TaskRemediationAction =
  | 'edit_task'
  | 'open_ai_settings'
  | 'open_tikhub_settings'
  | 'retry'
  | 'reload'
  | 'workspace_health'
  | 'view_diagnostics'

export type TaskRemediation = {
  message: string
  primaryAction: TaskRemediationAction
  secondaryAction?: TaskRemediationAction
}

export function remediationForTaskProblem(
  code?: string | null,
  message?: string | null,
): TaskRemediation {
  if (code === 'VALIDATION_ERROR') {
    return {
      message: message?.includes('请求参数') || message?.includes('地区和时间范围')
        ? '点击“编辑任务”，移除当前来源不支持的地区或时间条件，或更换具有明确筛选能力的平台或来源；原失败运行和日志会保留。'
        : '点击“编辑任务”，修正计划字段、来源或筛选条件后保存为新计划版本。',
      primaryAction: 'edit_task',
      secondaryAction: 'view_diagnostics',
    }
  }
  if (['MODEL_CONFIG_ERROR', 'MODEL_AUTH_ERROR', 'MODEL_PROTOCOL_ERROR', 'MODEL_NOT_FOUND'].includes(code ?? '')) {
    return {
      message: '打开 AI 设置，检查 Base URL、API Key、模型 ID，并完成真实连通性测试后重新解析。',
      primaryAction: 'open_ai_settings',
      secondaryAction: 'view_diagnostics',
    }
  }
  if (code === 'MODEL_SCHEMA_ERROR') {
    return {
      message: '编辑任务并补齐缺失字段，或调整当前提示词版本后重新解析。',
      primaryAction: 'edit_task',
      secondaryAction: 'open_ai_settings',
    }
  }
  if (code === 'MODEL_RATE_LIMIT') {
    return {
      message: '按服务端建议等待后重新解析；系统不会自动重复可能已计费的模型请求。',
      primaryAction: 'retry',
      secondaryAction: 'view_diagnostics',
    }
  }
  if (code === 'TIKHUB_AUTH_ERROR') {
    return {
      message: '打开 TikHub 设置，重新输入密钥并完成真实余额与额度测试。',
      primaryAction: 'open_tikhub_settings',
      secondaryAction: 'view_diagnostics',
    }
  }
  if (code === 'TIKHUB_RATE_LIMIT') {
    return {
      message: '根据 Retry-After 等待后重试；保留当前运行证据，禁止立即重复请求。',
      primaryAction: 'retry',
      secondaryAction: 'view_diagnostics',
    }
  }
  if (code === 'DATABASE_ERROR') {
    return {
      message: '重新读取本地工作区；数据库繁忙时会先短暂等待，不需要重建任务。',
      primaryAction: 'reload',
      secondaryAction: 'view_diagnostics',
    }
  }
  if (['PERMISSION_ERROR', 'WORKSPACE_ERROR'].includes(code ?? '')) {
    return {
      message: '运行工作区健康检查，修复目录权限或工作区状态后重新读取。',
      primaryAction: 'workspace_health',
      secondaryAction: 'view_diagnostics',
    }
  }
  if (code === 'COST_LIMIT_ERROR') {
    return {
      message: '编辑预算或缩小最大记录数，保存新计划版本后重新确认。',
      primaryAction: 'edit_task',
      secondaryAction: 'view_diagnostics',
    }
  }
  return {
    message: '保留当前任务和失败记录；查看诊断详情后编辑任务或重新执行安全操作。',
    primaryAction: 'view_diagnostics',
    secondaryAction: 'edit_task',
  }
}
