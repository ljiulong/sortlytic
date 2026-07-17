import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import {
  CollectionBuilder,
  CollectionPlanPreview,
} from './CollectionBuilder'
import {
  naturalIntentDefault,
  newCollectionFormDefaults,
  normalizeNaturalIntent,
} from './collection-form-defaults'
import { countryRegionSelectOptions } from './collection-select-options'
import {
  collectionFormSchema,
  countryRegionOptions,
  supportsRegionSelection,
} from './collection-options'
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

function renderBuilder(activePlan?: RuntimeCollectionPlan) {
  return renderToStaticMarkup(
    createElement(CollectionBuilder, {
      actionMessage: '等待生成',
      activePlan,
      isBusy: false,
      onConfirmPlan: vi.fn(async () => undefined),
      onGenerateFormPlan: vi.fn(async () => draftPlan),
      onGenerateNaturalPlan: vi.fn(async () => draftPlan),
    }),
  )
}

describe('CollectionPlanPreview', () => {
  it('计划卡按头部、事实区、成本区和稳定底栏组织', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '等待确认',
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: draftPlan,
      }),
    )

    expect(markup).toContain('collection-plan__header')
    expect(markup).toContain('collection-plan__facts')
    expect(markup).toContain('collection-plan__pricing')
    expect(markup).toContain('collection-plan__footer')
    expect(markup).not.toContain('plan-grid')
  })

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
    range: '180',
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

  it('新建任务使用四个业务分组和完整计划空状态', () => {
    const markup = renderBuilder()

    expect(markup).toContain('collection-builder')
    expect(markup).toContain('01 来源与目标')
    expect(markup).toContain('02 采集范围')
    expect(markup).toContain('03 数量与成本')
    expect(markup).toContain('04 公开信息筛选')
    expect(markup).toContain('collection-plan-empty')
    expect(markup).toContain('尚未生成计划')
  })

  it('平台与国家地区使用应用内下拉，且地区代码仍完整', () => {
    const markup = renderBuilder()
    const regionCodes = countryRegionOptions.map(({ code }) => code)
    const unitedStates = countryRegionSelectOptions.find(({ value }) => value === 'US')

    expect(markup).toMatch(/<button[^>]*id="platform"[^>]*aria-haspopup="listbox"/)
    expect(markup).toMatch(/<button[^>]*id="region-code"[^>]*aria-haspopup="listbox"/)
    expect(markup).toMatch(/<button[^>]*id="range"[^>]*aria-haspopup="listbox"/)
    expect(markup).not.toMatch(/<input[^>]*id="range"/)
    expect(markup).not.toContain('<select')
    expect(markup).not.toContain('<datalist')
    expect(regionCodes).toContain('CN')
    expect(regionCodes).toContain('US')
    expect(regionCodes).toContain('JP')
    expect(unitedStates).toMatchObject({
      label: '美国',
      description: 'United States',
      meta: 'US',
    })
    expect(unitedStates?.keywords).toContain('美国 United States US')
  })

  it('时间范围只接受平台能力中的规范值，不再接受任意文本', () => {
    for (const range of ['1', '7', '180']) {
      expect(collectionFormSchema.safeParse({ ...baseInput, range }).success).toBe(true)
    }
    expect(collectionFormSchema.safeParse({
      ...baseInput,
      platform: 'TikTok',
      range: '30',
    }).success).toBe(true)
    expect(collectionFormSchema.safeParse({ ...baseInput, range: '30' }).success).toBe(false)
    expect(collectionFormSchema.safeParse({
      ...baseInput,
      platform: '抖音',
      range: '30',
    }).success).toBe(false)
    expect(collectionFormSchema.safeParse({
      ...baseInput,
      range: '最近一个月',
    }).success).toBe(false)
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
