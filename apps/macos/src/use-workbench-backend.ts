import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import {
  backendErrorMessage,
  buildReportModel,
  confirmCollectionPlan,
  createModelProvider,
  createCollectionTask,
  createExportJob,
  enqueueTask,
  ensureDefaultWorkspace,
  generateCollectionPlanFromText,
  generateFormCollectionPlan,
  type GenerateFormPlanInput,
  getBackendStatus,
  getTikhubConnector,
  listModelProviders,
  listSecretRefs,
  listTasks,
  saveCollectionPlan,
  saveSecret,
  saveTikhubConnector,
  setActiveModelProvider,
  setDefaultModel,
  type CollectionPlanView,
  type CollectionTaskView,
  type ExportJobView,
  type ModelProviderView,
  type ProviderTestResult,
  type SecretRefView,
  type TikhubConnectionTestResult,
  type TikhubConnectorView,
  type WorkspaceSummary,
  testSecretConnection,
  testTikhubConnector,
  testModelProvider,
  updateModelProvider,
  updateSecret,
  upsertModelProfile,
} from './backend-api'
import { useAppUpdater } from './use-app-updater'
import { buildPlanParams } from './collection-plan-client'
import type { CollectionDataType } from './collection-options'
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
  dataTypes?: CollectionDataType[]
  ageRangeEnabled?: boolean
  ageMin?: number
  ageMax?: number
  genderFilterEnabled?: boolean
  genders?: Array<'male' | 'female' | 'other'>
}

export type ModelSettingsInput = {
  providerId: string
  displayName: string
  apiFormat: 'openai_compatible' | 'anthropic_messages' | 'gemini' | 'ollama'
  baseUrl: string
  defaultModelId: string
  apiKey: string
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
}

export type WorkbenchRuntimeData = {
  workspace: {
    name: string
    storage: string
    lastBackup: string
    health: string
  }
  tikhubConnector?: TikhubConnectorView | null
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
    id: string
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
  modelProviders: ModelProviderView[]
}

type BackendWorkbenchData = WorkbenchRuntimeData & {
  latestTaskId?: string
  runtimeMode: 'backend' | 'demo' | 'loading' | 'error'
}

const initialActionMessage = '后端正在初始化本地工作区'

const browserPreviewData: BackendWorkbenchData = {
  ...workspaceSnapshot,
  modelProviders: [],
  workspace: {
    ...workspaceSnapshot.workspace,
    health: '浏览器预览',
  },
  latestTaskId: undefined,
  runtimeMode: 'demo',
}

function createEmptyWorkbenchData(mode: 'loading' | 'error'): BackendWorkbenchData {
  const isError = mode === 'error'

  return {
    workspace: {
      name: '本地工作区',
      storage: '尚未读取',
      lastBackup: '尚未读取',
      health: isError ? '后端不可用' : '正在加载',
    },
    connections: [],
    metrics: [
      {
        label: '本地任务',
        value: '—',
        delta: isError ? '后端读取失败' : '正在读取真实数据',
        tone: isError ? 'danger' : 'info',
      },
      {
        label: '入库记录',
        value: '—',
        delta: isError ? '后端读取失败' : '正在读取真实数据',
        tone: isError ? 'danger' : 'info',
      },
      {
        label: '预计请求',
        value: '—',
        delta: isError ? '后端读取失败' : '正在读取真实数据',
        tone: isError ? 'danger' : 'info',
      },
      {
        label: '证据覆盖',
        value: '—',
        delta: isError ? '后端读取失败' : '正在读取真实数据',
        tone: isError ? 'danger' : 'info',
      },
    ],
    tasks: [],
    records: [],
    promptRuns: [],
    modelProviders: [],
    latestTaskId: undefined,
    runtimeMode: mode,
  }
}

export function useWorkbenchBackend() {
  const queryClient = useQueryClient()
  const [activePlan, setActivePlan] = useState<RuntimeCollectionPlan>()
  const [actionMessage, setActionMessage] = useState(initialActionMessage)
  const [latestExports, setLatestExports] = useState<ExportJobView[]>([])
  const [tikhubTestResult, setTikhubTestResult] = useState<TikhubConnectionTestResult>()
  const [modelValidationResult, setModelValidationResult] = useState<ProviderTestResult>()
  const [isModelSettingsPending, setIsModelSettingsPending] = useState(false)
  const [isModelActivationPending, setIsModelActivationPending] = useState(false)
  const appUpdater = useAppUpdater()

  const dataQuery = useQuery({
    queryKey,
    queryFn: loadBackendWorkbench,
    retry: 1,
  })

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
      setActivePlan((plan) => (plan ? { ...plan, status: '已排队' } : plan))
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
      return saveAndTestTikhubToken(input)
    },
    onSuccess: async (result) => {
      setTikhubTestResult(result)
      setActionMessage(result.message)
      await queryClient.invalidateQueries({ queryKey })
    },
    onError: async (error) => {
      setTikhubTestResult(undefined)
      setActionMessage(backendErrorMessage(error))
      await queryClient.invalidateQueries({ queryKey })
    },
  })

  const saveAndValidateModelProvider = async (input: ModelSettingsInput) => {
    assertTauriRuntime()
    setIsModelSettingsPending(true)
    setModelValidationResult(undefined)

    try {
      const providers = await listModelProviders()
      const existingProvider = providers.find(
        (provider) => provider.provider_id === input.providerId,
      )
      const apiKey = input.apiKey.trim()
      const secretRefId = existingProvider?.secret_ref_id
      if (!secretRefId && apiKey.length < 8) {
        throw new Error('请先输入至少 8 位模型 API Key')
      }
      if (secretRefId && apiKey) {
        await updateSecret(secretRefId, apiKey)
      }
      const savedSecretRefId = secretRefId ?? (await saveSecret({
        provider_type: 'model_provider',
        provider_id: input.providerId,
        secret: apiKey,
        alias: `${input.displayName} API Key`,
      })).id
      await testSecretConnection(savedSecretRefId)

      const providerInput = {
        provider_id: input.providerId,
        display_name: input.displayName,
        enabled: true,
        auth_type: 'api_key' as const,
        secret_ref_id: savedSecretRefId,
        base_url: input.baseUrl.trim() || null,
        api_format: input.apiFormat,
        region: null,
        cost_policy_json: null,
        rate_limit_policy_json: null,
        health_check_json: null,
      }

      if (existingProvider) {
        await updateModelProvider(input.providerId, providerInput)
      } else {
        await createModelProvider(providerInput)
      }

      await upsertModelProfile({
        provider_id: input.providerId,
        model_id: input.defaultModelId,
        display_name: input.defaultModelId,
        capabilities_json: null,
        context_window: null,
        supports_structured_output: false,
        supports_streaming: false,
        supports_tools: false,
        supports_vision: false,
        enabled: true,
      })
      await setDefaultModel(input.providerId, input.defaultModelId)
      await setActiveModelProvider(input.providerId)

      const result = await testModelProvider(input.providerId, input.defaultModelId)
      setModelValidationResult(result)
      setActionMessage(result.message)
      await queryClient.invalidateQueries({ queryKey })
      return result
    } catch (error) {
      setActionMessage(backendErrorMessage(error))
      throw error
    } finally {
      setIsModelSettingsPending(false)
    }
  }

  const activateModelProvider = async (providerId: string) => {
    assertTauriRuntime()
    setIsModelActivationPending(true)
    try {
      await setActiveModelProvider(providerId)
      setActionMessage('模型 API 配置已切换')
      await queryClient.invalidateQueries({ queryKey })
    } catch (error) {
      setActionMessage(backendErrorMessage(error))
      throw error
    } finally {
      setIsModelActivationPending(false)
    }
  }

  const data = dataQuery.data ?? createEmptyWorkbenchData(dataQuery.error ? 'error' : 'loading')
  const resolvedActionMessage = dataQuery.error
    ? backendErrorMessage(dataQuery.error)
    : actionMessage === initialActionMessage && dataQuery.isSuccess
      ? data.runtimeMode === 'demo'
        ? '浏览器演示模式：未连接 Tauri 后端，当前内容均为演示数据'
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
      saveTikhubTokenMutation.isPending ||
      isModelSettingsPending ||
      isModelActivationPending ||
      appUpdater.isUpdateBusy,
    generateFormPlan: generateFormPlanMutation.mutateAsync,
    generateNaturalPlan: generateNaturalPlanMutation.mutateAsync,
    confirmActivePlan: confirmPlanMutation.mutateAsync,
    exportLatestReport: exportMutation.mutateAsync,
    saveAndTestTikhubToken: saveTikhubTokenMutation.mutateAsync,
    tikhubTestResult,
    saveAndValidateModelProvider,
    modelValidationResult,
    isModelSettingsPending,
    isModelActivationPending,
    activateModelProvider,
    ...appUpdater,
    refresh: () => queryClient.invalidateQueries({ queryKey }),
  }
}

async function loadBackendWorkbench(): Promise<BackendWorkbenchData> {
  if (!isTauriRuntime()) {
    return browserPreviewData
  }

  const workspace = await ensureDefaultWorkspace()
  const [status, tasks, secretRefs, connector, providers] = await Promise.all([
    getBackendStatus(),
    listTasks(),
    listSecretRefs(),
    getTikhubConnector(),
    listModelProviders(),
  ])

  return mapBackendData(workspace, tasks, secretRefs, connector, providers, status.uptime_ms)
}

export async function saveAndTestTikhubToken(input: { token: string; baseUrl: string }) {
  const token = input.token.trim()
  const connector = await getTikhubConnector()
  const secretRefs = await listSecretRefs('tikhub')
  const boundSecret = connector?.secret_ref_id
    ? secretRefs.find(
        (secret) => secret.id === connector.secret_ref_id && secret.provider_type === 'tikhub',
      )
    : undefined
  if (boundSecret) {
    await saveTikhubConnector({
      secret_ref_id: boundSecret.id,
      base_url: input.baseUrl,
      enabled: true,
    })
    if (token) {
      await updateSecret(boundSecret.id, token)
    }
  } else {
    if (token.length < 8) {
      throw new Error('请先输入至少 8 位 TikHub Token')
    }
    const secret = await saveSecret({
      provider_type: 'tikhub',
      provider_id: 'default',
      secret: token,
      alias: input.baseUrl.includes('tikhub.dev')
        ? 'TikHub 中国大陆域名'
        : 'TikHub 国际域名',
    })
    await saveTikhubConnector({
      secret_ref_id: secret.id,
      base_url: input.baseUrl,
      enabled: true,
    })
  }
  return testTikhubConnector()
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
      regionCode: '',
      keyword: '',
      range: '未提供时间范围',
      maxRecords: 0,
      budget: 0,
    },
    result.collection_plan,
  )
}

export function mapBackendData(
  workspace: WorkspaceSummary,
  tasks: CollectionTaskView[],
  secretRefs: SecretRefView[],
  connector: TikhubConnectorView | null,
  providers: ModelProviderView[],
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
    tikhubConnector: connector,
    connections: buildConnections(secretRefs, connector, providers),
    metrics: [
      { label: '本地任务', value: String(tasks.length), delta: `${pendingCount} 个待确认`, tone: 'info' },
      { label: '入库记录', value: '0', delta: '真实记录读取尚未接入', tone: 'info' },
      { label: '预计请求', value: String(requestCount), delta: `${queuedCount} 个已入队`, tone: 'warning' },
      { label: '证据覆盖', value: '未计算', delta: '暂无真实记录', tone: 'info' },
    ],
    tasks: tasks.map(mapTaskRow),
    records: [],
    promptRuns: [],
    modelProviders: providers,
    latestTaskId,
    runtimeMode: 'backend',
  }
}

function buildConnections(
  secretRefs: SecretRefView[],
  connector: TikhubConnectorView | null,
  providers: ModelProviderView[],
) {
  const tikhubSecret = connector?.secret_ref_id
    ? secretRefs.find(
        (secret) => secret.id === connector.secret_ref_id && secret.provider_type === 'tikhub',
      )
    : undefined
  const officialBaseUrl = isOfficialTikhubBaseUrl(connector?.base_url)
    ? connector?.base_url
    : undefined
  const tikhubMeta = [officialBaseUrl, tikhubSecret?.masked_hint].filter(Boolean).join(' · ')
  let tikhubStatus = '未配置'
  let tikhubTone: Tone = 'warning'

  if (connector) {
    if (!connector.enabled) {
      tikhubStatus = '已禁用'
    } else if (!tikhubSecret) {
      tikhubStatus = '需重新绑定'
      tikhubTone = 'danger'
    } else if (connector.last_test_status === 'success') {
      tikhubStatus = '已验证'
      tikhubTone = 'success'
    } else if (connector.last_test_status === 'failed') {
      tikhubStatus = '测试失败'
      tikhubTone = 'danger'
    } else {
      tikhubStatus = '待测试'
      tikhubTone = 'info'
    }
  }
  const enabledProviders = providers.filter((provider) => provider.enabled)

  return [
    {
      name: 'TikHub',
      detail: 'REST API',
      status: tikhubStatus,
      tone: tikhubTone,
      icon: 'key',
      meta: tikhubMeta || '等待配置连接器',
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

function isOfficialTikhubBaseUrl(baseUrl?: string | null) {
  return baseUrl === 'https://api.tikhub.io' || baseUrl === 'https://api.tikhub.dev'
}

function mapTaskRow(task: CollectionTaskView): WorkbenchRuntimeData['tasks'][number] {
  const platforms = stringArrayFromJson(task.platforms_json)
  const dataTypes = stringArrayFromJson(task.data_types_json)
  const requestCount = numberFromJson(task.cost_estimate_json)

  return {
    id: task.id,
    name: task.name,
    platform: toUiPlatform(platforms[0] ?? 'xiaohongshu'),
    status: toUiTaskStatus(task.status),
    source: task.source_type === 'natural_language' ? '自然语言' : '表单式',
    progress: progressForTaskStatus(task.status),
    records: 0,
    cost: `${requestCount ? `预计 ${requestCount} 次请求` : '尚无请求估算'} · ${toUiDataType(dataTypes[0] ?? 'comments')}`,
  }
}

export function planFromBackend(values: CollectionFormPayload, plan: CollectionPlanView): RuntimeCollectionPlan {
  const missing = stringArrayFromJson(plan.validation_errors_json)
  const platforms = stringArrayFromJson(plan.plan_json.platforms).map(toUiPlatform)
  const dataTypes = stringArrayFromJson(plan.plan_json.data_types).map(toUiDataType)
  const recordLimit = positiveNumber(plan.plan_json.record_limit)
  const budgetLimit = positiveNumber(plan.plan_json.budget_limit)
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
    budget: budgetLimit ?? (useSubmittedLimits ? values.budget : 0),
    genderFilterEnabled: genders.length > 0,
    genders,
    status: plan.validation_status === 'valid' ? '等待确认' : '待人工确认',
    missing,
    taskId: plan.task_id,
    planId: plan.id,
    validationStatus: plan.validation_status,
    costEstimate: `${numberFromJson(plan.cost_estimate_json)} 次请求`,
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

function toUiPlatform(platform: string): Platform {
  if (platform === 'tiktok') return 'TikTok'
  if (platform === 'douyin') return '抖音'
  return '小红书'
}

function toBackendDataType(dataType: DataType) {
  if (dataType === '搜索结果账号' || dataType === '关键词搜索') return 'keyword_search'
  if (dataType === '账号公开信息') return 'account_profile'
  if (dataType === '作品/笔记作者' || dataType === '笔记详情') return 'item_detail'
  if (dataType === '账号作品所属账号') return 'account_posts'
  return 'comments'
}

function toUiDataType(dataType: string): DataType {
  if (dataType === 'keyword_search') return '搜索结果账号'
  if (dataType === 'account_profile') return '账号公开信息'
  if (dataType === 'item_detail') return '作品/笔记作者'
  if (dataType === 'account_posts') return '账号作品所属账号'
  return '评论用户'
}

function toUiTaskStatus(status: string): TaskStatus {
  if (status === 'success') return '成功'
  if (status === 'failed') return '失败'
  if (status === 'queued') return '已排队'
  if (status === 'waiting_confirmation') return '等待确认'
  if (status === 'draft') return '待人工确认'
  if (status === 'cancelled') return '失败'
  return '运行中'
}

function progressForTaskStatus(status: string) {
  if (status === 'success') return 100
  return 0
}

function numberFromJson(value: Record<string, unknown>) {
  const estimate = value.request_count_estimate
  return typeof estimate === 'number' ? estimate : 0
}

function positiveNumber(value: unknown) {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? value : undefined
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
