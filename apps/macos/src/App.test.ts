import { describe, expect, it } from 'vitest'
import { localizeBackendMessage, parseFeedbackNavigationTarget } from './App'

describe('工作台状态消息分类', () => {
  it('预算超限和计价限流不得回退成未知错误', () => {
    expect(localizeBackendMessage('TikHub 实时报价超过计划预算上限')).toEqual({
      key: 'error.pricingExceedsBudget',
    })
    expect(localizeBackendMessage('TikHub 请求失败，HTTP 429：请求过于频繁')).toEqual({
      key: 'error.pricingRateLimited',
    })
  })

  it('未登记的中文 Schema、数据库和参数错误保留真实消息', () => {
    for (const message of [
      'AI 服务请求超时',
      '数据库正在忙，请稍后重新读取',
      '地区和时间范围不能作为“小红书 · 搜索用户”端点的请求参数',
    ]) {
      expect(localizeBackendMessage(message)).toEqual({
        key: 'error.raw',
        options: { message },
      })
    }
  })

  it('只有空消息才使用未知错误兜底', () => {
    expect(localizeBackendMessage('')).toEqual({ key: 'error.unknown' })
  })
})

describe('自然语言反馈导航', () => {
  it('AI 配置错误进入设置，诊断记录进入任务页', () => {
    expect(parseFeedbackNavigationTarget('ai_settings')).toBe('settings')
    expect(parseFeedbackNavigationTarget('diagnostics')).toBe('tasks')
  })
})
