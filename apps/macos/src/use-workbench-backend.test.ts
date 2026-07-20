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
  listLatestTaskRuns,
  listTaskRecordCounts,
  listTaskResults,
  listTaskLogs,
  prepareAppUpdate,
  quoteTikhubConnectorPrice,
  relaunchAfterAppUpdate,
  type TaskRunView,
  updateCollectionTask,
  type WorkspaceSummary,
} from './backend-api'
import {
  browserFallbackData,
  buildAccountPlanRequest,
  buildFormPlanRequest,
  confirmPersistedTask,
  exportTaskArtifact,
  loadBackendWorkbench,
  mapBackendData,
  planFromBackend,
  type BackendWorkbenchData,
  type RuntimeCollectionPlan,
  useWorkbenchBackend,
} from './use-workbench-backend'
import {
  preflightCollectionPlanPricing,
  pricingEndpointsForPlan,
  resetCollectionPricingStateForTests,
} from './collection-pricing'
import { buildPlanParams } from './collection-plan-client'
import type { ApiProfileRegistryView } from './api-profiles'

type CapturedMutationOptions = {
  mutationFn?: (input: unknown) => Promise<unknown>
  onMutate?: (input: string) => unknown
  onSuccess?: (data: unknown) => unknown
  onError?: (error: unknown) => unknown
  retry?: boolean | number
}

type CapturedQueryOptions = {
  retry?: boolean | number
  retryDelay?: number
  placeholderData?: (previousData: BackendWorkbenchData) => BackendWorkbenchData
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

const generatedFormDraft = {
  source: 'form_generated',
  schema_version: 3,
  plan_json: {
    platforms: ['xiaohongshu'],
    data_types: ['keyword_search'],
    keywords: ['新能源汽车'],
    time_range: '7',
    record_limit: 100,
    budget_limit: { currency: 'USD', amount_micros: 1_000_000 },
    steps: [],
  },
  validation_status: 'valid',
  validation_errors_json: [],
  cost_estimate_json: { request_count_estimate: 2 },
}

const savedFormPlan: CollectionPlanView = {
  id: 'plan-draft-1',
  task_id: 'task-draft-1',
  ...generatedFormDraft,
  confirmed_by_user: false,
  created_at: '2026-07-18T00:00:00Z',
  updated_at: '2026-07-18T00:00:00Z',
}

const formPlanInput = {
  platform: '小红书',
  accountSource: 'user_search',
  selectedFields: ['avatar_url'],
  dataType: '关键词搜索',
  dataTypes: ['keyword_search'],
  regionCode: 'CN',
  keyword: '新能源汽车',
  range: '7',
  maxRecords: 100,
  budget: 1,
} satisfies Parameters<typeof buildFormPlanRequest>[0]

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
  resetCollectionPricingStateForTests()
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
  it('通过单个批量命令读取每条任务的最新运行状态', async () => {
    invokeMock.mockResolvedValue([])

    await listLatestTaskRuns()

    expect(invokeMock).toHaveBeenCalledWith('list_latest_task_runs', {
      rootPath: null,
    })
  })

  it('按最新运行 ID 读取经过后端脱敏的任务日志', async () => {
    invokeMock.mockResolvedValue([])

    await listTaskLogs('run-2')

    expect(invokeMock).toHaveBeenCalledWith('list_task_logs', {
      taskRunId: 'run-2',
      rootPath: null,
    })
  })

  it('通过单个批量命令读取每条任务的真实入库记录数', async () => {
    invokeMock.mockResolvedValue([])

    await listTaskRecordCounts()

    expect(invokeMock).toHaveBeenCalledWith('list_task_record_counts', {
      rootPath: null,
    })
  })

  it('按任务分页读取最新成功运行的应用内结果', async () => {
    invokeMock.mockResolvedValue({ items: [], total_count: 0 })

    await listTaskResults('task-1', 100, 200)

    expect(invokeMock).toHaveBeenCalledWith('list_task_results', {
      taskId: 'task-1',
      limit: 100,
      offset: 200,
      rootPath: null,
    })
  })

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

  it('从持久化计划直接确认和入队，不用额度预检阻塞运行', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
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
      if (command === 'estimate_task_cost') {
        return { request_count_estimate: 3 }
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
      'estimate_task_cost',
      'confirm_collection_plan',
      'enqueue_task',
    ])
  })

  it('预计总价超过设定上限时仍允许确认入队', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_latest_collection_plan') {
        return {
          id: 'plan-legacy-underestimated',
          task_id: 'task-legacy-underestimated',
          source: 'form_generated',
          schema_version: 3,
          plan_json: {
            platforms: ['xiaohongshu'],
            data_types: ['keyword_search', 'item_detail', 'account_profile', 'comments'],
            time_range: '7',
            record_limit: 1000,
            request_limit: 20,
            budget_limit: { amount_micros: 2_000_000 },
            steps: [{ endpoint_key: 'xiaohongshu.keyword_search' }],
          },
          validation_status: 'valid',
          validation_errors_json: [],
          cost_estimate_json: { request_count_estimate: 80 },
          confirmed_by_user: false,
          created_at: '2026-07-18T00:00:00Z',
          updated_at: '2026-07-18T00:00:00Z',
        }
      }
      if (command === 'estimate_task_cost') {
        return { request_count_estimate: 22_020 }
      }
      if (command === 'confirm_collection_plan') return task
      if (command === 'enqueue_task') {
        return { id: 'run-legacy', task_id: 'task-legacy-underestimated', status: 'queued' }
      }
      throw new Error(`意外命令：${command}`)
    })

    await expect(confirmPersistedTask('task-legacy-underestimated')).resolves.toMatchObject({
      task_id: 'task-legacy-underestimated',
      status: 'queued',
    })
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'get_latest_collection_plan',
      'estimate_task_cost',
      'confirm_collection_plan',
      'enqueue_task',
    ])
  })

  it('0.1 至 1.0 美元每档三轮都可确认入队，不触发额度阀门', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      const taskId = String(args?.taskId ?? '')
      const tenths = Number(/budget-(\d+)-round-\d+/.exec(taskId)?.[1] ?? 0)
      if (command === 'get_latest_collection_plan') {
        return {
          id: `plan-${taskId}`,
          task_id: taskId,
          source: 'form_generated',
          schema_version: 3,
          plan_json: {
            platforms: ['tiktok'],
            data_types: ['comments'],
            keywords: ['low-budget-matrix'],
            time_range: '1',
            record_limit: 10,
            budget_limit: { currency: 'USD', amount_micros: tenths * 100_000 },
            steps: [{ endpoint_key: 'tiktok.comments' }],
          },
          validation_status: 'valid',
          validation_errors_json: [],
          cost_estimate_json: { request_count_estimate: 1 },
          confirmed_by_user: false,
          created_at: '2026-07-19T00:00:00Z',
          updated_at: '2026-07-19T00:00:00Z',
        }
      }
      if (command === 'estimate_task_cost') return { request_count_estimate: 1 }
      if (command === 'confirm_collection_plan') return { ...task, id: taskId }
      if (command === 'enqueue_task') {
        return { id: `run-${taskId}`, task_id: taskId, status: 'queued' }
      }
      throw new Error(`意外命令：${command}`)
    })

    for (let round = 1; round <= 3; round += 1) {
      for (let tenths = 1; tenths <= 10; tenths += 1) {
        const taskId = `budget-${tenths}-round-${round}`
        await expect(confirmPersistedTask(taskId)).resolves.toMatchObject({
          task_id: taskId,
          status: 'queued',
        })
      }
    }

    const commands = invokeMock.mock.calls.map(([command]) => command)
    expect(commands.filter((command) => command === 'confirm_collection_plan')).toHaveLength(30)
    expect(commands.filter((command) => command === 'enqueue_task')).toHaveLength(30)
    expect(commands).not.toContain('get_api_profile_registry')
    expect(commands).not.toContain('test_api_profile')
    expect(commands).not.toContain('quote_tikhub_connector_price')
  })

  it('Excel 使用摘要模型，PDF 使用分析模型且只生成所选格式', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === 'build_report_model') {
        return { id: args?.reportType === 'analysis' ? 'report-analysis' : 'report-summary' }
      }
      if (command === 'create_export_job') {
        return {
          id: 'export-1',
          report_id: args?.reportId,
          export_type: args?.exportType,
          status: 'success',
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    await expect(exportTaskArtifact({ taskId: 'task-1', format: 'pdf' })).resolves.toMatchObject({
      report_id: 'report-analysis',
      export_type: 'pdf',
    })
    await expect(exportTaskArtifact({ taskId: 'task-1', format: 'xlsx' })).resolves.toMatchObject({
      report_id: 'report-summary',
      export_type: 'xlsx',
    })
    expect(invokeMock).toHaveBeenCalledWith('build_report_model', {
      taskId: 'task-1',
      reportType: 'analysis',
      rootPath: null,
    })
    expect(invokeMock).toHaveBeenCalledWith('build_report_model', {
      taskId: 'task-1',
      reportType: 'summary',
      rootPath: null,
    })
    expect(invokeMock).toHaveBeenCalledWith('create_export_job', {
      reportId: 'report-analysis',
      exportType: 'pdf',
      targetPath: null,
      rootPath: null,
    })
  })
})

describe('计划生成失败的草稿清理', () => {
  it('点击解析后立即进入 preparing 状态并保留原始输入', () => {
    renderWorkbenchHook()
    const generateNaturalMutation = mutationOptionsMock.current[1]
    const naturalParseStateSetter = stateSettersMock.current[3]

    generateNaturalMutation?.onMutate?.('  用中文查找英国 TikTok 宠物用品账号  ')

    expect(naturalParseStateSetter).toHaveBeenCalledWith(expect.objectContaining({
      phase: 'preparing',
      intentText: '用中文查找英国 TikTok 宠物用品账号',
      draftPreserved: true,
    }))
  })

  it('应用重启后从批量 attempt 数据恢复最近失败状态', () => {
    const failedAttempt = {
      id: 'attempt-failed',
      task_id: task.id,
      intent_text: '采集英国 TikTok 宠物用品账号',
      parse_status: 'failed' as const,
      parse_phase: 'requesting_ai',
      error_code: 'MODEL_RATE_LIMIT',
      error_message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
      retryable: true,
      error_safe_details_json: { retry_after: '17' },
      created_at: '2026-07-20T08:00:00Z',
      updated_at: '2026-07-20T08:00:17Z',
    }
    queryMock.current = {
      data: mapBackendData(
        workspace,
        [task],
        tikhubRegistryFixture(),
        1_000,
        [],
        [],
        [failedAttempt],
      ),
      error: null,
      isLoading: false,
      isSuccess: true,
    }

    const result = renderWorkbenchHook()

    expect(result.naturalParseState).toMatchObject({
      phase: 'failed',
      taskId: task.id,
      intentText: '采集英国 TikTok 宠物用品账号',
      problem: {
        code: 'MODEL_RATE_LIMIT',
        message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
        safeDetails: { retry_after: '17' },
      },
    })
  })

  it('账号表单把单一来源、字段和筛选转换为 v4 请求', () => {
    expect(buildAccountPlanRequest({
      ...formPlanInput,
      platform: '抖音',
      accountSource: 'user_search',
      selectedFields: ['avatar_url'],
      ageRangeEnabled: true,
      ageMin: 18,
      ageMax: 35,
      genderFilterEnabled: true,
      genders: ['female'],
    })).toEqual({
      platform: 'douyin',
      account_source: 'user_search',
      selected_fields: ['avatar_url', 'gender', 'age'],
      enrichment_policy: 'auto_costed',
      params: { keyword: '新能源汽车', region: 'CN', time_range: '7' },
      age_range: { min: 18, max: 35 },
      gender_filter: ['female'],
      request_limit: 5,
      record_limit: 100,
      budget_limit_micros: 1_000_000,
    })
  })

  it('当前表单缺少账号来源时在调用任何计划生成命令前拒绝', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    renderWorkbenchHook()
    const generateFormMutation = mutationOptionsMock.current[0]

    await expect(generateFormMutation?.mutationFn?.({
      ...formPlanInput,
      accountSource: undefined,
    }))
      .rejects.toThrow('请选择账号来源')
    expect(invokeMock).not.toHaveBeenCalled()
  })

  it('账号来源表单调用 v4 命令并把任务范围固定为 account', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'generate_account_collection_plan') return generatedFormDraft
      if (command === 'create_collection_task') {
        return { ...task, id: 'task-account-1', status: 'waiting_confirmation' }
      }
      if (command === 'save_collection_plan') return {
        ...savedFormPlan,
        task_id: 'task-account-1',
      }
      throw new Error(`意外命令：${command}`)
    })
    renderWorkbenchHook()
    const generateFormMutation = mutationOptionsMock.current[0]

    await generateFormMutation?.mutationFn?.({
      ...formPlanInput,
      accountSource: 'user_search',
      selectedFields: ['avatar_url'],
    })

    expect(invokeMock).toHaveBeenCalledWith('generate_account_collection_plan', {
      request: expect.objectContaining({
        account_source: 'user_search',
        selected_fields: ['avatar_url'],
      }),
    })
    expect(invokeMock).toHaveBeenCalledWith('create_collection_task', {
      input: expect.objectContaining({ data_types: ['account'] }),
      rootPath: null,
    })
  })

  it('表单计划保存失败后删除已创建的草稿任务', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'generate_account_collection_plan') return generatedFormDraft
      if (command === 'create_collection_task') {
        return { ...task, id: 'task-draft-1', status: 'waiting_confirmation' }
      }
      if (command === 'save_collection_plan') throw new Error('保存计划失败')
      if (command === 'delete_task') return undefined
      throw new Error(`意外命令：${command}`)
    })
    renderWorkbenchHook()
    const generateFormMutation = mutationOptionsMock.current[0]

    await expect(generateFormMutation?.mutationFn?.(formPlanInput))
      .rejects.toThrow('保存计划失败')
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'generate_account_collection_plan',
      'create_collection_task',
      'save_collection_plan',
      'delete_task',
    ])
    expect(invokeMock).toHaveBeenLastCalledWith('delete_task', {
      taskId: 'task-draft-1',
      rootPath: null,
    })
  })

  it('自然语言 AI 生成失败后保留草稿、原始输入和诊断记录', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'create_collection_task') {
        return { ...task, id: 'task-natural-1', status: 'waiting_confirmation' }
      }
      if (command === 'generate_collection_plan_from_text') throw new Error('AI 结构化输出无效')
      throw new Error(`意外命令：${command}`)
    })
    renderWorkbenchHook()
    const generateNaturalMutation = mutationOptionsMock.current[1]

    await expect(generateNaturalMutation?.mutationFn?.('采集小红书公开账号'))
      .rejects.toThrow('AI 结构化输出无效')
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'create_collection_task',
      'generate_collection_plan_from_text',
    ])
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'create_collection_task', {
      input: {
        name: '采集小红书公开账号',
        source_type: 'natural_language',
        platforms: [],
        data_types: [],
      },
      rootPath: null,
    })
  })

  it('自然语言解析成功后不在确认前调用 TikHub 配置、余额或报价', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'create_collection_task') {
        return { ...task, id: 'task-natural-1', status: 'draft' }
      }
      if (command === 'generate_collection_plan_from_text') {
        return {
          parsed_intent: {
            schema_version: 1,
            platform: 'tiktok',
            account_source: 'user_search',
            source_input: 'pet supplies',
            query_locale: 'en-GB',
            region_code: 'GB',
            selected_fields: [],
            time_range_days: null,
            age_range: null,
            gender_filter: null,
            record_limit: 10,
            budget_limit_micros: 100_000,
            missing_fields: [],
            confidence: 0.95,
          },
          issues: [],
          collection_plan: {
            ...savedFormPlan,
            task_id: 'task-natural-1',
            plan_json: {
              ...savedFormPlan.plan_json,
              platforms: ['tiktok'],
              keywords: [],
              account_source: 'user_search',
              region: 'GB',
              time_range: null,
              record_limit: 10,
              budget_limit: { currency: 'USD', amount_micros: 100_000 },
              selected_fields: [],
              steps: [{
                endpoint_key: 'tiktok.user_search',
                params: { keyword: 'pet supplies' },
              }],
            },
          },
        }
      }
      throw new Error(`意外命令：${command}`)
    })
    renderWorkbenchHook()
    const generateNaturalMutation = mutationOptionsMock.current[1]

    await expect(generateNaturalMutation?.mutationFn?.(
      '用中文查找英国 TikTok 宠物用品账号，最多 10 个，预算 0.1 美元。',
    )).resolves.toMatchObject({
      taskId: 'task-natural-1',
      keyword: 'pet supplies',
      regionCode: 'GB',
      pricingReady: false,
    })
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'create_collection_task',
      'generate_collection_plan_from_text',
    ])
  })

  it('重新解析复用原失败任务，不新建或删除任务', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'generate_collection_plan_from_text') {
        return {
          parsed_intent: {
            schema_version: 1,
            platform: 'tiktok',
            account_source: 'user_search',
            source_input: 'pet supplies',
            query_locale: 'en-GB',
            region_code: 'GB',
            selected_fields: [],
            time_range_days: null,
            age_range: null,
            gender_filter: null,
            record_limit: 10,
            budget_limit_micros: 100_000,
            missing_fields: [],
            confidence: 0.95,
          },
          issues: [],
          collection_plan: {
            ...savedFormPlan,
            task_id: 'task-natural-failed',
            plan_json: {
              platforms: ['tiktok'],
              account_source: 'user_search',
              region: 'GB',
              time_range: null,
              record_limit: 10,
              budget_limit: { currency: 'USD', amount_micros: 100_000 },
              selected_fields: [],
              steps: [{
                endpoint_key: 'tiktok.user_search',
                params: { keyword: 'pet supplies' },
              }],
            },
          },
        }
      }
      throw new Error(`意外命令：${command}`)
    })
    const result = renderWorkbenchHook()

    await expect(result.retryNaturalParse(
      'task-natural-failed',
      '用中文查找英国 TikTok 宠物用品账号',
    )).resolves.toMatchObject({
      taskId: 'task-natural-failed',
      keyword: 'pet supplies',
    })
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'generate_collection_plan_from_text',
    ])
  })

  it('草稿清理失败时同时返回原始错误与明确的人工清理提示', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'generate_account_collection_plan') return generatedFormDraft
      if (command === 'create_collection_task') {
        return { ...task, id: 'task-draft-1', status: 'waiting_confirmation' }
      }
      if (command === 'save_collection_plan') throw new Error('原始保存错误')
      if (command === 'delete_task') throw new Error('数据库删除失败')
      throw new Error(`意外命令：${command}`)
    })
    renderWorkbenchHook()
    const generateFormMutation = mutationOptionsMock.current[0]

    await expect(generateFormMutation?.mutationFn?.(formPlanInput)).rejects.toThrow(
      /原始保存错误.*草稿任务清理失败.*数据库删除失败.*手动删除/,
    )
  })

  it('表单和自然语言生成均禁止自动重试，且 onError 后刷新任务查询', async () => {
    renderWorkbenchHook()
    const generateFormMutation = mutationOptionsMock.current[0]
    const generateNaturalMutation = mutationOptionsMock.current[1]

    expect(generateFormMutation?.retry).toBe(false)
    expect(generateNaturalMutation?.retry).toBe(false)

    await generateFormMutation?.onError?.(new Error('表单生成失败'))
    await generateNaturalMutation?.onError?.(new Error('自然语言生成失败'))

    expect(invalidateQueriesMock).toHaveBeenCalledTimes(2)
    expect(invalidateQueriesMock).toHaveBeenNthCalledWith(1, { queryKey: ['workbench-backend'] })
    expect(invalidateQueriesMock).toHaveBeenNthCalledWith(2, { queryKey: ['workbench-backend'] })
  })

  it('定价预检失败只返回 pricingBlocker，不删除已保存计划的任务', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'generate_account_collection_plan') return generatedFormDraft
      if (command === 'create_collection_task') {
        return { ...task, id: 'task-draft-1', status: 'waiting_confirmation' }
      }
      if (command === 'save_collection_plan') return savedFormPlan
      throw new Error(`意外命令：${command}`)
    })
    renderWorkbenchHook()
    const generateFormMutation = mutationOptionsMock.current[0]

    await expect(generateFormMutation?.mutationFn?.(formPlanInput)).resolves.toMatchObject({
      taskId: 'task-draft-1',
      pricingReady: false,
      pricingBlocker: 'TikHub 计价端点未知，无法确认运行',
    })
    expect(invokeMock).not.toHaveBeenCalledWith('delete_task', expect.anything())
  })
})

describe('应用更新 API', () => {
  it('检查到新版本后分别准备更新和按用户操作重启', async () => {
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
    await prepareAppUpdate()

    expect(updaterInstallMock).toHaveBeenCalledOnce()
    expect(relaunchMock).not.toHaveBeenCalled()

    await relaunchAfterAppUpdate()

    expect(relaunchMock).toHaveBeenCalledOnce()
  })

  it('没有新版本时清空待安装版本并阻止误安装', async () => {
    updaterCheckMock.mockResolvedValue(null)

    await expect(checkForAppUpdate()).resolves.toBeNull()
    await expect(prepareAppUpdate()).rejects.toThrow('请先检查更新')
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

  it('多端点计划逐个读取实时报价，避免并发触发 TikHub 限流', async () => {
    const registry = tikhubRegistryFixture({
      balance: 4.99,
      freeCredit: 0.05,
      availableCredit: 5.04,
    })
    let activeQuoteRequests = 0
    let maxConcurrentQuoteRequests = 0
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        return { success: true, message: 'TikHub API 配置测试成功', registry }
      }
      if (command === 'quote_tikhub_connector_price') {
        activeQuoteRequests += 1
        maxConcurrentQuoteRequests = Math.max(maxConcurrentQuoteRequests, activeQuoteRequests)
        await Promise.resolve()
        try {
          if (activeQuoteRequests > 1) {
            throw new Error('TikHub 请求失败，HTTP 429：请求过于频繁')
          }
          return { total_price: 0.01 }
        } finally {
          activeQuoteRequests -= 1
        }
      }
      throw new Error(`意外命令：${command}`)
    })

    await expect(preflightCollectionPlanPricing({
      ...plan,
      budgetMicros: 2_000_000,
      requestCountEstimate: 80,
      pricingEndpoints: [
        '/api/v1/xiaohongshu/app_v2/search_notes',
        '/api/v1/xiaohongshu/app_v2/get_image_note_detail',
        '/api/v1/xiaohongshu/app_v2/get_video_note_detail',
        '/api/v1/xiaohongshu/app_v2/get_user_info',
        '/api/v1/xiaohongshu/app_v2/get_note_comments',
      ],
    })).resolves.toMatchObject({ quotedTotalMicros: 800_000 })
    expect(maxConcurrentQuoteRequests).toBe(1)
  })

  it('多端点串行报价仍保持最小请求间隔，避免触发时间窗口限流', async () => {
    vi.useFakeTimers()
    try {
      const registry = tikhubRegistryFixture({
        balance: 4.99,
        freeCredit: 0.05,
        availableCredit: 5.04,
      })
      let lastQuoteStartedAt: number | null = null
      invokeMock.mockImplementation(async (command: string) => {
        if (command === 'get_api_profile_registry') return registry
        if (command === 'test_api_profile') {
          return { success: true, message: 'TikHub API 配置测试成功', registry }
        }
        if (command === 'quote_tikhub_connector_price') {
          const startedAt = Date.now()
          if (lastQuoteStartedAt !== null && startedAt - lastQuoteStartedAt < 250) {
            throw {
              code: 'TIKHUB_RATE_LIMIT',
              message: 'TikHub 请求失败，HTTP 429：请求过于频繁',
              retryable: true,
            }
          }
          lastQuoteStartedAt = startedAt
          return { total_price: 0.01 }
        }
        throw new Error(`意外命令：${command}`)
      })

      const assertion = expect(preflightCollectionPlanPricing({
        ...plan,
        budgetMicros: 2_000_000,
        requestCountEstimate: 80,
        pricingEndpoints: [
          '/api/v1/tiktok/app/v3/fetch_video_search_result',
          '/api/v1/tiktok/app/v3/fetch_one_video',
          '/api/v1/tiktok/app/v3/handler_user_profile',
          '/api/v1/tiktok/app/v3/fetch_video_comments',
        ],
      })).resolves.toMatchObject({ quotedTotalMicros: 800_000 })

      await vi.runAllTimersAsync()
      await assertion
    } finally {
      vi.useRealTimers()
    }
  })

  it('相同配置与计划的连续预检复用短期成功结果', async () => {
    const registry = tikhubRegistryFixture({
      balance: 4.99,
      freeCredit: 0.05,
      availableCredit: 5.04,
    })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        return { success: true, message: 'TikHub API 配置测试成功', registry }
      }
      if (command === 'quote_tikhub_connector_price') return { total_price: 0.01 }
      throw new Error(`意外命令：${command}`)
    })
    const cachePlan = {
      ...plan,
      budgetMicros: 2_000_000,
      requestCountEstimate: 80,
      pricingEndpoints: [
        '/api/v1/douyin/search/fetch_video_search_v2',
        '/api/v1/douyin/app/v3/fetch_one_video',
      ],
    }

    await preflightCollectionPlanPricing(cachePlan)
    await preflightCollectionPlanPricing(cachePlan)

    expect(invokeMock.mock.calls.filter(([command]) => command === 'test_api_profile')).toHaveLength(1)
    expect(
      invokeMock.mock.calls.filter(([command]) => command === 'quote_tikhub_connector_price'),
    ).toHaveLength(2)
  })

  it('相同配置与计划的并发预检合并为同一组远端请求', async () => {
    const registry = tikhubRegistryFixture({
      balance: 4.99,
      freeCredit: 0.05,
      availableCredit: 5.04,
    })
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        await Promise.resolve()
        return { success: true, message: 'TikHub API 配置测试成功', registry }
      }
      if (command === 'quote_tikhub_connector_price') {
        await Promise.resolve()
        return { total_price: 0.01 }
      }
      throw new Error(`意外命令：${command}`)
    })
    const concurrentPlan = {
      ...plan,
      budgetMicros: 2_000_000,
      requestCountEstimate: 80,
      pricingEndpoints: [
        '/api/v1/douyin/app/v3/handler_user_profile',
        '/api/v1/douyin/app/v3/fetch_video_comments',
      ],
    }

    await Promise.all([
      preflightCollectionPlanPricing(concurrentPlan),
      preflightCollectionPlanPricing(concurrentPlan),
    ])

    expect(invokeMock.mock.calls.filter(([command]) => command === 'test_api_profile')).toHaveLength(1)
    expect(
      invokeMock.mock.calls.filter(([command]) => command === 'quote_tikhub_connector_price'),
    ).toHaveLength(2)
  })

  it('后段报价失败后保留前面已成功的端点报价供下次预检续用', async () => {
    const registry = tikhubRegistryFixture({
      balance: 4.99,
      freeCredit: 0.05,
      availableCredit: 5.04,
    })
    const quoteCalls = new Map<string, number>()
    invokeMock.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        return { success: true, message: 'TikHub API 配置测试成功', registry }
      }
      if (command === 'quote_tikhub_connector_price') {
        const endpoint = String(args?.endpoint)
        const calls = (quoteCalls.get(endpoint) ?? 0) + 1
        quoteCalls.set(endpoint, calls)
        if (endpoint.endsWith('get_image_note_detail') && calls === 1) {
          throw new Error('TikHub 临时计价失败')
        }
        return { total_price: 0.01 }
      }
      throw new Error(`意外命令：${command}`)
    })
    const resumePlan = {
      ...plan,
      budgetMicros: 2_000_000,
      requestCountEstimate: 80,
      pricingEndpoints: [
        '/api/v1/xiaohongshu/app_v2/search_notes',
        '/api/v1/xiaohongshu/app_v2/get_image_note_detail',
        '/api/v1/xiaohongshu/app_v2/get_video_note_detail',
      ],
    }

    await expect(preflightCollectionPlanPricing(resumePlan)).rejects.toThrow('TikHub 临时计价失败')
    await expect(preflightCollectionPlanPricing(resumePlan)).resolves.toMatchObject({
      quotedTotalMicros: 800_000,
    })

    expect(quoteCalls.get('/api/v1/xiaohongshu/app_v2/search_notes')).toBe(1)
    expect(quoteCalls.get('/api/v1/xiaohongshu/app_v2/get_image_note_detail')).toBe(2)
    expect(quoteCalls.get('/api/v1/xiaohongshu/app_v2/get_video_note_detail')).toBe(1)
  })

  it('预计总价超过余额上限时只返回参考信息而不阻塞', async () => {
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

    await expect(preflightCollectionPlanPricing(plan)).resolves.toMatchObject({
      availableCredit: 0.02,
      quotedTotalMicros: 60_000,
    })
  })

  it('预计总价超过设定上限时只返回参考信息而不阻塞', async () => {
    const registry = tikhubRegistryFixture()
    invokeMock.mockImplementation(async (command: string) => {
      if (command === 'get_api_profile_registry') return registry
      if (command === 'test_api_profile') {
        return { success: true, message: 'TikHub API 配置测试成功', registry }
      }
      if (command === 'quote_tikhub_connector_price') return { total_price: 0.02 }
      throw new Error(`意外命令：${command}`)
    })

    await expect(preflightCollectionPlanPricing({
      ...plan,
      budgetMicros: 10_000,
    })).resolves.toMatchObject({
      availableCredit: 0.3,
      quotedTotalMicros: 60_000,
    })
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
  it('用一次批量结果把最近自然语言解析记录合并到对应任务', () => {
    const attempt = {
      id: 'attempt-1',
      task_id: task.id,
      intent_text: '用中文查找英国 TikTok 宠物用品账号',
      parse_status: 'failed' as const,
      parse_phase: 'requesting_ai',
      error_code: 'MODEL_RATE_LIMIT',
      error_message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
      retryable: true,
      error_safe_details_json: { retry_after: '17' },
      created_at: '2026-07-20T08:00:00Z',
      updated_at: '2026-07-20T08:00:17Z',
    }

    const result = mapBackendData(
      workspace,
      [task],
      tikhubRegistryFixture(),
      1_000,
      [],
      [],
      [attempt],
    )

    expect(result.naturalParseAttempts).toEqual([attempt])
    expect(result.tasks[0]?.naturalParseAttempt).toEqual(attempt)
  })

  it('把 SQLite 标准化记录数关联到对应任务', () => {
    const result = mapBackendData(
      workspace,
      [task],
      tikhubRegistryFixture(),
      1_000,
      [],
      [{ task_id: task.id, record_count: 42 }],
    )

    expect(result.tasks[0]?.records).toBe(42)
    expect(result.metrics).toContainEqual({
      label: '入库记录',
      value: '42',
      delta: 'records_available:1',
      tone: 'success',
    })
    expect(result.metrics.some((metric) => metric.label === '证据覆盖')).toBe(false)
  })

  it('把最新运行阶段、安全错误和重试状态关联到对应任务', () => {
    const run: TaskRunView = {
      id: 'run-2',
      task_id: task.id,
      plan_id: 'plan-1',
      attempt_number: 2,
      claimed_at: '2026-07-12T00:01:00Z',
      status: 'failed',
      started_at: '2026-07-12T00:01:00Z',
      ended_at: '2026-07-12T00:02:00Z',
      current_stage: '持久化采集结果',
      current_stage_code: 'PERSISTING_RESULTS',
      error_code: 'TIKHUB_REQUEST_ERROR',
      error_message: 'TikHub 请求超时',
      retryable: true,
      cost_actual_json: {},
    }
    const result = mapBackendData(
      workspace,
      [{ ...task, status: 'failed' }],
      tikhubRegistryFixture(),
      1_000,
      [run],
    )

    expect(result.tasks[0]?.latestRun).toEqual({
      id: 'run-2',
      attemptNumber: 2,
      currentStage: '持久化采集结果',
      currentStageCode: 'PERSISTING_RESULTS',
      errorCode: 'TIKHUB_REQUEST_ERROR',
      errorMessage: 'TikHub 请求超时',
      retryable: true,
      startedAt: '2026-07-12T00:01:00Z',
      endedAt: '2026-07-12T00:02:00Z',
      status: 'failed',
    })
  })

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
  it('账号 Schema v4 的发现、关系与补全步骤全部进入实时计价端点', () => {
    const endpoints = pricingEndpointsForPlan({
      steps: [
        { endpoint_key: 'tiktok.user_search' },
        { endpoint_key: 'tiktok.followers' },
        { endpoint_key: 'tiktok.followings' },
        { endpoint_key: 'tiktok.similar_accounts' },
        { endpoint_key: 'tiktok.account_country' },
        { endpoint_key: 'douyin.user_search' },
        { endpoint_key: 'douyin.followers' },
        { endpoint_key: 'douyin.followings' },
        { endpoint_key: 'douyin.extended_demographics' },
        { endpoint_key: 'xiaohongshu.user_search' },
      ],
    })

    expect(endpoints).toEqual([
      '/api/v1/tiktok/app/v3/fetch_user_search_result',
      '/api/v1/tiktok/app/v3/fetch_user_follower_list',
      '/api/v1/tiktok/app/v3/fetch_user_following_list',
      '/api/v1/tiktok/app/v3/fetch_similar_user_recommendations',
      '/api/v1/tiktok/app/v3/fetch_user_country_by_username',
      '/api/v1/douyin/search/fetch_user_search',
      '/api/v1/douyin/web/fetch_user_fans_list',
      '/api/v1/douyin/web/fetch_user_following_list',
      '/api/v1/douyin/web/handler_user_profile_v4',
      '/api/v1/xiaohongshu/app_v2/search_users',
    ])
  })

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

  it('保留账号来源、所选字段和发现补全请求分项', () => {
    const plan: CollectionPlanView = {
      id: 'plan-account-facts',
      task_id: 'task-account',
      source: 'form_generated',
      schema_version: 4,
      plan_json: {
        platforms: ['douyin'],
        account_source: 'user_search',
        selected_fields: ['avatar_url', 'gender', 'age'],
        record_limit: 10,
        budget_limit: { currency: 'USD', amount_micros: 1_000_000 },
        steps: [],
      },
      validation_status: 'valid',
      validation_errors_json: [],
      cost_estimate_json: {
        request_count_estimate: 11,
        discovery_request_count: 1,
        enrichment_request_count: 10,
      },
      confirmed_by_user: false,
      created_at: '2026-07-20T00:00:00Z',
      updated_at: '2026-07-20T00:00:00Z',
    }

    const result = planFromBackend({
      platform: '抖音',
      dataType: '账号公开信息',
      regionCode: '',
      keyword: '汽车',
      range: '',
      maxRecords: 10,
      budget: 1,
    }, plan)

    expect(result.accountSource).toBe('user_search')
    expect(result.selectedFields).toEqual(['avatar_url', 'gender', 'age'])
    expect(result.discoveryRequestCount).toBe(1)
    expect(result.enrichmentRequestCount).toBe(10)
    expect(result.requestCountEstimate).toBe(11)
  })

  it('自然语言 Schema v4 计划保留年龄闭区间供确认页展示', () => {
    const plan: CollectionPlanView = {
      id: 'plan-natural-age',
      task_id: 'task-natural-age',
      source: 'ai_generated',
      schema_version: 4,
      plan_json: {
        platforms: ['douyin'],
        account_source: 'user_search',
        selected_fields: ['age'],
        age_range: { min: 18, max: 35 },
        record_limit: 10,
        budget_limit: { currency: 'USD', amount_micros: 1_000_000 },
        steps: [],
      },
      validation_status: 'valid',
      validation_errors_json: [],
      cost_estimate_json: { request_count_estimate: 11 },
      confirmed_by_user: false,
      created_at: '2026-07-20T00:00:00Z',
      updated_at: '2026-07-20T00:00:00Z',
    }

    const result = planFromBackend({
      platform: '抖音',
      dataType: '账号数据',
      regionCode: '',
      keyword: '',
      range: '',
      maxRecords: 0,
      budget: 0,
    }, plan)

    expect(result.ageRangeEnabled).toBe(true)
    expect(result.ageMin).toBe(18)
    expect(result.ageMax).toBe(35)
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
      if (command === 'get_active_workspace') return Promise.resolve(workspace)
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
    expect(invokeMock).not.toHaveBeenCalledWith('ensure_default_workspace')
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

  it('后台刷新使用短退避并保留最近一次成功快照', () => {
    renderWorkbenchHook()
    const options = queryOptionsMock.current
    const previous = mapBackendData(workspace, [task], tikhubRegistryFixture(), 1_000)

    expect(options?.retry).toBe(1)
    expect(options?.retryDelay).toBe(250)
    expect(options?.placeholderData?.(previous)).toBe(previous)
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
