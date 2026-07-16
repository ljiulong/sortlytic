import { describe, expect, it } from 'vitest'
import {
  AGE_RANGE_LIMITS,
  collectionDataTypeOptions,
  countryRegionOptions,
} from './collection-options'

describe('collectionDataTypeOptions', () => {
  it('公开五类 TikHub 核心采集目标且内部值稳定', () => {
    expect(collectionDataTypeOptions.map((option) => option.value)).toEqual([
      'keyword_search',
      'item_detail',
      'account_profile',
      'account_posts',
      'comments',
    ])
  })
})

describe('countryRegionOptions', () => {
  it('提供完整且不重复的 ISO 两位代码并显示中文名称', () => {
    const codes = countryRegionOptions.map((option) => option.code)

    expect(codes.length).toBeGreaterThanOrEqual(249)
    expect(new Set(codes).size).toBe(codes.length)
    expect(countryRegionOptions).toContainEqual({ code: 'CN', label: '中国（CN）' })
    expect(countryRegionOptions).toContainEqual({ code: 'US', label: '美国（US）' })
  })
})

describe('AGE_RANGE_LIMITS', () => {
  it('使用包含 0 和 130 的闭区间', () => {
    expect(AGE_RANGE_LIMITS).toEqual({ min: 0, max: 130 })
  })
})
