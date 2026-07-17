import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import {
  CollectionPlanPreview,
} from './CollectionBuilder'
import {
  naturalIntentDefault,
  newCollectionFormDefaults,
  normalizeNaturalIntent,
} from './collection-form-defaults'
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
    expect(markup).toContain('请先生成并保存采集计划')
    expect(markup).toMatch(/<button[^>]*disabled=""/)
  })

  it('后端校验未通过时显示第一条可操作阻塞原因', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '计划需要修正',
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: {
          ...draftPlan,
          taskId: 'task-1',
          planId: 'plan-1',
          validationStatus: 'needs_review',
          status: '待人工确认',
          missing: ['年龄范围必须填写上下限', '价格未知'],
        },
      }),
    )

    expect(markup).toContain('暂不能运行：年龄范围必须填写上下限')
    expect(markup).toMatch(/<button[^>]*disabled=""/)
  })

  it('实时计价或双额度预检未通过时禁用确认并显示原因', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '等待确认',
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: {
          ...draftPlan,
          taskId: 'task-1',
          planId: 'plan-1',
          validationStatus: 'valid',
          pricingReady: false,
          pricingBlocker: 'TikHub 免费额度与充值余额合计不足',
        },
      }),
    )

    expect(markup).toContain('暂不能运行：TikHub 免费额度与充值余额合计不足')
    expect(markup).toMatch(/<button[^>]*disabled=""/)
  })

  it('计划和实时计价均有效时允许确认运行', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '等待确认',
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: {
          ...draftPlan,
          taskId: 'task-1',
          planId: 'plan-1',
          validationStatus: 'valid',
          pricingReady: true,
        },
      }),
    )

    expect(markup).toContain('确认运行')
    expect(markup).not.toMatch(/<button[^>]*disabled=""/)
  })

  it('确认前展示已启用的明确性别筛选', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '等待确认',
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: {
          ...draftPlan,
          genderFilterEnabled: true,
          genders: ['female', 'other'],
        },
      }),
    )

    expect(markup).toContain('女性、其他明确性别')
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
    genderFilterEnabled: false,
    genders: [],
  }

  it('新建表单不把示例任务参数作为实际默认值', () => {
    expect(newCollectionFormDefaults).toMatchObject({
      dataTypes: [],
      regionCode: '',
      keyword: '',
      range: '',
      maxRecords: undefined,
      budget: undefined,
    })
  })

  it('自然语言入口不预填具体任务，并在提交前去除首尾空白', () => {
    expect(naturalIntentDefault).toBe('')
    expect(normalizeNaturalIntent('  采集公开账号  ')).toBe('采集公开账号')
    expect(normalizeNaturalIntent('   ')).toBe('')
  })

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

  it('性别筛选默认关闭，启用后至少选择一种明确性别', () => {
    expect(collectionFormSchema.safeParse(baseInput).success).toBe(true)
    expect(
      collectionFormSchema.safeParse({
        ...baseInput,
        genderFilterEnabled: true,
      }).success,
    ).toBe(false)
    const parsed = collectionFormSchema.safeParse({
      ...baseInput,
      genderFilterEnabled: true,
      genders: ['female', 'other'],
    })
    expect(parsed.success).toBe(true)
    if (parsed.success) expect(parsed.data.genders).toEqual(['female', 'other'])
  })

  it('只有所选平台和数据类型存在地区执行能力时启用地区选择', () => {
    expect(supportsRegionSelection('小红书', ['item_detail'])).toBe(false)
    expect(
      supportsRegionSelection('小红书', ['item_detail', 'keyword_search']),
    ).toBe(true)
    expect(supportsRegionSelection('TikTok', ['account_posts'])).toBe(true)
  })
})
