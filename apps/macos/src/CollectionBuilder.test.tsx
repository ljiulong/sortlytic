import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import { CollectionPlanPreview } from './CollectionBuilder'
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
