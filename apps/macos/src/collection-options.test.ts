import { describe, expect, it } from 'vitest'
import {
  AGE_RANGE_LIMITS,
  collectionFormSchema,
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

describe('collectionFormSchema time range', () => {
  const base = {
    accountSource: 'user_search',
    selectedFields: [],
    dataType: '关键词搜索',
    dataTypes: ['keyword_search'],
    regionCode: '',
    keyword: 'pet supplies',
    maxRecords: 10,
    budget: 0.1,
    ageRangeEnabled: false,
    genderFilterEnabled: false,
    genders: [],
  }

  it('只校验规范时间值，来源是否支持由能力目录决定', () => {
    for (const platform of ['TikTok', '抖音', '小红书'] as const) {
      for (const range of ['1', '7', '30', '180']) {
        expect(collectionFormSchema.safeParse({ ...base, platform, range }).success).toBe(true)
      }
    }
    expect(collectionFormSchema.safeParse({ ...base, platform: 'TikTok', range: '最近一周' }).success)
      .toBe(false)
  })
})

describe('countryRegionOptions', () => {
  it('提供完整且不重复的 ISO 两位代码，并保留中英文名称', () => {
    const codes = countryRegionOptions.map((option) => option.code)

    expect(codes).toHaveLength(249)
    expect(new Set(codes).size).toBe(codes.length)
    expect(countryRegionOptions.find(({ code }) => code === 'CN')).toMatchObject({
      code: 'CN',
      label: '中国（CN）',
      nameZh: '中国',
      nameEn: 'China',
    })
    expect(countryRegionOptions.find(({ code }) => code === 'US')).toMatchObject({
      code: 'US',
      label: '美国（US）',
      nameZh: '美国',
      nameEn: 'United States',
    })
    expect(countryRegionOptions.every(({ nameZh, nameEn }) => nameZh && nameEn)).toBe(true)
  })
})

describe('AGE_RANGE_LIMITS', () => {
  it('使用包含 0 和 130 的闭区间', () => {
    expect(AGE_RANGE_LIMITS).toEqual({ min: 0, max: 130 })
  })
})
