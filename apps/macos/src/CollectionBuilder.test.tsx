import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import {
  CollectionPlanPreview,
} from './CollectionBuilder'
import { collectionFormSchema, supportsRegionSelection } from './collection-options'
import type { RuntimeCollectionPlan } from './use-workbench-backend'

const draftPlan: RuntimeCollectionPlan = {
  platform: '小红书',
  dataType: '笔记详情',
  regionCode: '',
  keyword: '新能源汽车',
  range: '近 30 天',
  maxRecords: 1200,
  budget: 35,
  status: '等待确认',
  missing: [],
}

describe('CollectionPlanPreview', () => {
  it('未生成后端计划时保留完整预览并禁用确认按钮', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '先生成计划',
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: draftPlan,
      }),
    )

    expect(markup).toContain('新能源汽车')
    expect(markup).toContain('笔记详情')
    expect(markup).toContain('先生成计划')
    expect(markup).toMatch(/<button[^>]*disabled=""/)
  })
})

describe('collection form controls', () => {
  const baseInput = {
    platform: '小红书',
    dataType: '关键词搜索',
    dataTypes: ['keyword_search'],
    regionCode: 'CN',
    keyword: '新能源汽车',
    range: '近 30 天',
    maxRecords: 1200,
    budget: 35,
    ageRangeEnabled: false,
  }

  it('数据类型至少选择一项，且年龄范围使用闭区间校验', () => {
    expect(
      collectionFormSchema.safeParse({ ...baseInput, dataTypes: [] }).success,
    ).toBe(false)
    expect(
      collectionFormSchema.safeParse({
        ...baseInput,
        ageRangeEnabled: true,
        ageMin: 18,
        ageMax: 18,
      }).success,
    ).toBe(true)
    expect(
      collectionFormSchema.safeParse({
        ...baseInput,
        ageRangeEnabled: true,
        ageMin: 31,
        ageMax: 18,
      }).success,
    ).toBe(false)
  })

  it('只有所选平台和数据类型存在地区执行能力时启用地区选择', () => {
    expect(supportsRegionSelection('小红书', ['item_detail'])).toBe(false)
    expect(
      supportsRegionSelection('小红书', ['item_detail', 'keyword_search']),
    ).toBe(true)
    expect(supportsRegionSelection('TikTok', ['account_posts'])).toBe(true)
  })
})
