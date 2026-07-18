import { describe, expect, it } from 'vitest'
import { localizeBackendMessage } from './App'

describe('工作台状态消息分类', () => {
  it('预算超限和计价限流不得回退成未知错误', () => {
    expect(localizeBackendMessage('TikHub 实时报价超过计划预算上限')).toEqual({
      key: 'error.pricingExceedsBudget',
    })
    expect(localizeBackendMessage('TikHub 请求失败，HTTP 429：请求过于频繁')).toEqual({
      key: 'error.pricingRateLimited',
    })
  })
})
