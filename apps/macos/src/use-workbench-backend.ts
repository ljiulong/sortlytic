import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  backendErrorMessage,
  buildReportModel,
  cancelTask,
  confirmCollectionPlan,
  createCollectionTask,
  createExportJob,
  deleteTask,
  enqueueTask,
  estimateTaskCost,
  ensureDefaultWorkspace,
  generateAccountCollectionPlan,
  getActiveWorkspace,
  getLatestCollectionPlan,
  type GenerateFormPlanInput,
  type GenerateAccountPlanInput,
  getBackendStatus,
  listLatestTaskRuns,
  listLatestTaskIntents,
  listTaskRecordCounts,
  listTasks,
  retryTask as retryCollectionTask,
  saveCollectionPlan,
  type CollectionPlanView,
  type ExportJobView,
  updateCollectionTask,
} from './backend-api'
import { getApiProfileRegistry } from './api-profiles'
import { useAppUpdater } from './use-app-updater'
import { buildPlanParams } from './collection-plan-client'
import {
  preflightCollectionPlanPricing,
  pricingEndpointsForPlan,
} from './collection-pricing'
import type { CollectionDataType } from './collection-options'
import type { AccountSourceKey } from './collection-options'
import {
  numberFromJson,
  stringArrayFromJson,
  toUiDataType,
  toUiPlatform,
} from './workbench-task-mapper'
import {
  type DataType,
  type Platform,
  type TaskStatus,
} from './workbench-data'
import {
  mapBackendData,
  type BackendWorkbenchData,
} from './workbench-backend-mapper'
import {
  createIdleNaturalParseState,
  createPreparingNaturalParseState,
  resolveNaturalParseState,
  type NaturalParseState,
} from './natural-parse-state'
import {
  createNaturalTaskAttempt,
  describeNaturalParseFailure,
  parseNaturalTaskAttempt,
  type SuccessfulNaturalTaskAttempt,
} from './natural-task-attempt'

export {
  mapBackendData,
  type BackendWorkbenchData,
  type WorkbenchRuntimeData,
} from './workbench-backend-mapper'

const queryKey = ['workbench-backend']
const activeTaskRefetchIntervalMs = 2_000

export type CollectionFormPayload = {
  platform: Platform
  dataType: DataType
  regionCode: string
  queryLocale?: string
  keyword: string
  range: string
  maxRecords: number
  budget: number
  accountSource?: AccountSourceKey
  selectedFields?: string[]
  dataTypes?: CollectionDataType[]
  ageRangeEnabled?: boolean
  ageMin?: number
  ageMax?: number
  genderFilterEnabled?: boolean
  genders?: Array<'male' | 'female' | 'other'>
}

export type TaskExportInput = {
  taskId: string
  format: 'xlsx' | 'pdf'
}

export type RuntimeCollectionPlan = Omit<CollectionFormPayload, 'dataTypes'> & {
  platforms?: Platform[]
  dataTypes?: DataType[]
  targetDataTypes?: CollectionDataType[]
  status: TaskStatus
  missing: string[]
  taskId?: string
  planId?: string
  validationStatus?: string
  costEstimate?: string
  pricingEndpoints?: string[]
  requestCountEstimate?: number
  discoveryRequestCount?: number
  enrichmentRequestCount?: number
  budgetMicros?: number
  pricingReady?: boolean
  pricingBlocker?: string
}

const initialActionMessage = '后端正在初始化本地工作区'

function createEmptyWorkbenchData(mode: 'loading' | 'error' | 'unavailable'): BackendWorkbenchData {
  const isError = mode === 'error'
  const isUnavailable = mode === 'unavailable'
  const stateLabel = isError
    ? '后端读取失败'
    : isUnavailable
      ? '仅打包后的 macOS 应用可读取'
      : '正在读取真实数据'

  return {
    workspace: {
      name: '本地工作区',
      storage: '尚未读取',
      lastBackup: '尚未读取',
      health: isError ? '后端不可用' : isUnavailable ? '未连接本地后端' : '正在加载',
    },
    connections: [],
    metrics: [
      {
        label: '本地任务',
        value: '—',
        delta: stateLabel,
        tone: isError ? 'danger' : 'info',
      },
      {
        label: '入库记录',
        value: '—',
        delta: stateLabel,
        tone: isError ? 'danger' : 'info',
      },
      {
        label: '预计请求',
        value: '—',
        delta: stateLabel,
        tone: isError ? 'danger' : 'info',
      },
      {
        label: '证据覆盖',
        value: '—',
        delta: stateLabel,
        tone: isError ? 'danger' : 'info',
      },
    ],
    tasks: [],
    records: [],
    promptRuns: [],
    naturalParseAttempts: [],
    currentNaturalParseAttempts: [],
    latestTaskId: undefined,
    runtimeMode: mode,
  }
}

export const browserFallbackData = createEmptyWorkbenchData('unavailable')

export async function confirmPersistedTask(taskId: string) {
  assertTauriRuntime()
  const plan = await getLatestCollectionPlan(taskId)
  const estimate = await estimateTaskCost(taskId)
  const platforms = stringArrayFromJson(plan.plan_json.platforms)
  const dataTypes = stringArrayFromJson(plan.plan_json.data_types)
  const runtimePlan = planFromBackend(
    {
      platform: toUiPlatform(platforms[0] ?? nonEmptyString(plan.plan_json.platform) ?? ''),
      dataType: toUiDataType(dataTypes[0] ?? nonEmptyString(plan.plan_json.data_type) ?? ''),
      regionCode: '',
      keyword: '',
      range: '',
      maxRecords: 0,
      budget: 0,
    },
    plan,
  )
  runtimePlan.requestCountEstimate = estimate.request_count_estimate
  runtimePlan.costEstimate = `${estimate.request_count_estimate} 次请求`
  if (runtimePlan.validationStatus !== 'valid') {
    throw new Error(runtimePlan.missing[0] ?? '计划校验未通过，无法确认运行')
  }
  await confirmCollectionPlan(taskId, plan.id)
  return enqueueTask(taskId)
}

export async function retryPersistedTask(taskId: string) {
  assertTauriRuntime()
  return retryCollectionTask(taskId)
}

export async function exportTaskArtifact({ taskId, format }: TaskExportInput) {
  assertTauriRuntime()
  const report = await buildReportModel(taskId, format === 'pdf' ? 'analysis' : 'summary')
  return createExportJob(report.id, format)
}

export function useWorkbenchBackend() {
  const queryClient = useQueryClient()
  const [activePlan, setActivePlan] = useState<RuntimeCollectionPlan>()
  const [actionMessage, setActionMessage] = useState(initialActionMessage)
  const [latestExports, setLatestExports] = useState<ExportJobView[]>([])
  const [naturalParseState, setNaturalParseState] = useState<NaturalParseState>(
    createIdleNaturalParseState,
  )
  const appUpdater = useAppUpdater()

  const dataQuery = useQuery({
    queryKey,
    queryFn: loadBackendWorkbench,
    retry: 1,
    retryDelay: 250,
    placeholderData: (previousData) => previousData,
    refetchInterval: (query) => {
      const current = query.state.data as BackendWorkbenchData | undefined
      if (isNaturalParseInProgress(naturalParseState.phase)) return 1_000
      return current?.tasks.some((task) => ['已排队', '运行中'].includes(task.status))
        ? activeTaskRefetchIntervalMs
        : false
    },
    refetchIntervalInBackground: false,
  })

  const generateFormPlanMutation = useMutation({
    mutationFn: createFormPlan,
    retry: false,
    onSuccess: (plan) => {
      setActivePlan(plan)
      setActionMessage('采集计划已保存到本地 SQLite，等待确认运行')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => {
      setActionMessage(backendErrorMessage(error))
      void queryClient.invalidateQueries({ queryKey })
    },
  })

  const generateNaturalPlanMutation = useMutation({
    mutationFn: (intentText: string) => createNaturalPlan(intentText, (taskId) => {
      setNaturalParseState((state) => ({ ...state, phase: 'requesting_ai', taskId }))
    }),
    retry: false,
    onMutate: (intentText) => {
      setNaturalParseState(createPreparingNaturalParseState(intentText))
    },
    onSuccess: (plan) => {
      setActivePlan(plan)
      setNaturalParseState((state) => ({
        ...state,
        phase: 'success',
        taskId: plan.taskId,
        finishedAt: new Date().toISOString(),
        problem: undefined,
      }))
      setActionMessage('自然语言计划已生成，并保存了提示词运行快照')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => {
      const failure = describeNaturalParseFailure(error)
      setNaturalParseState((state) => ({
        ...state,
        phase: failure.phase,
        taskId: failure.taskId ?? state.taskId,
        finishedAt: new Date().toISOString(),
        problem: failure.problem,
        draftPreserved: true,
      }))
      setActionMessage(failure.problem.message)
      void queryClient.invalidateQueries({ queryKey })
    },
  })

  const confirmPlanMutation = useMutation({
    mutationFn: async () => {
      assertTauriRuntime()
      if (!activePlan?.taskId || !activePlan.planId) {
        throw new Error('请先生成采集计划')
      }
      await confirmCollectionPlan(activePlan.taskId, activePlan.planId)
      const run = await enqueueTask(activePlan.taskId)
      return run
    },
    onSuccess: () => {
      setActivePlan((plan) => (plan ? { ...plan, status: '已排队' } : plan))
      setActionMessage('任务已确认并加入本地队列')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const exportMutation = useMutation({
    mutationFn: exportTaskArtifact,
    onSuccess: (exportJob) => {
      setLatestExports((exports) => [
        exportJob,
        ...exports.filter((item) => item.export_type !== exportJob.export_type),
      ])
      setActionMessage(
        `${exportJob.export_type === 'xlsx' ? 'Excel' : 'PDF'} 已导出到本地工作区`,
      )
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const updateTaskMutation = useMutation({
    mutationFn: async ({ taskId, name }: { taskId: string; name: string }) => {
      assertTauriRuntime()
      const normalizedName = name.trim()
      if (normalizedName.length < 2) throw new Error('任务名称至少需要 2 个字符')
      return updateCollectionTask(taskId, { name: normalizedName })
    },
    onSuccess: () => {
      setActionMessage('任务名称已更新')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const cancelTaskMutation = useMutation({
    mutationFn: async (taskId: string) => {
      assertTauriRuntime()
      return cancelTask(taskId)
    },
    onSuccess: () => {
      setActionMessage('任务已取消')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const deleteTaskMutation = useMutation({
    mutationFn: async (taskId: string) => {
      assertTauriRuntime()
      return deleteTask(taskId)
    },
    onSuccess: (_, taskId) => {
      setActivePlan((plan) => (plan?.taskId === taskId ? undefined : plan))
      setActionMessage('任务已删除')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const confirmTaskMutation = useMutation({
    mutationFn: confirmPersistedTask,
    onSuccess: (run) => {
      setActivePlan((plan) => (
        plan?.taskId === run.task_id ? { ...plan, status: '已排队' } : plan
      ))
      setActionMessage('任务已确认并加入本地队列')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const retryTaskMutation = useMutation({
    mutationFn: retryPersistedTask,
    onSuccess: () => {
      setActionMessage('失败任务已重新加入本地队列')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const data = dataQuery.data ?? createEmptyWorkbenchData(dataQuery.error ? 'error' : 'loading')
  const resolvedNaturalParseState = resolveNaturalParseState(
    naturalParseState,
    data.currentNaturalParseAttempts ?? data.naturalParseAttempts,
  )
  const resolvedActionMessage = dataQuery.error
    ? backendErrorMessage(dataQuery.error)
    : actionMessage === initialActionMessage && dataQuery.isSuccess
      ? data.runtimeMode === 'unavailable'
        ? '当前未连接本地后端，不展示预览数据；请打开打包后的 macOS 应用'
        : '本地工作区已打开，后端可用'
      : actionMessage
  const retryNaturalParse = async (taskId: string, intentText: string) => {
    setNaturalParseState({
      ...createPreparingNaturalParseState(intentText),
      taskId,
      phase: 'requesting_ai',
    })
    try {
      const plan = naturalAttemptToRuntimePlan(await parseNaturalTaskAttempt(taskId, intentText))
      setActivePlan(plan)
      setNaturalParseState((state) => ({
        ...state,
        phase: 'success',
        taskId,
        finishedAt: new Date().toISOString(),
        problem: undefined,
      }))
      setActionMessage('自然语言计划已重新生成，原失败记录已保留')
      void queryClient.invalidateQueries({ queryKey })
      return plan
    } catch (error) {
      const failure = describeNaturalParseFailure(error)
      setNaturalParseState((state) => ({
        ...state,
        phase: failure.phase,
        taskId: failure.taskId ?? taskId,
        finishedAt: new Date().toISOString(),
        problem: failure.problem,
      }))
      setActionMessage(failure.problem.message)
      void queryClient.invalidateQueries({ queryKey })
      throw error
    }
  }

  return {
    data,
    activePlan,
    naturalParseState: resolvedNaturalParseState,
    latestExports,
    actionMessage: resolvedActionMessage,
    isInitializing: dataQuery.isLoading,
    isBusy:
      generateFormPlanMutation.isPending ||
      generateNaturalPlanMutation.isPending ||
      confirmPlanMutation.isPending ||
      exportMutation.isPending ||
      updateTaskMutation.isPending ||
      cancelTaskMutation.isPending ||
      deleteTaskMutation.isPending ||
      confirmTaskMutation.isPending ||
      retryTaskMutation.isPending ||
      isNaturalParseInProgress(resolvedNaturalParseState.phase) ||
      appUpdater.isUpdateBusy,
    generateFormPlan: generateFormPlanMutation.mutateAsync,
    generateNaturalPlan: generateNaturalPlanMutation.mutateAsync,
    retryNaturalParse,
    confirmActivePlan: confirmPlanMutation.mutateAsync,
    updateTask: updateTaskMutation.mutateAsync,
    cancelTask: cancelTaskMutation.mutateAsync,
    deleteTask: deleteTaskMutation.mutateAsync,
    confirmTask: confirmTaskMutation.mutateAsync,
    retryTask: retryTaskMutation.mutateAsync,
    exportTask: exportMutation.mutateAsync,
    ...appUpdater,
    refresh: () => queryClient.invalidateQueries({ queryKey }),
  }
}

export async function loadBackendWorkbench(): Promise<BackendWorkbenchData> {
  if (!isTauriRuntime()) {
    return browserFallbackData
  }

  const workspace = await getActiveWorkspace() ?? await ensureDefaultWorkspace()
  const [status, tasks, latestRuns, recordCounts, naturalParseAttempts, registry] = await Promise.all([
    getBackendStatus(),
    listTasks(),
    listLatestTaskRuns(),
    listTaskRecordCounts(),
    listLatestTaskIntents(),
    getApiProfileRegistry().catch(() => null),
  ])

  return mapBackendData(
    workspace,
    tasks,
    registry,
    status.uptime_ms,
    latestRuns,
    recordCounts,
    naturalParseAttempts,
  )
}

async function createFormPlan(values: CollectionFormPayload): Promise<RuntimeCollectionPlan> {
  assertTauriRuntime()
  const request = buildAccountPlanRequest(values)
  const draft = await generateAccountCollectionPlan(request)
  const task = await createCollectionTask({
    name: values.keyword.trim(),
    source_type: 'form',
    platforms: [request.platform],
    data_types: ['account'],
  })
  let runtimePlan: RuntimeCollectionPlan
  try {
    const plan = await saveCollectionPlan({
      task_id: task.id,
      source: draft.source,
      plan_json: draft.plan_json,
      validation_status: draft.validation_status,
      validation_errors_json: draft.validation_errors_json,
      cost_estimate_json: draft.cost_estimate_json,
    })
    runtimePlan = planFromBackend(values, plan)
  } catch (error) {
    return cleanupFailedDraftTask(task.id, error)
  }

  return preparePlanPricing(runtimePlan)
}

const keywordAccountSources = new Set<AccountSourceKey>([
  'user_search',
  'content_search_authors',
])
const itemAccountSources = new Set<AccountSourceKey>(['item_author', 'comment_authors'])

export function buildAccountPlanRequest(values: CollectionFormPayload): GenerateAccountPlanInput {
  if (!values.accountSource) throw new Error('请选择账号来源')
  const platform = toBackendPlatform(values.platform)
  const sourceInputKey = keywordAccountSources.has(values.accountSource)
    ? 'keyword'
    : itemAccountSources.has(values.accountSource)
      ? 'item_id'
      : 'account_id'
  const singleSource = ['direct_account', 'item_author'].includes(values.accountSource)
  const selectedFields = new Set(values.selectedFields ?? [])
  if (values.genderFilterEnabled) selectedFields.add('gender')
  if (values.ageRangeEnabled) selectedFields.add('age')
  const ageRange = values.ageRangeEnabled
    && values.ageMin !== undefined
    && values.ageMax !== undefined
    ? { min: values.ageMin, max: values.ageMax }
    : null
  const params: Record<string, unknown> = { [sourceInputKey]: values.keyword.trim() }
  if (values.regionCode) params.region = values.regionCode
  if (values.range) params.time_range = values.range

  return {
    platform,
    account_source: values.accountSource,
    selected_fields: [...selectedFields],
    enrichment_policy: 'auto_costed',
    params,
    age_range: ageRange,
    gender_filter: values.genderFilterEnabled ? values.genders ?? [] : null,
    request_limit: singleSource ? 1 : Math.max(1, Math.ceil(values.maxRecords / 20)),
    record_limit: singleSource ? 1 : values.maxRecords,
    budget_limit_micros: Math.round(values.budget * 1_000_000),
  }
}

export function buildFormPlanRequest(values: CollectionFormPayload): GenerateFormPlanInput {
  const platform = toBackendPlatform(values.platform)
  const dataTypes = values.dataTypes?.length
    ? [...new Set(values.dataTypes)]
    : [toBackendDataType(values.dataType)]
  const ageRange = values.ageRangeEnabled && values.ageMin !== undefined && values.ageMax !== undefined
    ? { min: values.ageMin, max: values.ageMax }
    : null

  return {
    platform,
    data_type: dataTypes[0],
    data_types: dataTypes,
    params: buildPlanParams(values, platform, 'keyword_search'),
    age_range: ageRange,
    request_limit: Math.max(1, Math.ceil(values.maxRecords / 50)),
    record_limit: values.maxRecords,
    budget_limit_micros: Math.round(values.budget * 1_000_000),
  }
}

async function createNaturalPlan(
  intentText: string,
  onTaskCreated?: (taskId: string) => void,
): Promise<RuntimeCollectionPlan> {
  assertTauriRuntime()
  return naturalAttemptToRuntimePlan(await createNaturalTaskAttempt(intentText, onTaskCreated))
}

function naturalAttemptToRuntimePlan(attempt: SuccessfulNaturalTaskAttempt) {
  const intent = attempt.intent
  return planFromBackend(
    {
      platform: toUiPlatform(intent?.platform ?? ''),
      dataType: toUiDataType('account'),
      regionCode: intent?.region_code ?? '',
      queryLocale: intent?.query_locale ?? '',
      keyword: intent?.source_input ?? '',
      range: intent?.time_range_days ? String(intent.time_range_days) : '未提供时间范围',
      maxRecords: intent?.record_limit ?? 0,
      budget: (intent?.budget_limit_micros ?? 0) / 1_000_000,
    },
    attempt.plan,
  )
}

function isNaturalParseInProgress(phase: NaturalParseState['phase']) {
  return ['preparing', 'requesting_ai', 'validating_intent', 'building_plan'].includes(phase)
}

async function cleanupFailedDraftTask(taskId: string, originalError: unknown): Promise<never> {
  try {
    await deleteTask(taskId)
  } catch (cleanupError) {
    throw new Error(
      `${backendErrorMessage(originalError)}；草稿任务清理失败：${backendErrorMessage(cleanupError)}`
      + '。任务页可能存在待删除草稿，请手动删除。',
    )
  }
  throw originalError
}

export function planFromBackend(values: CollectionFormPayload, plan: CollectionPlanView): RuntimeCollectionPlan {
  const missing = stringArrayFromJson(plan.validation_errors_json)
  const platforms = stringArrayFromJson(plan.plan_json.platforms).map(toUiPlatform)
  const dataTypes = stringArrayFromJson(plan.plan_json.data_types).map(toUiDataType)
  const recordLimit = positiveNumber(plan.plan_json.record_limit)
  const budgetMicros = amountMicros(plan.plan_json.budget_limit)
  const requestCountEstimate = numberFromJson(plan.cost_estimate_json)
  const selectedFields = stringArrayFromJson(plan.plan_json.selected_fields)
  const discoveryRequestCount = nonNegativeNumber(
    plan.cost_estimate_json.discovery_request_count,
  )
  const enrichmentRequestCount = nonNegativeNumber(
    plan.cost_estimate_json.enrichment_request_count,
  )
  const useSubmittedLimits = plan.source === 'form_generated'
  const genders = stringArrayFromJson(plan.plan_json.gender_filter).filter(
    (value): value is 'male' | 'female' | 'other' => ['male', 'female', 'other'].includes(value),
  )
  const ageRange = ageRangeFromPlan(plan.plan_json.age_range)

  return {
    ...values,
    targetDataTypes: values.dataTypes,
    platforms,
    dataTypes,
    platform: platforms[0] ?? values.platform,
    dataType: dataTypes[0] ?? values.dataType,
    accountSource: accountSourceFromPlan(plan.plan_json.account_source) ?? values.accountSource,
    selectedFields: Array.isArray(plan.plan_json.selected_fields)
      ? selectedFields
      : values.selectedFields,
    regionCode: regionFromPlan(plan.plan_json.region),
    queryLocale: nonEmptyString(plan.plan_json.query_locale) ?? values.queryLocale,
    keyword: targetFromPlan(plan.plan_json) || '未提供采集对象',
    range: nonEmptyString(plan.plan_json.time_range) ?? '未提供时间范围',
    maxRecords: recordLimit ?? (useSubmittedLimits ? values.maxRecords : 0),
    budget: budgetMicros ? budgetMicros / 1_000_000 : (useSubmittedLimits ? values.budget : 0),
    ageRangeEnabled: Boolean(ageRange),
    ageMin: ageRange?.min,
    ageMax: ageRange?.max,
    genderFilterEnabled: genders.length > 0,
    genders,
    status: plan.validation_status === 'valid' ? '等待确认' : '待人工确认',
    missing,
    taskId: plan.task_id,
    planId: plan.id,
    validationStatus: plan.validation_status,
    costEstimate: `${requestCountEstimate} 次请求`,
    pricingEndpoints: pricingEndpointsForPlan(plan.plan_json),
    requestCountEstimate,
    discoveryRequestCount,
    enrichmentRequestCount,
    budgetMicros,
    pricingReady: false,
  }
}

function ageRangeFromPlan(value: unknown) {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  const min = (value as Record<string, unknown>).min
  const max = (value as Record<string, unknown>).max
  if (!Number.isInteger(min) || !Number.isInteger(max)) return undefined
  const normalizedMin = min as number
  const normalizedMax = max as number
  if (normalizedMin < 0 || normalizedMax > 130 || normalizedMin > normalizedMax) return undefined
  return { min: normalizedMin, max: normalizedMax }
}

async function preparePlanPricing(plan: RuntimeCollectionPlan) {
  try {
    const preview = await preflightCollectionPlanPricing(plan)
    return {
      ...plan,
      pricingReady: true,
      pricingBlocker: undefined,
      costEstimate: `${plan.requestCountEstimate ?? 0} 次请求，实时报价上限 $${(preview.quotedTotalMicros / 1_000_000).toFixed(4)}`,
    }
  } catch (error) {
    return { ...plan, pricingReady: false, pricingBlocker: backendErrorMessage(error) }
  }
}

function toBackendPlatform(platform: Platform) {
  if (platform === 'TikTok') return 'tiktok'
  if (platform === '抖音') return 'douyin'
  return 'xiaohongshu'
}

function toBackendDataType(dataType: DataType) {
  if (dataType === '搜索结果账号' || dataType === '关键词搜索') return 'keyword_search'
  if (dataType === '账号公开信息') return 'account_profile'
  if (dataType === '作品/笔记作者' || dataType === '笔记详情') return 'item_detail'
  if (dataType === '账号作品所属账号') return 'account_posts'
  return 'comments'
}

function positiveNumber(value: unknown) {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : undefined
}

function nonNegativeNumber(value: unknown) {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? value : undefined
}

function accountSourceFromPlan(value: unknown): AccountSourceKey | undefined {
  const source = nonEmptyString(value)
  if (!source) return undefined
  if ([
    'user_search',
    'content_search_authors',
    'direct_account',
    'item_author',
    'comment_authors',
    'followers',
    'followings',
    'similar_accounts',
  ].includes(source)) return source as AccountSourceKey
  return undefined
}

function amountMicros(value: unknown) {
  if (!value || typeof value !== 'object' || !('amount_micros' in value)) return undefined
  return positiveNumber(value.amount_micros)
}

function nonEmptyString(value: unknown) {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function regionFromPlan(value: unknown) {
  if (typeof value === 'string') return value.trim()
  if (value && typeof value === 'object' && 'value' in value) {
    return nonEmptyString(value.value) ?? ''
  }
  return ''
}

function targetFromPlan(planJson: Record<string, unknown>) {
  const keyword = stringArrayFromJson(planJson.keywords)[0]
  if (keyword) return keyword
  if (!Array.isArray(planJson.steps)) return ''
  for (const step of planJson.steps) {
    if (!step || typeof step !== 'object' || !('params' in step)) continue
    const params = step.params
    if (!params || typeof params !== 'object') continue
    for (const key of ['keyword', 'item_id', 'account_id']) {
      if (key in params) {
        const target = nonEmptyString(params[key as keyof typeof params])
        if (target) return target
      }
    }
  }
  return ''
}

function isTauriRuntime() {
  return typeof window !== 'undefined' && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
}

function assertTauriRuntime() {
  if (!isTauriRuntime()) {
    throw new Error('请在打包后的 macOS 应用内使用后端能力')
  }
}
