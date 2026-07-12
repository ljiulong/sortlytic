import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  backendErrorMessage,
  type CollectionPlanView,
  type CollectionTaskView,
  type WorkspaceSummary,
} from './backend-api'
import { mapBackendData, planFromBackend, useWorkbenchBackend } from './use-workbench-backend'
import { workspaceSnapshot } from './workbench-data'

const queryMock = vi.hoisted(() => ({
  current: {
    data: undefined as unknown,
    error: null as Error | null,
    isLoading: true,
    isSuccess: false,
  },
}))

vi.mock('@tanstack/react-query', () => ({
  useMutation: () => ({
    isPending: false,
    mutateAsync: vi.fn(),
  }),
  useQuery: () => queryMock.current,
  useQueryClient: () => ({
    invalidateQueries: vi.fn(),
  }),
}))

const workspace: WorkspaceSummary = {
  id: 'workspace-1',
  name: '测试工作区',
  root_path: '/tmp/workspace-1',
  database_path: '/tmp/workspace-1/workspace.sqlite',
  schema_version: 1,
  created_at: '2026-07-12T00:00:00Z',
  updated_at: '2026-07-12T00:00:00Z',
  last_opened_at: '2026-07-12T00:00:00Z',
}

const task: CollectionTaskView = {
  id: 'task-1',
  name: '重复名称也必须保留稳定 ID',
  source_type: 'form',
  status: 'queued',
  platforms_json: ['xiaohongshu'],
  data_types_json: ['comments'],
  created_at: '2026-07-12T00:00:00Z',
  updated_at: '2026-07-12T00:00:00Z',
  cost_estimate_json: { request_count_estimate: 3 },
  actual_cost_json: {},
}

function renderWorkbenchHook() {
  let result: ReturnType<typeof useWorkbenchBackend> | undefined

  function Probe() {
    result = useWorkbenchBackend()
    return null
  }

  renderToStaticMarkup(createElement(Probe))

  if (!result) {
    throw new Error('工作台 Hook 未完成渲染')
  }

  return result
}

beforeEach(() => {
  queryMock.current = {
    data: undefined,
    error: null,
    isLoading: true,
    isSuccess: false,
  }
})

describe('backendErrorMessage', () => {
  it('保留标准错误的可读消息', () => {
    expect(backendErrorMessage(new Error('后端连接失败'))).toBe('后端连接失败')
  })
})

describe('mapBackendData', () => {
  it('不会把浏览器演示数据伪装成真实工作区结果', () => {
    const result = mapBackendData(workspace, [task], [], [], 1_000)

    expect(result.records).toEqual([])
    expect(result.promptRuns).toEqual([])
    expect(JSON.stringify(result)).not.toContain('example.local')
    expect(result.tasks[0]?.id).toBe('task-1')
    expect(result.tasks[0]?.records).toBe(0)
    expect(result.metrics).toContainEqual({
      label: '证据覆盖',
      value: '未计算',
      delta: '暂无真实记录',
      tone: 'info',
    })
  })

  it('把 queued 明确映射为已排队，而不是运行中', () => {
    const result = mapBackendData(workspace, [task], [], [], 1_000)

    expect(result.tasks[0]?.status).toBe('已排队')
    expect(result.tasks[0]?.progress).toBe(0)
  })
})

describe('planFromBackend', () => {
  it('确认视图使用后端多平台计划且不虚构记录数和金额预算', () => {
    const plan: CollectionPlanView = {
      id: 'plan-1',
      task_id: 'task-1',
      source: 'ai_generated',
      schema_version: 1,
      plan_json: {
        platforms: ['tiktok', 'douyin'],
        data_types: ['comments', 'keyword_search'],
        region: {
          value: 'US',
          source: 'natural_language',
          validation_status: 'unverified',
        },
        keywords: ['electric-car'],
        time_range: null,
        steps: [],
        request_limit: 4,
      },
      validation_status: 'needs_review',
      validation_errors_json: ['region 尚未验证', 'time_range 不能为空'],
      cost_estimate_json: { request_count_estimate: 4 },
      confirmed_by_user: false,
      created_at: '2026-07-12T00:00:00Z',
      updated_at: '2026-07-12T00:00:00Z',
    }

    const result = planFromBackend(
      {
        platform: '小红书',
        dataType: '评论采集',
        regionCode: 'CN',
        keyword: '前端启发式占位值',
        range: '由自然语言解析',
        maxRecords: 500,
        budget: 35,
      },
      plan,
    )

    expect(result.platforms).toEqual(['TikTok', '抖音'])
    expect(result.dataTypes).toEqual(['评论采集', '关键词搜索'])
    expect(result.regionCode).toBe('US')
    expect(result.keyword).toBe('electric-car')
    expect(result.range).toBe('未提供时间范围')
    expect(result.maxRecords).toBe(0)
    expect(result.budget).toBe(0)
    expect(result.missing).toEqual(['region 尚未验证', 'time_range 不能为空'])
  })
})

describe('useWorkbenchBackend 数据边界', () => {
  it('加载 Tauri 后端期间只返回空状态，不暴露浏览器演示数据', () => {
    const result = renderWorkbenchHook()

    expect(result.data.runtimeMode).toBe('loading')
    expect(result.data.tasks).toEqual([])
    expect(result.data.records).toEqual([])
    expect(JSON.stringify(result.data)).not.toContain('example.local')
    expect(JSON.stringify(result.data)).not.toContain('8,742')
  })

  it('Tauri 查询失败时只返回错误空状态，不回退虚构成功数据', () => {
    queryMock.current = {
      data: undefined,
      error: new Error('数据库读取失败'),
      isLoading: false,
      isSuccess: false,
    }

    const result = renderWorkbenchHook()

    expect(result.data.runtimeMode).toBe('error')
    expect(result.data.tasks).toEqual([])
    expect(result.data.records).toEqual([])
    expect(result.actionMessage).toBe('数据库读取失败')
    expect(JSON.stringify(result.data)).not.toContain('example.local')
    expect(JSON.stringify(result.data)).not.toContain('100%')
  })

  it('浏览器预览明确标记为演示模式且不声称后端可用', () => {
    queryMock.current = {
      data: {
        ...workspaceSnapshot,
        workspace: {
          ...workspaceSnapshot.workspace,
          health: '浏览器预览',
        },
        modelProviders: [],
        runtimeMode: 'demo',
      },
      error: null,
      isLoading: false,
      isSuccess: true,
    }

    const result = renderWorkbenchHook()

    expect(result.data.runtimeMode).toBe('demo')
    expect(result.actionMessage).toBe('浏览器演示模式：未连接 Tauri 后端，当前内容均为演示数据')
    expect(result.actionMessage).not.toContain('后端可用')
  })
})
