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
  generateCollectionPlanFromText,
  generateFormCollectionPlan,
  getLatestCollectionPlan,
  type GenerateFormPlanInput,
  getBackendStatus,
  listLatestTaskRuns,
  listTaskRecordCounts,
  listTasks,
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

export {
  mapBackendData,
  type WorkbenchRuntimeData,
} from './workbench-backend-mapper'

const queryKey = ['workbench-backend']
const activeTaskRefetchIntervalMs = 2_000

export type CollectionFormPayload = {
  platform: Platform
  dataType: DataType
  regionCode: string
  keyword: string
  range: string
  maxRecords: number
  budget: number
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
  await preflightCollectionPlanPricing(runtimePlan)
  await confirmCollectionPlan(taskId, plan.id)
  return enqueueTask(taskId)
}

export async function exportTaskArtifact({ taskId, format }: TaskExportInput) {
  assertTauriRuntime()
  const report = await buildReportModel(taskId)
  return createExportJob(report.id, format)
}

export function useWorkbenchBackend() {
  const queryClient = useQueryClient()
  const [activePlan, setActivePlan] = useState<RuntimeCollectionPlan>()
  const [actionMessage, setActionMessage] = useState(initialActionMessage)
  const [latestExports, setLatestExports] = useState<ExportJobView[]>([])
  const appUpdater = useAppUpdater()

  const dataQuery = useQuery({
    queryKey,
    queryFn: loadBackendWorkbench,
    retry: 1,
    refetchInterval: (query) => {
      const current = query.state.data as BackendWorkbenchData | undefined
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
    mutationFn: createNaturalPlan,
    retry: false,
    onSuccess: (plan) => {
      setActivePlan(plan)
      setActionMessage('自然语言计划已生成，并保存了提示词运行快照')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => {
      setActionMessage(backendErrorMessage(error))
      void queryClient.invalidateQueries({ queryKey })
    },
  })

  const confirmPlanMutation = useMutation({
    mutationFn: async () => {
      assertTauriRuntime()
      if (!activePlan?.taskId || !activePlan.planId) {
        throw new Error('请先生成采集计划')
      }
      await preflightCollectionPlanPricing(activePlan)
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

  const data = dataQuery.data ?? createEmptyWorkbenchData(dataQuery.error ? 'error' : 'loading')
  const resolvedActionMessage = dataQuery.error
    ? backendErrorMessage(dataQuery.error)
    : actionMessage === initialActionMessage && dataQuery.isSuccess
      ? data.runtimeMode === 'unavailable'
        ? '当前未连接本地后端，不展示预览数据；请打开打包后的 macOS 应用'
        : '本地工作区已打开，后端可用'
      : actionMessage

  return {
    data,
    activePlan,
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
      appUpdater.isUpdateBusy,
    generateFormPlan: generateFormPlanMutation.mutateAsync,
    generateNaturalPlan: generateNaturalPlanMutation.mutateAsync,
    confirmActivePlan: confirmPlanMutation.mutateAsync,
    updateTask: updateTaskMutation.mutateAsync,
    cancelTask: cancelTaskMutation.mutateAsync,
    deleteTask: deleteTaskMutation.mutateAsync,
    confirmTask: confirmTaskMutation.mutateAsync,
    exportTask: exportMutation.mutateAsync,
    ...appUpdater,
    refresh: () => queryClient.invalidateQueries({ queryKey }),
  }
}

export async function loadBackendWorkbench(): Promise<BackendWorkbenchData> {
  if (!isTauriRuntime()) {
    return browserFallbackData
  }

  const workspace = await ensureDefaultWorkspace()
  const [status, tasks, latestRuns, recordCounts, registry] = await Promise.all([
    getBackendStatus(),
    listTasks(),
    listLatestTaskRuns().catch(() => []),
    listTaskRecordCounts().catch(() => []),
    getApiProfileRegistry().catch(() => null),
  ])

  return mapBackendData(workspace, tasks, registry, status.uptime_ms, latestRuns, recordCounts)
}

async function createFormPlan(values: CollectionFormPayload): Promise<RuntimeCollectionPlan> {
  assertTauriRuntime()
  const request = buildFormPlanRequest(values)
  const draft = await generateFormCollectionPlan(request)
  const task = await createCollectionTask({
    name: values.keyword.trim(),
    source_type: 'form',
    platforms: [request.platform],
    data_types: request.data_types ?? [request.data_type ?? 'keyword_search'],
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

async function createNaturalPlan(intentText: string): Promise<RuntimeCollectionPlan> {
  assertTauriRuntime()
  const hints = inferNaturalPlanHints(intentText)
  const task = await createCollectionTask({
    name: intentText.trim().slice(0, 42) || '自然语言采集任务',
    source_type: 'natural_language',
    platforms: [hints.platform],
    data_types: [hints.dataType],
  })
  let runtimePlan: RuntimeCollectionPlan
  try {
    const result = await generateCollectionPlanFromText({
      task_id: task.id,
      intent_text: intentText,
      provider_id: null,
      model_id: null,
    })
    runtimePlan = planFromBackend(
      {
        platform: toUiPlatform(hints.platform),
        dataType: toUiDataType(hints.dataType),
        regionCode: '',
        keyword: '',
        range: '未提供时间范围',
        maxRecords: 0,
        budget: 0,
      },
      result.collection_plan,
    )
  } catch (error) {
    return cleanupFailedDraftTask(task.id, error)
  }

  return preparePlanPricing(runtimePlan)
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
  const useSubmittedLimits = plan.source === 'form_generated'
  const genders = stringArrayFromJson(plan.plan_json.gender_filter).filter(
    (value): value is 'male' | 'female' | 'other' => ['male', 'female', 'other'].includes(value),
  )

  return {
    ...values,
    targetDataTypes: values.dataTypes,
    platforms,
    dataTypes,
    platform: platforms[0] ?? values.platform,
    dataType: dataTypes[0] ?? values.dataType,
    regionCode: regionFromPlan(plan.plan_json.region),
    keyword: targetFromPlan(plan.plan_json) || '未提供采集对象',
    range: nonEmptyString(plan.plan_json.time_range) ?? '未提供时间范围',
    maxRecords: recordLimit ?? (useSubmittedLimits ? values.maxRecords : 0),
    budget: budgetMicros ? budgetMicros / 1_000_000 : (useSubmittedLimits ? values.budget : 0),
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
    budgetMicros,
    pricingReady: false,
  }
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

function inferNaturalPlanHints(intentText: string) {
  const lower = intentText.toLocaleLowerCase()
  const platform = intentText.includes('抖音')
    ? 'douyin'
    : intentText.includes('小红书')
      ? 'xiaohongshu'
      : lower.includes('tiktok')
        ? 'tiktok'
        : 'xiaohongshu'
  const dataType = intentText.includes('关键词') || lower.includes('keyword') ? 'keyword_search' : 'comments'

  return { platform, dataType }
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
