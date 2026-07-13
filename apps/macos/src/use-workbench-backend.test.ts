import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  backendErrorMessage,
  type CollectionPlanView,
  type CollectionTaskView,
  getTikhubConnector,
  saveTikhubConnector,
  type SecretRefView,
  testTikhubConnector,
  type TikhubConnectorView,
  updateSecret,
  type WorkspaceSummary,
} from './backend-api'
import {
  mapBackendData,
  planFromBackend,
  saveAndTestTikhubToken,
  type RuntimeCollectionPlan,
  useWorkbenchBackend,
} from './use-workbench-backend'
import { workspaceSnapshot } from './workbench-data'

type CapturedMutationOptions = {
  onSuccess?: (data: unknown) => unknown
  onError?: (error: unknown) => unknown
}

const invokeMock = vi.hoisted(() => vi.fn())
const invalidateQueriesMock = vi.hoisted(() => vi.fn())
const mutationOptionsMock = vi.hoisted(() => ({ current: [] as CapturedMutationOptions[] }))
const stateSettersMock = vi.hoisted(() => ({ current: [] as ReturnType<typeof vi.fn>[] }))
const queryMock = vi.hoisted(() => ({
  current: {
    data: undefined as unknown,
    error: null as Error | null,
    isLoading: true,
    isSuccess: false,
  },
}))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

vi.mock('react', async () => {
  const actual = await vi.importActual<typeof import('react')>('react')
  return {
    ...actual,
    useState(initialValue: unknown) {
      const [value] = actual.useState(initialValue)
      const setter = vi.fn()
      stateSettersMock.current.push(setter)
      return [value, setter]
    },
  }
})

vi.mock('@tanstack/react-query', () => ({
  useMutation: (options: CapturedMutationOptions) => {
    mutationOptionsMock.current.push(options)
    return {
      isPending: false,
      mutateAsync: vi.fn(),
    }
  },
  useQuery: () => queryMock.current,
  useQueryClient: () => ({
    invalidateQueries: invalidateQueriesMock,
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

const tikhubSecret: SecretRefView = {
  id: 'secret-tikhub-1',
  provider_type: 'tikhub',
  provider_id: 'default',
  masked_hint: 'tikh...[REDACTED]...1234',
}

function connectorFixture(
  overrides: Partial<TikhubConnectorView> = {},
): TikhubConnectorView {
  return {
    id: 'default',
    workspace_id: workspace.id,
    secret_ref_id: tikhubSecret.id,
    base_url: 'https://api.tikhub.io',
    enabled: true,
    config_version: 1,
    last_tested_at: null,
    last_test_status: null,
    created_at: '2026-07-13T00:00:00Z',
    updated_at: '2026-07-13T00:00:00Z',
    ...overrides,
  }
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
  invokeMock.mockReset()
  invalidateQueriesMock.mockReset()
  invalidateQueriesMock.mockResolvedValue(undefined)
  mutationOptionsMock.current = []
  stateSettersMock.current = []
  queryMock.current = {
    data: undefined,
    error: null,
    isLoading: true,
    isSuccess: false,
  }
})

describe('TikHub connector 后端 API', () => {
  it('使用固定 command 和 camelCase 外层参数读写并测试 connector', async () => {
    const input = {
      secret_ref_id: tikhubSecret.id,
      base_url: 'https://api.tikhub.io',
      enabled: true,
    }
    invokeMock.mockResolvedValue(undefined)

    await getTikhubConnector()
    await saveTikhubConnector(input)
    await testTikhubConnector()

    expect(invokeMock).toHaveBeenNthCalledWith(1, 'get_tikhub_connector', {
      rootPath: null,
    })
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'save_tikhub_connector', {
      input,
      rootPath: null,
    })
    expect(invokeMock).toHaveBeenNthCalledWith(3, 'test_tikhub_connector', {
      rootPath: null,
    })
  })

  it('更新密钥时使用既有 secretRefId 且不创建新引用', async () => {
    invokeMock.mockResolvedValue(undefined)

    await updateSecret(tikhubSecret.id, 'replacement-token')

    expect(invokeMock).toHaveBeenCalledWith('update_secret', {
      secretRefId: tikhubSecret.id,
      secret: 'replacement-token',
      rootPath: null,
    })
  })

  it('保存流程复用 connector 已绑定且仍存在的密钥引用', async () => {
    const connector = connectorFixture()
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_tikhub_connector') return connector
      if (command === 'list_secret_refs') return [tikhubSecret]
      if (command === 'update_secret') return tikhubSecret
      if (command === 'save_tikhub_connector') return connector
      if (command === 'test_tikhub_connector') {
        return {
          success: true,
          base_url: connector.base_url,
          daily_usage_json: {},
          message: 'TikHub Token 可用',
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    const result = await saveAndTestTikhubToken({
      token: 'replacement-token',
      baseUrl: connector.base_url,
    })

    expect(result.success).toBe(true)
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'get_tikhub_connector',
      'list_secret_refs',
      'save_tikhub_connector',
      'update_secret',
      'test_tikhub_connector',
    ])
    expect(invokeMock).not.toHaveBeenCalledWith('test_tikhub_connection', expect.anything())
  })

  it('connector 的密钥引用已丢失时创建新引用并重新绑定', async () => {
    const connector = connectorFixture({ secret_ref_id: 'deleted-secret' })
    const replacement = { ...tikhubSecret, id: 'replacement-secret' }
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_tikhub_connector') return connector
      if (command === 'list_secret_refs') return [tikhubSecret]
      if (command === 'save_secret') return replacement
      if (command === 'save_tikhub_connector') return connector
      if (command === 'test_tikhub_connector') {
        return {
          success: true,
          base_url: connector.base_url,
          daily_usage_json: {},
          message: 'TikHub Token 可用',
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    await saveAndTestTikhubToken({ token: 'new-token', baseUrl: connector.base_url })

    expect(invokeMock).toHaveBeenCalledWith('save_tikhub_connector', {
      input: {
        secret_ref_id: replacement.id,
        base_url: connector.base_url,
        enabled: true,
      },
      rootPath: null,
    })
    expect(invokeMock.mock.calls.map(([command]) => command)).not.toContain('update_secret')
  })
})

describe('TikHub mutation 失败回读', () => {
  it('清空旧测试结果并等待连接状态查询失效完成', async () => {
    renderWorkbenchHook()
    const tikhubMutation = mutationOptionsMock.current[4]
    if (!tikhubMutation?.onSuccess || !tikhubMutation.onError) {
      throw new Error('TikHub mutation 应为 Hook 中按顺序注册的第 5 个 mutation')
    }
    const oldResult = {
      success: true,
      base_url: 'https://api.tikhub.io',
      daily_usage_json: {},
      message: '旧成功状态',
    }
    await tikhubMutation.onSuccess(oldResult)
    const tikhubResultSetter = stateSettersMock.current.find((setter) =>
      setter.mock.calls.some(([value]) => value === oldResult),
    )
    const actionMessageSetter = stateSettersMock.current.find((setter) =>
      setter.mock.calls.some(([value]) => value === oldResult.message),
    )
    if (!actionMessageSetter || !tikhubResultSetter) {
      throw new Error('未捕获 TikHub mutation 使用的状态 setter')
    }
    expect(tikhubResultSetter).toHaveBeenCalledWith(oldResult)
    tikhubResultSetter.mockClear()
    actionMessageSetter.mockClear()
    invalidateQueriesMock.mockReset()
    let finishInvalidation: (() => void) | undefined
    const invalidation = new Promise<void>((resolve) => {
      finishInvalidation = resolve
    })
    invalidateQueriesMock.mockReturnValue(invalidation)

    const completion = Promise.resolve(tikhubMutation.onError(new Error('Token 已失效')))
    let completed = false
    void completion.then(() => {
      completed = true
    })
    await Promise.resolve()

    expect(tikhubResultSetter).toHaveBeenCalledWith(undefined)
    expect(actionMessageSetter).toHaveBeenCalledWith('Token 已失效')
    expect(invalidateQueriesMock).toHaveBeenCalledWith({ queryKey: ['workbench-backend'] })
    expect(completed).toBe(false)

    finishInvalidation?.()
    await completion
    expect(completed).toBe(true)
  })
})

describe('计划确认状态转换', () => {
  it('确认并入队后把活动计划标记为已排队，而不是运行中', () => {
    renderWorkbenchHook()
    const confirmMutation = mutationOptionsMock.current[2]
    const activePlanSetter = stateSettersMock.current[0]
    if (!confirmMutation?.onSuccess || !activePlanSetter) {
      throw new Error('未捕获计划确认 mutation 或活动计划状态 setter')
    }

    confirmMutation.onSuccess(undefined)
    const updatePlan = activePlanSetter.mock.calls.at(-1)?.[0]
    if (typeof updatePlan !== 'function') {
      throw new Error('计划确认成功后应通过函数更新活动计划')
    }
    const currentPlan: RuntimeCollectionPlan = {
      platform: 'TikTok',
      dataType: '评论采集',
      regionCode: 'US',
      keyword: 'electric vehicle',
      range: '2026-07-01/2026-07-07',
      maxRecords: 100,
      budget: 10,
      status: '等待确认',
      missing: [],
      taskId: 'task-1',
      planId: 'plan-1',
      validationStatus: 'valid',
    }

    expect(updatePlan(currentPlan).status).toBe('已排队')
  })
})

describe('backendErrorMessage', () => {
  it('保留标准错误的可读消息', () => {
    expect(backendErrorMessage(new Error('后端连接失败'))).toBe('后端连接失败')
  })
})

describe('mapBackendData', () => {
  it('不会把浏览器演示数据伪装成真实工作区结果', () => {
    const result = mapBackendData(workspace, [task], [], null, [], 1_000)

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
    const result = mapBackendData(workspace, [task], [], null, [], 1_000)

    expect(result.tasks[0]?.status).toBe('已排队')
    expect(result.tasks[0]?.progress).toBe(0)
  })

  it('孤立 TikHub 密钥不会被当成已配置 connector', () => {
    const result = mapBackendData(workspace, [], [tikhubSecret], null, [], 1_000)
    const connection = result.connections[0]

    expect(connection?.status).toBe('未配置')
    expect(connection?.meta).not.toContain(tikhubSecret.masked_hint)
  })

  it.each([
    ['待测试', connectorFixture()],
    ['已验证', connectorFixture({ last_test_status: 'success' })],
    ['测试失败', connectorFixture({ last_test_status: 'failed' })],
    ['已禁用', connectorFixture({ enabled: false })],
    ['需重新绑定', connectorFixture({ secret_ref_id: null })],
  ])('把 connector 状态映射为“%s”', (expected, connector) => {
    const result = mapBackendData(workspace, [], [tikhubSecret], connector, [], 1_000)
    const connection = result.connections[0]

    expect(connection?.status).toBe(expected)
    expect(connection?.meta).toContain(connector.base_url)
    if (connector.secret_ref_id) {
      expect(connection?.meta).toContain(tikhubSecret.masked_hint)
    }
  })

  it('连接卡片不会显示非官方 Base URL', () => {
    const connector = connectorFixture({ base_url: 'https://untrusted.example/api' })
    const result = mapBackendData(workspace, [], [tikhubSecret], connector, [], 1_000)

    expect(result.connections[0]?.meta).not.toContain('untrusted.example')
    expect(result.connections[0]?.meta).toContain(tikhubSecret.masked_hint)
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
