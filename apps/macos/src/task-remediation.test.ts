import { describe, expect, it } from 'vitest'
import { remediationForTaskProblem } from './task-remediation'

describe('任务错误修改方式', () => {
  it.each([
    ['VALIDATION_ERROR', '编辑任务'],
    ['MODEL_CONFIG_ERROR', '打开 AI 设置'],
    ['MODEL_AUTH_ERROR', '打开 AI 设置'],
    ['MODEL_RATE_LIMIT', '等待'],
    ['TIKHUB_AUTH_ERROR', '打开 TikHub 设置'],
    ['TIKHUB_RATE_LIMIT', 'Retry-After'],
    ['DATABASE_ERROR', '重新读取'],
    ['PERMISSION_ERROR', '工作区健康检查'],
    ['WORKSPACE_ERROR', '工作区健康检查'],
    ['COST_LIMIT_ERROR', '编辑预算'],
  ])('%s 提供明确操作', (code, expected) => {
    expect(remediationForTaskProblem(code).message).toContain(expected)
  })

  it('端点白名单错误给出当前小红书失败任务可直接执行的修改方式', () => {
    const remediation = remediationForTaskProblem('VALIDATION_ERROR',
      '地区和时间范围不能作为“小红书 · 搜索用户”端点的请求参数')

    expect(remediation.message).toContain('移除当前来源不支持的地区或时间条件')
    expect(remediation.message).toContain('更换具有明确筛选能力的平台或来源')
    expect(remediation.primaryAction).toBe('edit_task')
  })

  it('历史配置失败即使使用通用校验码也仍指向 AI 设置', () => {
    const remediation = remediationForTaskProblem(
      'VALIDATION_ERROR',
      '尚未设置当前 AI 配置，请先在设置中完成真实连通性测试',
    )

    expect(remediation.primaryAction).toBe('open_ai_settings')
    expect(remediation.message).toContain('打开 AI 设置')
  })

  it.each([
    ['MODEL_REQUEST_ERROR', { transport_kind: 'timeout' }],
    ['MODEL_PROTOCOL_ERROR', { transport_kind: 'connect' }],
    ['MODEL_PROTOCOL_ERROR', { http_status: '503' }],
  ])('%s 临时失败提供重新尝试而不是错误指向 AI 设置', (code, safeDetails) => {
    const remediation = remediationForTaskProblem(
      code,
      'AI 服务暂时不可用，请稍后重试',
      true,
      safeDetails,
    )

    expect(remediation.primaryAction).toBe('retry')
    expect(remediation.message).toContain('重新解析')
    expect(remediation.message).not.toContain('打开 AI 设置')
  })

  it('不可重试的真实模型协议错误仍指向 AI 设置', () => {
    expect(remediationForTaskProblem(
      'MODEL_PROTOCOL_ERROR',
      'AI 供应商类型与协议不匹配',
      false,
      {},
    ).primaryAction).toBe('open_ai_settings')
  })

  it('未知错误仍保留记录并允许查看诊断和编辑，不只显示稍后重试', () => {
    expect(remediationForTaskProblem('UNCLASSIFIED_ERROR')).toEqual({
      message: '保留当前任务和失败记录；查看诊断详情后编辑任务或重新执行安全操作。',
      primaryAction: 'view_diagnostics',
      secondaryAction: 'edit_task',
    })
  })
})
