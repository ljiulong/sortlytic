import { describe, expect, it } from 'vitest'
import type { CollectionTranslator } from './collection-options'
import {
  localizeActionMessage,
  localizedCostEstimate,
  localizePlanMessage,
  localizePricingMessage,
} from './collection-plan-localization'

const t = ((key: string) => `translated:${key}`) as CollectionTranslator

describe('collection plan message localization', () => {
  it('preserves actionable redacted Chinese backend messages in the operation area', () => {
    const validation = '地区和时间范围不能作为“小红书 · 搜索用户”端点的请求参数'
    const database = '数据库正在忙，已保留最近一次成功快照'
    const pricing = 'TikHub 余额读取失败，请重新测试当前配置'
    const estimate = '成本估算暂不可用：端点报价返回临时错误'

    expect(localizePlanMessage(t, validation)).toBe(validation)
    expect(localizeActionMessage(t, database)).toBe(database)
    expect(localizePricingMessage(t, pricing)).toBe(pricing)
    expect(localizedCostEstimate(t, estimate, 'zh-CN')).toBe(estimate)
  })

  it('uses the unknown fallback only when an action message is genuinely empty', () => {
    expect(localizeActionMessage(t, '')).toBe('translated:action.unknown')
    expect(localizePlanMessage(t, undefined)).toBe('')
  })
})
