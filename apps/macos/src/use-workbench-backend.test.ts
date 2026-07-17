import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  backendErrorMessage,
  cancelTask,
  checkForAppUpdate,
  type CollectionPlanView,
  type CollectionTaskView,
  deleteTask,
  getLatestCollectionPlan,
  installAppUpdate,
  quoteTikhubConnectorPrice,
  updateCollectionTask,
  type WorkspaceSummary,
} from './backend-api'
import {
  browserFallbackData,
  buildFormPlanRequest,
  confirmPersistedTask,
  exportTaskArtifact,
  loadBackendWorkbench,
  mapBackendData,
  planFromBackend,
  type RuntimeCollectionPlan,
  useWorkbenchBackend,
} from './use-workbench-backend'
import { preflightCollectionPlanPricing } from './collection-pricing'
import { buildPlanParams } from './collection-plan-client'
import type { ApiProfileRegistryView } from './api-profiles'

type CapturedMutationOptions = {
  onSuccess?: (data: unknown) => unknown
  onError?: (error: unknown) => unknown
}

type CapturedQueryOptions = {
  refetchInterval?: (query: { state: { data?: unknown } }) => number | false
  refetchIntervalInBackground?: boolean
}

const invokeMock = vi.hoisted(() => vi.fn())
const updaterCheckMock = vi.hoisted(() => vi.fn())
const updaterInstallMock = vi.hoisted(() => vi.fn())
const relaunchMock = vi.hoisted(() => vi.fn())
const invalidateQueriesMock = vi.hoisted(() => vi.fn())
const mutationOptionsMock = vi.hoisted(() => ({ current: [] as CapturedMutationOptions[] }))
const queryOptionsMock = vi.hoisted(() => ({ current: null as CapturedQueryOptions | null }))
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

vi.mock('@tauri-apps/plugin-updater', () => ({ check: updaterCheckMock }))

vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: relaunchMock }))

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
  useQuery: (options: CapturedQueryOptions) => {
    queryOptionsMock.current = options
    return queryMock.current
  },
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

function tikhubRegistryFixture({
  activeProfileId = 'tikhub-profile-1',
  profileId = 'tikhub-profile-1',
  includeProfile = true,
  status = 'success',
  balance = 0.2,
  freeCredit = 0.1,
  availableCredit = 0.3,
}: {
  activeProfileId?: string | null
  profileId?: string
  includeProfile?: boolean
  status?: 'needs_rebind' | 'untested' | 'success' | 'failed'
  balance?: number | null
  freeCredit?: number | null
  availableCredit?: number | null
} = {}): ApiProfileRegistryView {
  return {
    activeProfileIds: { tikhub: activeProfileId, ai: null },
    tikhubProfiles: includeProfile
      ? [{
          kind: 'tikhub',
          id: profileId,
          name: '主账号',
          baseUrl: 'https://api.tikhub.io',
          revision: 1,
          status,
          maskedKey: 'tikh...[REDACTED]...1234',
          hasCredential: true,
          isActive: activeProfileId === profileId,
          lastTestedAt: '2026-07-17T00:00:00Z',
          testSummary: {
            maskedAccount: 'st***@example.com',
            balance,
            freeCredit,
            availableCredit,
            todayUsage: 0.01,
          },
          createdAt: '2026-07-17T00:00:00Z',
          updatedAt: '2026-07-17T00:00:00Z',
        }]
      : [],
    aiProfiles: [],
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
  vi.unstubAllGlobals()
  invokeMock.mockReset()
  invalidateQueriesMock.mockReset()
  invalidateQueriesMock.mockResolvedValue(undefined)
  mutationOptionsMock.current = []
  queryOptionsMock.current = null
  stateSettersMock.current = []
  queryMock.current = {
    data: undefined,
    error: null,
    isLoading: true,
    isSuccess: false,
  }
  updaterCheckMock.mockReset()
  updaterInstallMock.mockReset()
  relaunchMock.mockReset()
})

describe('任务页动作', () => {
  it('使用稳定的 Tauri 命令读取、更新、取消并删除指定任务', async () => {
    invokeMock.mockResolvedValue({})

    await getLatestCollectionPlan('task-1')
    await updateCollectionTask('task-1', { name: '更新后的任务名' })
    await cancelTask('task-1')
    await deleteTask('task-1')

    expect(invokeMock).toHaveBeenNthCalledWith(1, 'get_latest_collection_plan', {
      taskId: 'task-1',
      rootPath: null,
    })
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'update_collection_task', {
      taskId: 'task-1',
      input: { name: '更新后的任务名' },
      rootPath: null,
    })
    expect(invokeMock).toHaveBeenNthCalledWith(3, 'cancel_task', {
      taskId: 'task-1',
      rootPath: null,
    })
    expect(invokeMock).toHaveBeenNthCalledWith(4, 'delete_task', {
      taskId: 'task-1',
      rootPath: null,
    })
  })

  it('删除成功后更新状态消息并刷新工作台真实数据', async () => {
    renderWorkbenchHook()
    const deleteMutation = mutationOptionsMock.current[6]
    const actionMessageSetter = stateSettersMock.current[1]

    expect(deleteMutation).toBeDefined()
    await deleteMutation?.onSuccess?.(undefined)

    expect(actionMessageSetter).toHaveBeenCalledWith('任务已删除')
    expect(invalidateQueriesMock).toHaveBeenCalledWith({ queryKey: ['workbench-backend'] })
  })

  it('从持久化计划完成计价、确认和入队，不依赖页面内存中的 activePlan', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    const registry = tikhubRegistryFixture({ balance: 1, freeCredit: 1, availableCredit: 2 })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_latest_collection_plan') {
        return {
          id: 'plan-1',
          task_id: 'task-1',
          source: 'form_generated',
          schema_version: 3,
          plan_json: {
            platforms: ['tiktok'],
            data_types: ['comments'],
            keywords: ['electric vehicle'],
            time_range: '近 30 天',
            record_limit: 100,
            budget_limit: { amount_micros: 1_000_000 },
            steps: [{ endpoint_key: 'tiktok.comments' }],
          },
          validation_status: 'valid',
          validation_errors_json: [],
          cost_estimate_json: { request_count_estimate: 3 },
          confirmed_by_user: false,
          created_at: '2026-07-17T00:00:00Z',
          updated_at: '2026-07-17T00:00:00Z',
        }
      }
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        return { success: true, message: 'TikHub API 配置测试成功', registry }
      }
      if (command === 'quote_tikhub_connector_price') {
        return { total_price: 0.01 }
      }
      if (command === 'confirm_collection_plan') return task
      if (command === 'enqueue_task') return { id: 'run-1', task_id: 'task-1', status: 'queued' }
      throw new Error(`意外命令：${command}`)
    })

    await expect(confirmPersistedTask('task-1')).resolves.toMatchObject({
      task_id: 'task-1',
      status: 'queued',
    })
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'get_latest_collection_plan',
      'get_api_profile_registry',
      'test_api_profile',
      'quote_tikhub_connector_price',
      'confirm_collection_plan',
      'enqueue_task',
    ])
  })

  it('单任务导出只生成用户选择的文件格式', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === 'build_report_model') return { id: 'report-1' }
      if (command === 'create_export_job') {
        return {
          id: 'export-1',
          report_id: 'report-1',
          export_type: args?.exportType,
          status: 'success',
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    await expect(exportTaskArtifact({ taskId: 'task-1', format: 'pdf' })).resolves.toMatchObject({
      export_type: 'pdf',
    })
    expect(invokeMock).toHaveBeenCalledWith('create_export_job', {
      reportId: 'report-1',
      exportType: 'pdf',
      targetPath: null,
      rootPath: null,
    })
    expect(invokeMock).not.toHaveBeenCalledWith(
      'create_export_job',
      expect.objectContaining({ exportType: 'xlsx' }),
    )
  })
})

describe('应用更新 API', () => {
  it('检查到新版本后可下载安装并重启应用', async () => {
    const update = {
      version: '0.1.4',
      date: '2026-07-15T08:00:00Z',
      body: '修复稳定性问题',
      downloadAndInstall: updaterInstallMock,
    }
    updaterCheckMock.mockResolvedValue(update)
    updaterInstallMock.mockResolvedValue(undefined)
    relaunchMock.mockResolvedValue(undefined)

    await expect(checkForAppUpdate()).resolves.toEqual({
      version: update.version,
      date: update.date,
      body: update.body,
    })
    await installAppUpdate()

    expect(updaterInstallMock).toHaveBeenCalledOnce()
    expect(relaunchMock).toHaveBeenCalledOnce()
  })

  it('没有新版本时清空待安装版本并阻止误安装', async () => {
    updaterCheckMock.mockResolvedValue(null)

    await expect(checkForAppUpdate()).resolves.toBeNull()
    await expect(installAppUpdate()).rejects.toThrow('请先检查更新')
  })
})

describe('TikHub 实时价格', () => {
  it('使用固定 command 读取实时价格', async () => {
    invokeMock.mockResolvedValue(undefined)

    await quoteTikhubConnectorPrice('/api/v1/tiktok/app/v3/fetch_video_comments', 1)

    expect(invokeMock).toHaveBeenCalledWith('quote_tikhub_connector_price', {
      endpoint: '/api/v1/tiktok/app/v3/fetch_video_comments',
      requestPerDay: 1,
      rootPath: null,
    })
  })
})

describe('计划实时计价预检', () => {
  const plan = {
    platform: 'TikTok',
    dataType: '评论采集',
    regionCode: 'US',
    keyword: 'electric vehicle',
    range: '近 30 天',
    maxRecords: 100,
    budget: 1,
    budgetMicros: 1_000_000,
    requestCountEstimate: 3,
    pricingEndpoints: [
      '/api/v1/tiktok/app/v3/fetch_video_comments',
      '/api/v1/tiktok/app/v3/handler_user_profile',
    ],
    status: '等待确认',
    missing: [],
    taskId: 'task-1',
    planId: 'plan-1',
    validationStatus: 'valid',
  } satisfies RuntimeCollectionPlan

  it('同时读取双额度并使用最高单次报价核对计划请求上限', async () => {
    const registryBeforeTest = tikhubRegistryFixture({
      balance: 9,
      freeCredit: 1,
      availableCredit: 10,
    })
    const testedRegistry = tikhubRegistryFixture()
    invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === 'get_api_profile_registry') return registryBeforeTest
      if (command === 'test_api_profile') {
        return {
          success: true,
          message: 'TikHub API 配置测试成功',
          registry: testedRegistry,
        }
      }
      if (command === 'quote_tikhub_connector_price') {
        const endpoint = String(args?.endpoint)
        return {
          endpoint,
          request_per_day: 1,
          base_unit_price: endpoint.includes('comments') ? 0.02 : 0.01,
          total_price: endpoint.includes('comments') ? 0.02 : 0.01,
          currency: 'USD',
          quote_json: {},
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    await expect(preflightCollectionPlanPricing(plan)).resolves.toMatchObject({
      balance: 0.2,
      freeCredit: 0.1,
      availableCredit: 0.3,
      quotedTotalMicros: 60_000,
    })
    expect(invokeMock).toHaveBeenCalledWith('test_api_profile', {
      kind: 'tikhub',
      profileId: 'tikhub-profile-1',
      rootPath: null,
    })
  })

  it('免费额度与充值余额合计不足时阻止确认', async () => {
    const registry = tikhubRegistryFixture({
      balance: 0.01,
      freeCredit: 0.01,
      availableCredit: 0.02,
    })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        return { success: true, message: 'TikHub API 配置测试成功', registry }
      }
      if (command === 'quote_tikhub_connector_price') {
        return {
          endpoint: '/api/v1/tiktok/app/v3/fetch_video_comments',
          request_per_day: 1,
          base_unit_price: 0.02,
          total_price: 0.02,
          currency: 'USD',
          quote_json: {},
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    await expect(preflightCollectionPlanPricing(plan)).rejects.toThrow(
      'TikHub 免费额度与充值余额合计不足',
    )
  })

  it('没有当前 TikHub 配置时失败关闭且不测试、不报价', async () => {
    invokeMock.mockResolvedValue(tikhubRegistryFixture({ activeProfileId: null }))

    await expect(preflightCollectionPlanPricing(plan)).rejects.toThrow(
      '当前未选择 TikHub API 配置，无法确认运行',
    )
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'get_api_profile_registry',
    ])
  })

  it('当前 TikHub 配置未验证时失败关闭', async () => {
    invokeMock.mockResolvedValue(tikhubRegistryFixture({ status: 'needs_rebind' }))

    await expect(preflightCollectionPlanPricing(plan)).rejects.toThrow(
      '当前 TikHub API 配置未通过验证，无法确认运行',
    )
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'get_api_profile_registry',
    ])
  })

  it('注册表读取失败时使用固定安全错误且不泄露完整密钥', async () => {
    const secret = 'tikhub-secret-that-must-not-leak'
    invokeMock.mockRejectedValue(new Error(`读取失败：token=${secret}`))

    const rejection = expect(preflightCollectionPlanPricing(plan)).rejects
    await rejection.toThrow('TikHub API 配置读取失败，无法确认运行')
    await rejection.not.toThrow(secret)
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'get_api_profile_registry',
    ])
  })

  it('当前配置的真实账号测试失败时不继续报价', async () => {
    const registry = tikhubRegistryFixture()
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        return {
          success: false,
          message: 'TikHub API 配置测试失败',
          registry: tikhubRegistryFixture({ activeProfileId: null, status: 'failed' }),
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    await expect(preflightCollectionPlanPricing(plan)).rejects.toThrow(
      '当前 TikHub API 配置测试失败，无法确认运行',
    )
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'get_api_profile_registry',
      'test_api_profile',
    ])
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
    const result = mapBackendData(
      workspace,
      [task],
      tikhubRegistryFixture({ activeProfileId: null, includeProfile: false }),
      1_000,
    )

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
    const result = mapBackendData(
      workspace,
      [task],
      tikhubRegistryFixture({ activeProfileId: null, includeProfile: false }),
      1_000,
    )

    expect(result.tasks[0]?.status).toBe('已排队')
    expect(result.tasks[0]?.progress).toBe(0)
  })

  it('把 partial_success 映射为已结束的部分成功', () => {
    const result = mapBackendData(
      workspace,
      [{ ...task, status: 'partial_success' }],
      tikhubRegistryFixture({ activeProfileId: null, includeProfile: false }),
      1_000,
    )

    expect(result.tasks[0]?.status).toBe('部分成功')
    expect(result.tasks[0]?.progress).toBe(100)
  })

  it('没有 TikHub 配置时明确显示未配置', () => {
    const result = mapBackendData(
      workspace,
      [],
      tikhubRegistryFixture({ activeProfileId: null, includeProfile: false }),
      1_000,
    )
    const connection = result.connections[0]

    expect(connection?.status).toBe('未配置')
  })

  it.each([
    ['待测试', 'untested'],
    ['已验证', 'success'],
    ['测试失败', 'failed'],
    ['需重新绑定', 'needs_rebind'],
  ] as const)('把 TikHub 配置状态映射为“%s”', (expected, status) => {
    const registry = tikhubRegistryFixture({ status })
    const result = mapBackendData(workspace, [], registry, 1_000)
    const connection = result.connections[0]

    expect(connection?.status).toBe(expected)
    expect(connection?.meta).toContain(registry.tikhubProfiles[0]?.baseUrl)
    expect(connection?.meta).toContain(registry.tikhubProfiles[0]?.maskedKey)
  })

  it('API 配置文件不可读时显示错误但保留工作区数据', () => {
    const result = mapBackendData(workspace, [task], null, 1_000)

    expect(result.runtimeMode).toBe('backend')
    expect(result.tasks).toHaveLength(1)
    expect(result.connections[0]?.status).toBe('配置不可用')
  })
})

describe('planFromBackend', () => {
  it('恢复运行前实时计价所需的端点、请求上限和微美元预算', () => {
    const plan: CollectionPlanView = {
      id: 'plan-price',
      task_id: 'task-1',
      source: 'form_generated',
      schema_version: 3,
      plan_json: {
        platforms: ['tiktok'],
        data_types: ['comments'],
        region: 'US',
        time_range: '30',
        record_limit: 100,
        budget_limit: { currency: 'USD', amount_micros: 2_000_000 },
        steps: [{ endpoint_key: 'tiktok.comments', params: { keyword: 'car' } }],
      },
      validation_status: 'valid',
      validation_errors_json: [],
      cost_estimate_json: { request_count_estimate: 3 },
      confirmed_by_user: false,
      created_at: '2026-07-12T00:00:00Z',
      updated_at: '2026-07-12T00:00:00Z',
    }

    const result = planFromBackend(
      {
        platform: 'TikTok',
        dataType: '评论采集',
        regionCode: 'US',
        keyword: 'car',
        range: '近 30 天',
        maxRecords: 100,
        budget: 2,
      },
      plan,
    )

    expect(result.pricingEndpoints).toEqual(['/api/v1/tiktok/app/v3/fetch_video_comments'])
    expect(result.requestCountEstimate).toBe(3)
    expect(result.budgetMicros).toBe(2_000_000)
    expect(result.pricingReady).toBe(false)
  })

  it('确认视图使用后端多平台计划且不虚构记录数和金额预算', () => {
    const plan: CollectionPlanView = {
      id: 'plan-1',
      task_id: 'task-1',
      source: 'ai_generated',
      schema_version: 1,
      plan_json: {
        platforms: ['tiktok', 'douyin'],
        data_types: ['comments', 'keyword_search', 'account_posts'],
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
    expect(result.dataTypes).toEqual(['评论用户', '搜索结果账号', '账号作品所属账号'])
    expect(result.regionCode).toBe('US')
    expect(result.keyword).toBe('electric-car')
    expect(result.range).toBe('未提供时间范围')
    expect(result.maxRecords).toBe(0)
    expect(result.budget).toBe(0)
    expect(result.missing).toEqual(['region 尚未验证', 'time_range 不能为空'])
  })
})

describe('buildPlanParams', () => {
  const values = {
    regionCode: 'cn',
    keyword: '新能源汽车',
    range: '7',
    maxRecords: 120,
  }

  it('小红书详情不会携带后端明确不支持的地区参数', () => {
    expect(buildPlanParams(values, 'xiaohongshu', 'item_detail')).toEqual({
      item_id: '新能源汽车',
    })
  })

  it('把地区交给支持供应商或本地筛选的端点', () => {
    expect(buildPlanParams(values, 'tiktok', 'keyword_search')).toEqual({
      keyword: '新能源汽车',
      region: 'CN',
      time_range: '7',
      page_size: 50,
    })
    expect(buildPlanParams(values, 'xiaohongshu', 'keyword_search')).toEqual({
      keyword: '新能源汽车',
      region: 'CN',
      time_range: '7',
    })
    expect(buildPlanParams(values, 'douyin', 'comments')).toEqual({
      item_id: '新能源汽车',
      region: 'CN',
      page_size: 50,
    })
  })
})

describe('v3 form plan request', () => {
  it('传递多目标、年龄闭区间与明确性别，同时保留旧单值字段', () => {
    expect(
      buildFormPlanRequest({
        platform: '小红书',
        dataType: '关键词搜索',
        dataTypes: ['item_detail', 'comments'],
        regionCode: 'CN',
        keyword: '新能源汽车',
        range: '近 30 天',
        maxRecords: 1200,
        budget: 35,
        ageRangeEnabled: true,
        ageMin: 18,
        ageMax: 35,
        genderFilterEnabled: true,
        genders: ['female', 'other'],
      }),
    ).toMatchObject({
      platform: 'xiaohongshu',
      data_type: 'item_detail',
      data_types: ['item_detail', 'comments'],
      age_range: { min: 18, max: 35 },
      record_limit: 1200,
      budget_limit_micros: 35_000_000,
      params: {
        keyword: '新能源汽车',
        region: 'CN',
        time_range: '近 30 天',
        genders: ['female', 'other'],
      },
    })
  })
})

describe('useWorkbenchBackend 数据边界', () => {
  it('仅在存在排队或运行中的任务时前台轮询真实状态', () => {
    renderWorkbenchHook()
    const options = queryOptionsMock.current
    const refetchInterval = options?.refetchInterval

    expect(refetchInterval).toBeTypeOf('function')
    expect(refetchInterval?.({
      state: { data: mapBackendData(workspace, [task], tikhubRegistryFixture(), 1_000) },
    })).toBe(2_000)
    expect(refetchInterval?.({
      state: {
        data: mapBackendData(
          workspace,
          [{ ...task, status: 'running' }],
          tikhubRegistryFixture(),
          1_000,
        ),
      },
    })).toBe(2_000)
    expect(refetchInterval?.({
      state: {
        data: mapBackendData(
          workspace,
          [{ ...task, status: 'success' }],
          tikhubRegistryFixture(),
          1_000,
        ),
      },
    })).toBe(false)
    expect(options?.refetchIntervalInBackground).toBe(false)
  })

  it('即使 API 配置 JSON 损坏也保留历史任务浏览', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation((command: string) => {
      if (command === 'ensure_default_workspace') return Promise.resolve(workspace)
      if (command === 'get_backend_status') {
        return Promise.resolve({
          service: 'sortlytic',
          backend_version: '0.2.3',
          has_active_workspace: true,
          uptime_ms: 1_000,
        })
      }
      if (command === 'list_tasks') return Promise.resolve([task])
      if (command === 'get_api_profile_registry') {
        return Promise.reject(new Error('api-config.json 无法解析'))
      }
      return Promise.reject(new Error(`不应调用旧配置命令: ${command}`))
    })

    const result = await loadBackendWorkbench()

    expect(result.runtimeMode).toBe('backend')
    expect(result.tasks).toHaveLength(1)
    expect(result.tasks[0]?.id).toBe(task.id)
    expect(result.connections[0]).toMatchObject({ status: '配置不可用' })
    expect(invokeMock).not.toHaveBeenCalledWith('list_secret_refs', expect.anything())
    expect(invokeMock).not.toHaveBeenCalledWith('get_tikhub_connector', expect.anything())
    expect(invokeMock).not.toHaveBeenCalledWith('list_model_providers', expect.anything())
  })

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

  it('非 Tauri 环境只提供真实空状态，不回退演示业务数据', () => {
    expect(browserFallbackData.tasks).toEqual([])
    expect(browserFallbackData.records).toEqual([])
    expect(browserFallbackData.metrics.every((metric) => metric.value === '—')).toBe(true)
    expect(JSON.stringify(browserFallbackData)).not.toContain('example.local')
    expect(JSON.stringify(browserFallbackData)).not.toContain('8,742')

    queryMock.current = {
      data: browserFallbackData,
      error: null,
      isLoading: false,
      isSuccess: true,
    }

    const result = renderWorkbenchHook()

    expect(result.data.runtimeMode).toBe('unavailable')
    expect(result.actionMessage).toContain('不展示预览数据')
    expect(result.actionMessage).not.toContain('后端可用')
  })
})
