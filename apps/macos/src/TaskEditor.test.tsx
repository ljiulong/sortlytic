// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  AccountCollectionCapabilityView,
  CollectionPlanView,
  CollectionTaskView,
  NaturalParseAttemptView,
} from './backend-api'

const apiMocks = vi.hoisted(() => ({
  generateAccountCollectionPlan: vi.fn(),
  getAccountCollectionCapabilities: vi.fn(),
  getAiRun: vi.fn(),
  getLatestCollectionPlan: vi.fn(),
  getTask: vi.fn(),
  listAiRuns: vi.fn(),
  reviseCollectionTask: vi.fn(),
}))

vi.mock('./backend-api', async (importOriginal) => ({
  ...await importOriginal<typeof import('./backend-api')>(),
  ...apiMocks,
}))

import TaskEditor from './TaskEditor'

type MountedEditor = { container: HTMLDivElement; root: Root }
const mountedEditors = new Set<MountedEditor>()

beforeEach(() => {
  Object.values(apiMocks).forEach((mock) => mock.mockReset())
  apiMocks.getTask.mockResolvedValue(task())
  apiMocks.getLatestCollectionPlan.mockResolvedValue(plan())
  apiMocks.getAiRun.mockResolvedValue({ output_json: parsedIntent() })
  apiMocks.getAccountCollectionCapabilities.mockResolvedValue(capability())
  apiMocks.listAiRuns.mockResolvedValue([])
  apiMocks.generateAccountCollectionPlan.mockResolvedValue({
    source: 'form_generated',
    schema_version: 4,
    plan_json: plan().plan_json,
    validation_status: 'valid',
    validation_errors_json: [],
    cost_estimate_json: { request_count_estimate: 1 },
  })
  apiMocks.reviseCollectionTask.mockResolvedValue({
    task: task({ status: 'waiting_confirmation' }),
    collection_plan: plan({ id: 'plan-2' }),
    copied_from_task_id: null,
  })
})

afterEach(() => {
  for (const mounted of mountedEditors) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedEditors.clear()
})

describe('完整任务编辑器', () => {
  it('加载计划、自然语言意图和审计上下文中的全部可编辑字段', async () => {
    const mounted = mountEditor({ naturalParseAttempt: attempt({ parse_status: 'valid' }) })
    await flushEditor()

    expect(mounted.container.textContent).toContain('编辑任务')
    expect(inputValue(mounted.container, 'task-editor-name')).toBe('宠物园区')
    expect(inputValue(mounted.container, 'task-editor-source-input')).toBe('pet supplies')
    expect(inputValue(mounted.container, 'task-editor-query-locale')).toBe('en-GB')
    expect(mounted.container.textContent).toContain('collection_intent_v1')
    expect(mounted.container.textContent).toContain('collection_plan_v4')
    expect(mounted.container.textContent).toContain('prompt-v3')
  })

  it('失败且没有计划的自然语言任务保留原文并可直接重新解析', async () => {
    const onRetryNaturalTask = vi.fn(async () => undefined)
    apiMocks.getLatestCollectionPlan.mockRejectedValue(new Error('任务还没有采集计划'))
    const mounted = mountEditor({
      naturalParseAttempt: attempt({
        parse_status: 'failed',
        error_code: 'MODEL_AUTH_ERROR',
        error_message: 'AI 配置鉴权失败',
      }),
      onRetryNaturalTask,
    })
    await flushEditor()

    expect(textareaValue(mounted.container, 'task-editor-natural-input'))
      .toBe('用中文查找英国 TikTok 宠物用品账号')
    expect(mounted.container.textContent).toContain('AI 配置鉴权失败')
    const retryButton = buttonByText(mounted.container, '重新解析')
    await act(async () => retryButton.click())

    expect(onRetryNaturalTask).toHaveBeenCalledWith(
      'task-1',
      '用中文查找英国 TikTok 宠物用品账号',
    )
  })

  it('保存时先由后端重新生成安全计划，再提交 user_edited 新版本', async () => {
    const onSaved = vi.fn()
    const mounted = mountEditor({
      naturalParseAttempt: attempt({ parse_status: 'valid' }),
      onSaved,
    })
    await flushEditor()

    await act(async () => buttonByText(mounted.container, '保存新计划版本').click())
    await flushEditor()

    expect(apiMocks.generateAccountCollectionPlan).toHaveBeenCalledWith(expect.objectContaining({
      platform: 'tiktok',
      account_source: 'user_search',
      params: expect.objectContaining({ keyword: 'pet supplies', region: 'GB' }),
      record_limit: 10,
      budget_limit_micros: 100_000,
    }))
    expect(apiMocks.reviseCollectionTask).toHaveBeenCalledWith(expect.objectContaining({
      task_id: 'task-1',
      source: 'user_edited',
      plan_json: expect.objectContaining({ query_locale: 'en-GB' }),
    }))
    expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({
      collection_plan: expect.objectContaining({ id: 'plan-2' }),
    }))
  })

  it('不可靠的旧地区和时间条件贴近控件显示原因并可移除', async () => {
    apiMocks.getAccountCollectionCapabilities.mockResolvedValue(capability({
      region_filter: 'unsupported',
      time_range_filter: 'unsupported',
      time_ranges: [],
    }))
    const mounted = mountEditor({ naturalParseAttempt: attempt({ parse_status: 'valid' }) })
    await flushEditor()

    expect(mounted.container.textContent).toContain('当前平台或来源无法可靠筛选地区')
    expect(mounted.container.textContent).toContain('当前平台或来源无法可靠筛选时间')
    expect(buttonByText(mounted.container, '移除地区条件')).toBeTruthy()
    expect(buttonByText(mounted.container, '移除时间条件')).toBeTruthy()
  })
})

function mountEditor({
  naturalParseAttempt,
  onRetryNaturalTask = vi.fn(async () => undefined),
  onSaved = vi.fn(),
}: {
  naturalParseAttempt?: NaturalParseAttemptView
  onRetryNaturalTask?: (taskId: string, intentText: string) => Promise<unknown>
  onSaved?: (result: unknown) => void
}) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  document.body.append(container)
  mountedEditors.add(mounted)
  act(() => root.render(createElement(TaskEditor, {
    isBusy: false,
    naturalParseAttempt,
    onCancel: vi.fn(),
    onRetryNaturalTask,
    onSaved,
    taskId: 'task-1',
  })))
  return mounted
}

async function flushEditor() {
  await act(async () => {
    await Promise.resolve()
    await new Promise((resolve) => setTimeout(resolve, 0))
  })
}

function inputValue(container: HTMLElement, id: string) {
  return (container.querySelector(`#${id}`) as HTMLInputElement | null)?.value
}

function textareaValue(container: HTMLElement, id: string) {
  return (container.querySelector(`#${id}`) as HTMLTextAreaElement | null)?.value
}

function buttonByText(container: HTMLElement, label: string) {
  const button = [...container.querySelectorAll('button')]
    .find((candidate) => candidate.textContent?.includes(label))
  if (!button) throw new Error(`找不到按钮：${label}`)
  return button
}

function task(overrides: Partial<CollectionTaskView> = {}): CollectionTaskView {
  return {
    id: 'task-1',
    name: '宠物园区',
    source_type: 'natural_language',
    status: 'failed',
    platforms_json: ['tiktok'],
    data_types_json: ['account'],
    created_at: '2026-07-20T00:00:00Z',
    updated_at: '2026-07-20T00:00:00Z',
    cost_estimate_json: {},
    actual_cost_json: {},
    ...overrides,
  }
}

function plan(overrides: Partial<CollectionPlanView> = {}): CollectionPlanView {
  return {
    id: 'plan-1',
    task_id: 'task-1',
    source: 'ai_generated',
    schema_version: 4,
    plan_json: {
      schema_version: 4,
      platforms: ['tiktok'],
      account_source: 'user_search',
      selected_fields: ['country_region'],
      region: 'GB',
      time_range: '30',
      record_limit: 10,
      budget_limit: { amount_micros: 100_000 },
      steps: [{ params: { keyword: 'pet supplies' } }],
    },
    validation_status: 'valid',
    validation_errors_json: [],
    cost_estimate_json: { request_count_estimate: 1 },
    confirmed_by_user: false,
    created_at: '2026-07-20T00:00:00Z',
    updated_at: '2026-07-20T00:00:00Z',
    ...overrides,
  }
}

function parsedIntent() {
  return {
    schema_version: 1,
    platform: 'tiktok',
    account_source: 'user_search',
    source_input: 'pet supplies',
    query_locale: 'en-GB',
    region_code: 'GB',
    selected_fields: ['country_region'],
    time_range_days: 30,
    age_range: null,
    gender_filter: null,
    record_limit: 10,
    budget_limit_micros: 100_000,
    missing_fields: [],
    confidence: 0.96,
  }
}

function attempt(overrides: Partial<NaturalParseAttemptView> = {}): NaturalParseAttemptView {
  return {
    id: 'attempt-1',
    task_id: 'task-1',
    intent_text: '用中文查找英国 TikTok 宠物用品账号',
    parse_status: 'valid',
    ai_run_id: 'ai-run-1',
    model_id: 'deepseek-v4-flash',
    prompt_version_id: 'prompt-v3',
    error_safe_details_json: {},
    created_at: '2026-07-20T00:00:00Z',
    updated_at: '2026-07-20T00:00:00Z',
    ...overrides,
  }
}

function capability(sourceOverrides: Record<string, unknown> = {}): AccountCollectionCapabilityView {
  return {
    catalog_version: 1,
    platform: 'tiktok',
    display_name: 'TikTok',
    account_sources: [{
      key: 'user_search',
      display_name: '搜索用户',
      description: '按关键词搜索账号',
      input_kind: 'keyword',
      endpoint_key: 'tiktok.user_search',
      pagination_mode: 'cursor',
      region_filter: 'local',
      time_range_filter: 'local',
      time_ranges: ['1', '7', '30', '180'],
      max_page_size: 20,
      max_request_count: 10,
      ...sourceOverrides,
    }],
    field_groups: [{ key: 'profile', display_name: '账号资料' }],
    fields: [{
      key: 'country_region',
      group: 'profile',
      display_name: '国家地区',
      description: '接口明确返回的地区',
      value_type: 'text',
      availability: 'enrichment',
      default_selected: true,
      required_operation_keys: ['enrich.account_country'],
    }],
  }
}
