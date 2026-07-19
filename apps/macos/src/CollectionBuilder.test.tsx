// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const backendApiMocks = vi.hoisted(() => ({
  getAccountCollectionCapabilities: vi.fn(),
  listPlatformDataTypes: vi.fn(),
}))

vi.mock('./backend-api', async (importOriginal) => ({
  ...await importOriginal<typeof import('./backend-api')>(),
  ...backendApiMocks,
}))

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
import { i18n } from './i18n'
import type { AccountCollectionCapabilityView } from './backend-api'
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

const sourceAwareAccountCapability: AccountCollectionCapabilityView = {
  catalog_version: 1,
  platform: 'douyin',
  display_name: '抖音',
  account_sources: [],
  field_groups: [],
  fields: [
    {
      key: 'bio', group: 'profile', display_name: '简介', description: '',
      value_type: 'text', availability: 'enrichment', default_selected: true,
      required_operation_keys: ['enrich.profile'], covered_by_source_keys: ['user_search'],
    },
    {
      key: 'age', group: 'demographics', display_name: '年龄', description: '',
      value_type: 'integer', availability: 'enrichment', default_selected: false,
      required_operation_keys: ['enrich.extended_demographics'], covered_by_source_keys: [],
    },
    {
      key: 'followers_count', group: 'statistics', display_name: '粉丝数', description: '',
      value_type: 'integer', availability: 'enrichment', default_selected: true,
      required_operation_keys: ['enrich.profile'], covered_by_source_keys: ['user_search'],
    },
    {
      key: 'last_posted_at', group: 'activity', display_name: '最近发文', description: '',
      value_type: 'timestamp', availability: 'enrichment', default_selected: true,
      required_operation_keys: ['enrich.account_posts'], covered_by_source_keys: [],
    },
  ],
}

type MountedBuilder = {
  container: HTMLDivElement
  root: Root
}

const mountedBuilders = new Set<MountedBuilder>()

function mountBuilder({
  onGenerateFormPlan = vi.fn(async () => draftPlan),
  onGenerateNaturalPlan,
}: {
  onGenerateFormPlan?: (values: unknown) => Promise<RuntimeCollectionPlan>
  onGenerateNaturalPlan: (intentText: string) => Promise<RuntimeCollectionPlan>
}) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  document.body.append(container)
  mountedBuilders.add(mounted)

  act(() => root.render(createElement(CollectionBuilder, {
    actionMessage: '等待生成',
    isBusy: false,
    onConfirmPlan: vi.fn(async () => undefined),
    onGenerateFormPlan,
    onGenerateNaturalPlan,
  })))

  return mounted
}

function findButton(container: HTMLElement, text: string) {
  return Array.from(container.querySelectorAll('button'))
    .find((button) => button.textContent?.includes(text))
}

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  await i18n.changeLanguage('zh-CN')
  backendApiMocks.getAccountCollectionCapabilities.mockResolvedValue({
    catalog_version: 1,
    platform: 'tiktok',
    display_name: 'TikTok',
    account_sources: [{
      key: 'user_search',
      display_name: '搜索用户',
      description: '按关键词搜索公开账号。',
      input_kind: 'keyword',
      endpoint_key: 'tiktok.user_search',
      pagination_mode: 'cursor',
      max_page_size: 20,
      max_request_count: 100,
    }, {
      key: 'direct_account',
      display_name: '指定账号',
      description: '读取指定公开账号。',
      input_kind: 'account',
      endpoint_key: 'tiktok.account_profile',
      pagination_mode: 'single',
      max_page_size: 1,
      max_request_count: 1,
    }],
    field_groups: [{ key: 'profile', display_name: '账号资料' }],
    fields: [{
      key: 'avatar_url',
      group: 'profile',
      display_name: '头像',
      description: '账号公开头像地址。',
      value_type: 'text',
      availability: 'direct',
      default_selected: true,
      required_operation_keys: [],
    }],
  })
  backendApiMocks.listPlatformDataTypes.mockResolvedValue([{
    data_type: 'keyword_search',
    provider_time_ranges: ['1', '7', '30', '180'],
  }])
})

afterEach(() => {
  for (const mounted of mountedBuilders) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedBuilders.clear()
})

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
  it('普通计价事实区沿用平面表面且禁止反相主题块', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '等待确认',
        accountCapability: sourceAwareAccountCapability,
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: draftPlan,
      }),
    )

    expect(markup).toContain('class="collection-plan__pricing" data-tone="neutral"')
  })

  it('计划卡按头部、事实区、成本区和稳定底栏组织', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '等待确认',
        accountCapability: sourceAwareAccountCapability,
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

  it('预计总价超过余额时显示参考提示但不禁用运行', () => {
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

    expect(markup).toContain('额度参考：TikHub 免费额度与充值余额合计不足')
    expect(markup).toContain('任务会在达到金额上限或可用余额上限时自动停止')
    expect(markup).not.toMatch(/<button[^>]*disabled=""/)
  })

  it('实时计价被限流时显示参考提示但不禁用运行', () => {
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
          pricingBlocker: 'TikHub 请求失败，HTTP 429：请求过于频繁',
        },
      }),
    )

    expect(markup).toContain('额度参考：实时计价请求过于频繁，请稍后重试')
    expect(markup).not.toContain('计划校验未通过')
    expect(markup).not.toMatch(/<button[^>]*disabled=""/)
  })

  it('实时报价超过设定上限时显示参考提示但不禁用运行', () => {
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
          pricingBlocker: 'TikHub 实时报价超过计划预算上限',
        },
      }),
    )

    expect(markup).toContain('额度参考：实时计价超过计划预算上限，请缩小范围或提高预算')
    expect(markup).not.toContain('计划校验未通过')
    expect(markup).not.toMatch(/<button[^>]*disabled=""/)
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

  it('确认前列出补全字段分类和涉及的计价端点', () => {
    const markup = renderToStaticMarkup(
      createElement(CollectionPlanPreview, {
        actionMessage: '等待确认',
        accountCapability: sourceAwareAccountCapability,
        isBusy: false,
        onConfirmPlan: vi.fn(),
        plan: {
          ...draftPlan,
          accountSource: 'user_search',
          selectedFields: ['bio', 'age', 'followers_count', 'last_posted_at'],
          discoveryRequestCount: 1,
          enrichmentRequestCount: 40,
          requestCountEstimate: 41,
          pricingEndpoints: [
            '/api/v1/douyin/search/fetch_user_search',
            '/api/v1/douyin/web/handler_user_profile_v4',
          ],
          pricingReady: true,
        },
      }),
    )

    expect(markup).toContain('补全字段分类：人口属性、账号活跃')
    expect(markup).not.toContain('补全字段分类：账号资料')
    expect(markup).toContain('账号来源')
    expect(markup).toContain('搜索用户')
    expect(markup).toContain('4 个扩展字段')
    expect(markup).toContain('账号发现：1 次请求')
    expect(markup).toContain('字段补全：40 次请求')
    expect(markup).toContain('涉及端点：/api/v1/douyin/search/fetch_user_search')
    expect(markup).toContain('/api/v1/douyin/web/handler_user_profile_v4')
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
    accountSource: 'user_search',
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
      accountSource: undefined,
      dataTypes: [],
      selectedFields: [],
      regionCode: '',
      keyword: '',
      range: '',
      maxRecords: undefined,
      budget: undefined,
    })
  })

  it('空的记录数和成本上限使用当前语言的业务错误', () => {
    const result = collectionFormSchema.safeParse({
      ...baseInput,
      maxRecords: undefined,
      budget: undefined,
    })

    expect(result.success).toBe(false)
    if (result.success) return
    expect(Object.fromEntries(result.error.issues.map((issue) => [
      issue.path.join('.'),
      issue.message,
    ]))).toMatchObject({
      maxRecords: '请输入最大记录数',
      budget: '请输入成本上限',
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
    expect(markup).toMatch(/<button[^>]*id="account-source"[^>]*aria-haspopup="listbox"/)
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

  it('来源区域使用单一账号来源和紧凑字段摘要，不再渲染旧数据类型列表', () => {
    const markup = renderBuilder()

    expect(markup).toContain('账号来源')
    expect(markup).toContain('结果字段')
    expect(markup).toContain('6 个基础字段 + 0 个扩展字段')
    expect(markup).not.toContain('collection-builder__option-list')
    expect(markup).not.toContain('搜索结果中的账号')
    expect(markup).not.toContain('账号作品所属账号')
  })

  it('表单提交单一账号来源、字段集合和兼容数据类型映射', async () => {
    const onGenerateFormPlan = vi.fn(async () => draftPlan)
    const mounted = mountBuilder({
      onGenerateFormPlan,
      onGenerateNaturalPlan: vi.fn(async () => draftPlan),
    })
    const choose = async (selectId: string, optionId: string) => {
      await act(async () => mounted.container.querySelector<HTMLButtonElement>(`#${selectId}`)?.click())
      await act(async () => mounted.container.querySelector<HTMLButtonElement>(`#${optionId}`)?.click())
    }
    const enter = async (selector: string, value: string) => {
      const input = mounted.container.querySelector<HTMLInputElement>(selector)
      await act(async () => {
        const nativeSetter = Object.getOwnPropertyDescriptor(
          HTMLInputElement.prototype,
          'value',
        )?.set
        nativeSetter?.call(input, value)
        input?.dispatchEvent(new Event('input', { bubbles: true }))
      })
    }

    await choose('platform', 'platform-option-TikTok')
    await act(async () => undefined)
    expect(mounted.container.querySelector<HTMLButtonElement>('#account-source')?.disabled).toBe(false)
    expect(mounted.container.querySelector<HTMLInputElement>('input[name="ageRangeEnabled"]')?.disabled)
      .toBe(true)
    expect(mounted.container.querySelector<HTMLInputElement>('input[name="genderFilterEnabled"]')?.disabled)
      .toBe(true)
    expect(mounted.container.textContent).toContain('当前平台没有可验证的公开年龄来源')
    expect(mounted.container.textContent).toContain('当前平台没有可验证的公开性别来源')
    await choose('account-source', 'account-source-option-user_search')
    expect(mounted.container.querySelector('#source-input')).not.toBeNull()
    await enter('#source-input', '新能源汽车')
    await choose('account-source', 'account-source-option-direct_account')
    expect(mounted.container.querySelector<HTMLInputElement>('#source-input')?.value).toBe('')
    await choose('account-source', 'account-source-option-user_search')
    await enter('#source-input', '新能源汽车')
    await choose('range', 'range-option-7')
    await enter('#max-records', '20')
    await enter('#budget', '1')
    await act(async () => {
      mounted.container.querySelector<HTMLFormElement>('form')?.dispatchEvent(
        new Event('submit', { bubbles: true, cancelable: true }),
      )
      await Promise.resolve()
    })

    expect([...mounted.container.querySelectorAll('.form-error')].map((node) => node.textContent)).toEqual([])
    expect(onGenerateFormPlan).toHaveBeenCalledTimes(1)
    expect(onGenerateFormPlan).toHaveBeenCalledWith(expect.objectContaining({
      accountSource: 'user_search',
      dataType: '搜索结果账号',
      dataTypes: ['keyword_search'],
      selectedFields: ['avatar_url'],
    }))
  })

  it('抖音能力声明人口属性可用时启用年龄和性别筛选', async () => {
    backendApiMocks.getAccountCollectionCapabilities.mockResolvedValue({
      catalog_version: 1,
      platform: 'douyin',
      display_name: '抖音',
      account_sources: [],
      field_groups: [{ key: 'demographics', display_name: '人口属性' }],
      fields: [
        {
          key: 'age',
          group: 'demographics',
          display_name: '年龄',
          description: '公开年龄。',
          value_type: 'number',
          availability: 'enrichment',
          default_selected: false,
          required_operation_keys: ['enrich.extended_demographics'],
        },
        {
          key: 'gender',
          group: 'demographics',
          display_name: '性别',
          description: '公开性别。',
          value_type: 'text',
          availability: 'direct',
          default_selected: false,
          required_operation_keys: [],
        },
      ],
    })
    const mounted = mountBuilder({
      onGenerateNaturalPlan: vi.fn(async () => draftPlan),
    })

    await act(async () => mounted.container.querySelector<HTMLButtonElement>('#platform')?.click())
    await act(async () => Array.from(
      mounted.container.querySelectorAll<HTMLButtonElement>('.app-select__option'),
    ).find((option) => option.textContent?.includes('抖音'))?.click())
    await act(async () => undefined)

    expect(mounted.container.querySelector<HTMLInputElement>('input[name="ageRangeEnabled"]')?.disabled)
      .toBe(false)
    expect(mounted.container.querySelector<HTMLInputElement>('input[name="genderFilterEnabled"]')?.disabled)
      .toBe(false)
    expect(mounted.container.textContent).toContain('单一闭区间，不接收未知、异常或推断年龄')
    expect(mounted.container.textContent).toContain('不根据头像、姓名或简介推断，仅使用明确公开性别')

    await act(async () => {
      mounted.container.querySelector<HTMLInputElement>('input[name="ageRangeEnabled"]')?.click()
      mounted.container.querySelector<HTMLInputElement>('input[name="genderFilterEnabled"]')?.click()
    })

    const filterCheckboxes = mounted.container.querySelectorAll<HTMLInputElement>(
      '.collection-builder__filter-grid input[type="checkbox"]',
    )
    expect(filterCheckboxes.length).toBeGreaterThan(2)
    expect(Array.from(filterCheckboxes).every(
      (input) => input.dataset.ui === 'sortlytic-checkbox',
    )).toBe(true)

    expect(mounted.container.textContent).toContain('6 个基础字段 + 2 个扩展字段')
    await act(async () => findButton(mounted.container, '配置字段')?.click())
    expect(mounted.container.textContent).toContain('人口属性2/2')
  })

  it('平台能力仍在加载时禁止生成旧能力计划', async () => {
    let resolveCapability: ((value: Awaited<ReturnType<
      typeof backendApiMocks.getAccountCollectionCapabilities
    >>) => void) | undefined
    backendApiMocks.getAccountCollectionCapabilities.mockImplementation(() => new Promise(
      (resolve) => { resolveCapability = resolve },
    ))
    const mounted = mountBuilder({
      onGenerateNaturalPlan: vi.fn(async () => draftPlan),
    })

    await act(async () => mounted.container.querySelector<HTMLButtonElement>('#platform')?.click())
    await act(async () => Array.from(
      mounted.container.querySelectorAll<HTMLButtonElement>('.app-select__option'),
    ).find((option) => option.textContent?.includes('TikTok'))?.click())

    expect(findButton(mounted.container, '生成计划')?.disabled).toBe(true)
    await act(async () => resolveCapability?.({
      catalog_version: 1,
      platform: 'tiktok',
      display_name: 'TikTok',
      account_sources: [{
        key: 'user_search',
        display_name: '搜索用户',
        description: '按关键词搜索公开账号。',
        input_kind: 'keyword',
        endpoint_key: 'tiktok.user_search',
        pagination_mode: 'cursor',
        max_page_size: 20,
        max_request_count: 100,
      }],
      field_groups: [{ key: 'profile', display_name: '账号资料' }],
      fields: [{
        key: 'avatar_url',
        group: 'profile',
        display_name: '头像',
        description: '账号公开头像地址。',
        value_type: 'text',
        availability: 'direct',
        default_selected: true,
        required_operation_keys: [],
      }],
    }))
    expect(findButton(mounted.container, '生成计划')?.disabled).toBe(false)
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

  it('0.1 至 1.0 美元每 0.1 一档都可设置，并重复验证三轮', () => {
    const markup = renderBuilder()

    expect(markup).toMatch(/<input[^>]*id="budget"[^>]*min="0.1"/)
    for (let round = 1; round <= 3; round += 1) {
      for (let tenths = 1; tenths <= 10; tenths += 1) {
        const budget = tenths / 10
        expect(
          collectionFormSchema.safeParse({ ...baseInput, budget }).success,
          `第 ${round} 轮 $${budget.toFixed(1)} 应通过`,
        ).toBe(true)
      }
    }
  })

  it('自然语言入口不预填具体任务，并在提交前去除首尾空白', () => {
    expect(naturalIntentDefault).toBe('')
    expect(normalizeNaturalIntent('  采集公开账号  ')).toBe('采集公开账号')
    expect(normalizeNaturalIntent('   ')).toBe('')
  })

  it('同一渲染帧内快速重复提交自然语言计划时只生成一次', async () => {
    let resolvePlan!: (plan: RuntimeCollectionPlan) => void
    const pendingPlan = new Promise<RuntimeCollectionPlan>((resolve) => {
      resolvePlan = resolve
    })
    const onGenerateNaturalPlan = vi.fn(() => pendingPlan)
    const mounted = mountBuilder({ onGenerateNaturalPlan })

    act(() => findButton(mounted.container, '自然语言')?.dispatchEvent(
      new MouseEvent('mousedown', { bubbles: true, button: 0 }),
    ))
    const textarea = mounted.container.querySelector<HTMLTextAreaElement>('#intent')
    act(() => {
      if (!textarea) throw new Error('未找到自然语言输入框')
      const nativeSetter = Object.getOwnPropertyDescriptor(
        HTMLTextAreaElement.prototype,
        'value',
      )?.set
      nativeSetter?.call(textarea, '  采集公开账号  ')
      textarea.dispatchEvent(new Event('input', { bubbles: true }))
    })
    const submitButton = findButton(mounted.container, '解析为计划')

    act(() => {
      submitButton?.click()
      submitButton?.click()
    })

    expect(onGenerateNaturalPlan).toHaveBeenCalledTimes(1)
    expect(onGenerateNaturalPlan).toHaveBeenCalledWith('采集公开账号')

    resolvePlan(draftPlan)
    await act(async () => pendingPlan)

    await act(async () => submitButton?.click())
    expect(onGenerateNaturalPlan).toHaveBeenCalledTimes(2)
  })

  it('数据类型至少选择一项，且年龄范围使用闭区间校验', () => {
    const { accountSource: _accountSource, ...withoutAccountSource } = baseInput
    expect(collectionFormSchema.safeParse(withoutAccountSource).success).toBe(false)
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
