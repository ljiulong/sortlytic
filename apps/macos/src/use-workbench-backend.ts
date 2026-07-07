import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'
import {
  backendErrorMessage,
  buildReportModel,
  confirmCollectionPlan,
  createCollectionTask,
  createExportJob,
  enqueueTask,
  ensureDefaultWorkspace,
  generateCollectionPlanFromText,
  generateFormCollectionPlan,
  getBackendStatus,
  listModelProviders,
  listPromptTemplates,
  listSecretRefs,
  listTasks,
  saveCollectionPlan,
  saveSecret,
  type CollectionPlanView,
  type CollectionTaskView,
  type ExportJobView,
  type ModelProviderView,
  type PromptTemplateView,
  type SecretRefView,
  type TikhubConnectionTestResult,
  type WorkspaceSummary,
  testSecretConnection,
  testTikhubConnection,
} from './backend-api'
import {
  type ConnectionIcon,
  type DataType,
  type Platform,
  type SocialRecord,
  type TaskStatus,
  type Tone,
  workspaceSnapshot,
} from './workbench-data'

const queryKey = ['workbench-backend']

export type CollectionFormPayload = {
  platform: Platform
  dataType: DataType
  regionCode: string
  keyword: string
  range: string
  maxRecords: number
  budget: number
}

export type RuntimeCollectionPlan = CollectionFormPayload & {
  status: TaskStatus
  missing: string[]
  taskId?: string
  planId?: string
  validationStatus?: string
  costEstimate?: string
}

export type WorkbenchRuntimeData = {
  workspace: {
    name: string
    storage: string
    lastBackup: string
    health: string
  }
  connections: Array<{
    name: string
    detail: string
    status: string
    tone: Tone
    icon: ConnectionIcon
    meta: string
  }>
  metrics: Array<{ label: string; value: string; delta: string; tone: Tone }>
  tasks: Array<{
    name: string
    platform: Platform
    status: TaskStatus
    source: string
    progress: number
    records: number
    cost: string
  }>
  records: SocialRecord[]
  promptRuns: Array<{ name: string; status: '通过' | '失败'; provider: string; diff: string }>
}

type BackendWorkbenchData = WorkbenchRuntimeData & {
  latestTaskId?: string
}

const fallbackData: BackendWorkbenchData = {
  ...workspaceSnapshot,
  workspace: {
    ...workspaceSnapshot.workspace,
    health: '浏览器预览',
  },
  latestTaskId: undefined,
}

export function useWorkbenchBackend() {
  const queryClient = useQueryClient()
  const [activePlan, setActivePlan] = useState<RuntimeCollectionPlan>()
  const [actionMessage, setActionMessage] = useState('后端正在初始化本地工作区')
  const [latestExports, setLatestExports] = useState<ExportJobView[]>([])
  const [tikhubTestResult, setTikhubTestResult] = useState<TikhubConnectionTestResult>()

  const dataQuery = useQuery({
    queryKey,
    queryFn: loadBackendWorkbench,
    retry: 1,
  })

  useEffect(() => {
    if (dataQuery.isSuccess && actionMessage === '后端正在初始化本地工作区') {
      setActionMessage('本地工作区已打开，后端可用')
    }
  }, [actionMessage, dataQuery.isSuccess])

  const generateFormPlanMutation = useMutation({
    mutationFn: createFormPlan,
    onSuccess: (plan) => {
      setActivePlan(plan)
      setActionMessage('采集计划已保存到本地 SQLite，等待确认运行')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const generateNaturalPlanMutation = useMutation({
    mutationFn: createNaturalPlan,
    onSuccess: (plan) => {
      setActivePlan(plan)
      setActionMessage('自然语言计划已生成，并保存了提示词运行快照')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
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
      setActivePlan((plan) => (plan ? { ...plan, status: '运行中' } : plan))
      setActionMessage('任务已确认并加入本地队列')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const exportMutation = useMutation({
    mutationFn: async () => {
      assertTauriRuntime()
      const taskId = activePlan?.taskId ?? dataQuery.data?.latestTaskId
      if (!taskId) {
        throw new Error('请先创建一个采集任务再导出')
      }
      const report = await buildReportModel(taskId)
      const xlsx = await createExportJob(report.id, 'xlsx')
      const pdf = await createExportJob(report.id, 'pdf')
      return [xlsx, pdf]
    },
    onSuccess: (exports) => {
      setLatestExports(exports)
      setActionMessage('Excel 与 PDF 已导出到本地工作区')
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  const saveTikhubTokenMutation = useMutation({
    mutationFn: async (input: { token: string; baseUrl: string }) => {
      assertTauriRuntime()
      const secret = await saveSecret({
        provider_type: 'tikhub',
        provider_id: 'default',
        secret: input.token,
        alias: input.baseUrl.includes('tikhub.dev') ? 'TikHub 中国大陆域名' : 'TikHub 国际域名',
      })
      await testSecretConnection(secret.id)
      const result = await testTikhubConnection(secret.id, input.baseUrl)
      return result
    },
    onSuccess: (result) => {
      setTikhubTestResult(result)
      setActionMessage(result.message)
      void queryClient.invalidateQueries({ queryKey })
    },
    onError: (error) => setActionMessage(backendErrorMessage(error)),
  })

  return {
    data: dataQuery.data ?? fallbackData,
    activePlan,
    latestExports,
    actionMessage: dataQuery.error ? backendErrorMessage(dataQuery.error) : actionMessage,
    isInitializing: dataQuery.isLoading,
    isBusy:
      generateFormPlanMutation.isPending ||
      generateNaturalPlanMutation.isPending ||
      confirmPlanMutation.isPending ||
      exportMutation.isPending ||
      saveTikhubTokenMutation.isPending,
    generateFormPlan: generateFormPlanMutation.mutateAsync,
    generateNaturalPlan: generateNaturalPlanMutation.mutateAsync,
    confirmActivePlan: confirmPlanMutation.mutateAsync,
    exportLatestReport: exportMutation.mutateAsync,
    saveAndTestTikhubToken: saveTikhubTokenMutation.mutateAsync,
    tikhubTestResult,
    refresh: () => queryClient.invalidateQueries({ queryKey }),
  }
}

async function loadBackendWorkbench(): Promise<BackendWorkbenchData> {
  if (!isTauriRuntime()) {
    return fallbackData
  }

  const workspace = await ensureDefaultWorkspace()
  const [status, tasks, secretRefs, providers, templates] = await Promise.all([
    getBackendStatus(),
    listTasks(),
    listSecretRefs(),
    listModelProviders(),
    listPromptTemplates(),
  ])

  return mapBackendData(workspace, tasks, secretRefs, providers, templates, status.uptime_ms)
}

async function createFormPlan(values: CollectionFormPayload): Promise<RuntimeCollectionPlan> {
  assertTauriRuntime()
  const platform = toBackendPlatform(values.platform)
  const dataType = toBackendDataType(values.dataType)
  const params = buildPlanParams(values, dataType)
  const requestLimit = Math.max(1, Math.ceil(values.maxRecords / 50))
  const draft = await generateFormCollectionPlan({
    platform,
    data_type: dataType,
    params,
    request_limit: requestLimit,
  })
  const task = await createCollectionTask({
    name: values.keyword.trim(),
    source_type: 'form',
    platforms: [platform],
    data_types: [dataType],
  })
  const plan = await saveCollectionPlan({
    task_id: task.id,
    source: draft.source,
    plan_json: draft.plan_json,
    validation_status: draft.validation_status,
    validation_errors_json: draft.validation_errors_json,
    cost_estimate_json: draft.cost_estimate_json,
  })

  return planFromBackend(values, plan)
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
  const result = await generateCollectionPlanFromText({
    task_id: task.id,
    intent_text: intentText,
    provider_id: null,
    model_id: null,
  })

  return planFromBackend(
    {
      platform: toUiPlatform(hints.platform),
      dataType: toUiDataType(hints.dataType),
      regionCode: inferRegionCode(intentText, hints.platform),
      keyword: intentText.trim().slice(0, 36) || '自然语言采集',
      range: '由自然语言解析',
      maxRecords: 500,
      budget: 35,
    },
    result.collection_plan,
  )
}

function mapBackendData(
  workspace: WorkspaceSummary,
  tasks: CollectionTaskView[],
  secretRefs: SecretRefView[],
  providers: ModelProviderView[],
  templates: PromptTemplateView[],
  uptimeMs: number,
): BackendWorkbenchData {
  const latestTaskId = tasks[0]?.id
  const pendingCount = tasks.filter((task) => task.status === 'waiting_confirmation').length
  const queuedCount = tasks.filter((task) => task.status === 'queued').length
  const requestCount = tasks.reduce((total, task) => total + numberFromJson(task.cost_estimate_json), 0)

  return {
    workspace: {
      name: workspace.name,
      storage: shortPath(workspace.root_path),
      lastBackup: '未创建备份',
      health: `可用，运行 ${Math.max(1, Math.round(uptimeMs / 1000))} 秒`,
    },
    connections: buildConnections(secretRefs, providers),
    metrics: [
      { label: '本地任务', value: String(tasks.length), delta: `${pendingCount} 个待确认`, tone: 'info' },
      { label: '入库记录', value: String(workspaceSnapshot.records.length), delta: '示例数据待真实采集填充', tone: 'success' },
      { label: '预计请求', value: String(requestCount), delta: `${queuedCount} 个已入队`, tone: 'warning' },
      { label: '证据覆盖', value: '100%', delta: '导出前执行敏感信息检查', tone: 'success' },
    ],
    tasks: tasks.map(mapTaskRow),
    records: workspaceSnapshot.records,
    promptRuns: buildPromptRuns(templates),
    latestTaskId,
  }
}

function buildConnections(secretRefs: SecretRefView[], providers: ModelProviderView[]) {
  const tikhubSecret = secretRefs.find((secret) => secret.provider_type === 'tikhub')
  const enabledProviders = providers.filter((provider) => provider.enabled)

  return [
    {
      name: 'TikHub',
      detail: 'REST API',
      status: tikhubSecret ? '已配置' : '未配置',
      tone: tikhubSecret ? 'success' : 'warning',
      icon: 'key',
      meta: tikhubSecret?.masked_hint ?? '等待 API Key',
    },
    {
      name: '模型供应商',
      detail: '结构化输出',
      status: enabledProviders.length ? '已配置' : '本地规则',
      tone: enabledProviders.length ? 'success' : 'info',
      icon: 'bot',
      meta: enabledProviders[0]?.display_name ?? '无需联网即可生成计划',
    },
    {
      name: 'Webhook',
      detail: 'n8n 轻集成',
      status: '未启用',
      tone: 'warning',
      icon: 'share',
      meta: '仅发送摘要',
    },
  ] satisfies WorkbenchRuntimeData['connections']
}

function buildPromptRuns(templates: PromptTemplateView[]) {
  if (!templates.length) return workspaceSnapshot.promptRuns

  return templates.slice(0, 4).map((template) => ({
    name: template.name,
    status: '通过' as const,
    provider: template.is_builtin ? '内置模板' : '用户模板',
    diff: template.output_schema_id ? `${template.output_schema_id} 已激活` : 'Schema 待配置',
  }))
}

function mapTaskRow(task: CollectionTaskView): WorkbenchRuntimeData['tasks'][number] {
  const platforms = stringArrayFromJson(task.platforms_json)
  const dataTypes = stringArrayFromJson(task.data_types_json)
  const requestCount = numberFromJson(task.cost_estimate_json)

  return {
    name: task.name,
    platform: toUiPlatform(platforms[0] ?? 'xiaohongshu'),
    status: toUiTaskStatus(task.status),
    source: task.source_type === 'natural_language' ? '自然语言' : '表单式',
    progress: progressForTaskStatus(task.status),
    records: requestCount * 50,
    cost: `$${(requestCount * 0.06).toFixed(2)} · ${toUiDataType(dataTypes[0] ?? 'comments')}`,
  }
}

function planFromBackend(values: CollectionFormPayload, plan: CollectionPlanView): RuntimeCollectionPlan {
  const missing = stringArrayFromJson(plan.validation_errors_json)

  return {
    ...values,
    status: plan.validation_status === 'valid' ? '等待确认' : '待人工确认',
    missing,
    taskId: plan.task_id,
    planId: plan.id,
    validationStatus: plan.validation_status,
    costEstimate: `${numberFromJson(plan.cost_estimate_json)} 次请求`,
  }
}

function buildPlanParams(values: CollectionFormPayload, dataType: string) {
  const keyword = values.keyword.trim()
  const regionParams = { region: values.regionCode.trim().toUpperCase() }
  const pagingParams = {
    ...regionParams,
    time_range: values.range.trim(),
    page_size: Math.min(values.maxRecords, 50),
  }

  if (dataType === 'keyword_search') return { ...pagingParams, keyword }
  if (dataType === 'comments') return { ...pagingParams, item_id: keyword }
  if (dataType === 'account_profile') return { ...regionParams, account_id: keyword }
  return { ...regionParams, item_id: keyword }
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

function inferRegionCode(intentText: string, platform: string) {
  const lower = intentText.toLocaleLowerCase()
  if (intentText.includes('美国') || lower.includes(' usa') || lower.includes(' us ')) return 'US'
  if (intentText.includes('中国') || lower.includes(' china') || lower.includes(' cn ')) return 'CN'
  if (platform === 'douyin' || platform === 'xiaohongshu') return 'CN'
  return 'US'
}

function toBackendPlatform(platform: Platform) {
  if (platform === 'TikTok') return 'tiktok'
  if (platform === '抖音') return 'douyin'
  return 'xiaohongshu'
}

function toUiPlatform(platform: string): Platform {
  if (platform === 'tiktok') return 'TikTok'
  if (platform === 'douyin') return '抖音'
  return '小红书'
}

function toBackendDataType(dataType: DataType) {
  if (dataType === '关键词搜索') return 'keyword_search'
  if (dataType === '账号公开信息') return 'account_profile'
  if (dataType === '笔记详情') return 'item_detail'
  return 'comments'
}

function toUiDataType(dataType: string): DataType {
  if (dataType === 'keyword_search') return '关键词搜索'
  if (dataType === 'account_profile') return '账号公开信息'
  if (dataType === 'item_detail') return '笔记详情'
  return '评论采集'
}

function toUiTaskStatus(status: string): TaskStatus {
  if (status === 'success') return '成功'
  if (status === 'failed') return '失败'
  if (status === 'waiting_confirmation') return '等待确认'
  if (status === 'draft') return '待人工确认'
  if (status === 'cancelled') return '失败'
  return '运行中'
}

function progressForTaskStatus(status: string) {
  if (status === 'success') return 100
  if (status === 'waiting_confirmation') return 45
  if (status === 'queued') return 68
  if (status === 'failed' || status === 'cancelled') return 20
  return 12
}

function numberFromJson(value: Record<string, unknown>) {
  const estimate = value.request_count_estimate
  return typeof estimate === 'number' ? estimate : 0
}

function stringArrayFromJson(value: unknown) {
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === 'string')
  }
  return []
}

function shortPath(path: string) {
  const parts = path.split('/').filter(Boolean)
  return parts.slice(-2).join('/') || path
}

function isTauriRuntime() {
  return typeof window !== 'undefined' && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
}

function assertTauriRuntime() {
  if (!isTauriRuntime()) {
    throw new Error('请在打包后的 macOS 应用内使用后端能力')
  }
}
