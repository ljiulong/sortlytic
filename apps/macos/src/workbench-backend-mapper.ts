import type {
  ApiProfileRegistryView,
  ApiProfileStatus,
} from './api-profiles'
import type {
  CollectionTaskView,
  NaturalParseAttemptView,
  TaskRecordCountView,
  TaskRunView,
  WorkspaceSummary,
} from './backend-api'
import {
  mapTaskRow,
  numberFromJson,
} from './workbench-task-mapper'
import type {
  ConnectionIcon,
  Platform,
  SocialRecord,
  TaskStatus,
  Tone,
} from './workbench-data'

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
    id: string
    name: string
    platform: Platform
    status: TaskStatus
    source: string
    sourceType?: 'natural_language' | 'form'
    progress: number
    records: number
    cost: string
    requestCount?: number
    dataTypeCode?: string
    latestRun?: {
      id: string
      attemptNumber: number
      status: string
      currentStage?: string | null
      currentStageCode?: string
      errorCode?: string | null
      errorMessage?: string | null
      safeDetails?: Record<string, unknown>
      retryable: boolean
      startedAt: string
      endedAt?: string | null
    }
    naturalParseAttempt?: NaturalParseAttemptView
  }>
  records: SocialRecord[]
  promptRuns: Array<{ name: string; status: '通过' | '失败'; provider: string; diff: string }>
}

export type BackendWorkbenchData = WorkbenchRuntimeData & {
  latestTaskId?: string
  naturalParseAttempts: NaturalParseAttemptView[]
  currentNaturalParseAttempts: NaturalParseAttemptView[]
  runtimeMode: 'backend' | 'unavailable' | 'loading' | 'error'
}

export function mapBackendData(
  workspace: WorkspaceSummary,
  tasks: CollectionTaskView[],
  registry: ApiProfileRegistryView | null,
  uptimeMs: number,
  latestRuns: TaskRunView[] = [],
  recordCounts: TaskRecordCountView[] = [],
  naturalParseAttempts: NaturalParseAttemptView[] = [],
): BackendWorkbenchData {
  const latestTaskId = tasks[0]?.id
  const pendingCount = tasks.filter((task) => task.status === 'waiting_confirmation').length
  const queuedCount = tasks.filter((task) => task.status === 'queued').length
  const requestCount = tasks.reduce((total, task) => total + numberFromJson(task.cost_estimate_json), 0)

  const latestRunByTask = new Map(latestRuns.map((run) => [run.task_id, run]))
  const recordCountByTask = new Map(recordCounts.map((count) => [count.task_id, count.record_count]))
  const taskById = new Map(tasks.map((task) => [task.id, task]))
  const currentNaturalParseAttempts = naturalParseAttempts.filter((attempt) => {
    const task = taskById.get(attempt.task_id)
    return task !== undefined && currentNaturalParseAttempt(task, attempt) !== undefined
  })
  const parseAttemptByTask = new Map(
    currentNaturalParseAttempts.map((attempt) => [attempt.task_id, attempt]),
  )
  const taskRecordCounts = tasks.map((task) => Math.max(0, recordCountByTask.get(task.id) ?? 0))
  const storedRecordCount = taskRecordCounts.reduce((total, count) => total + count, 0)
  const tasksWithRecords = taskRecordCounts.filter((count) => count > 0).length

  return {
    workspace: {
      name: workspace.name,
      storage: shortPath(workspace.root_path),
      lastBackup: '未创建备份',
      health: `可用，运行 ${Math.max(1, Math.round(uptimeMs / 1000))} 秒`,
    },
    connections: buildConnections(registry),
    metrics: [
      { label: '本地任务', value: String(tasks.length), delta: `${pendingCount} 个待确认`, tone: 'info' },
      {
        label: '入库记录',
        value: String(storedRecordCount),
        delta: storedRecordCount > 0 ? `records_available:${tasksWithRecords}` : '暂无真实记录',
        tone: storedRecordCount > 0 ? 'success' : 'info',
      },
      { label: '预计请求', value: String(requestCount), delta: `${queuedCount} 个已入队`, tone: 'warning' },
      ...(storedRecordCount === 0
        ? [{ label: '证据覆盖', value: '未计算', delta: '暂无真实记录', tone: 'info' as const }]
        : []),
    ],
    tasks: tasks.map((task) => {
      const row = mapTaskRow(task)
      const run = latestRunByTask.get(task.id)
      const naturalParseAttempt = currentNaturalParseAttempt(
        task,
        parseAttemptByTask.get(task.id),
      )
      return {
        ...row,
        records: recordCountByTask.get(task.id) ?? 0,
        naturalParseAttempt,
        latestRun: run
          ? {
              id: run.id,
              attemptNumber: run.attempt_number,
              status: run.status,
              currentStage: run.current_stage,
              currentStageCode: run.current_stage_code,
              errorCode: run.error_code,
              errorMessage: run.error_message,
              safeDetails: run.error_safe_details_json,
              retryable: run.retryable,
              startedAt: run.started_at,
              endedAt: run.ended_at,
            }
          : undefined,
      }
    }),
    records: [],
    promptRuns: [],
    naturalParseAttempts,
    currentNaturalParseAttempts,
    latestTaskId,
    runtimeMode: 'backend',
  }
}

function currentNaturalParseAttempt(
  task: CollectionTaskView,
  attempt: NaturalParseAttemptView | undefined,
) {
  if (!attempt || !['failed', 'interrupted', 'needs_review'].includes(attempt.parse_status)) {
    return attempt
  }
  const taskUpdatedAt = Date.parse(task.updated_at)
  const attemptUpdatedAt = Date.parse(attempt.updated_at)
  if (Number.isFinite(taskUpdatedAt)
    && Number.isFinite(attemptUpdatedAt)
    && taskUpdatedAt > attemptUpdatedAt) {
    return undefined
  }
  return attempt
}

function buildConnections(registry: ApiProfileRegistryView | null) {
  if (!registry) {
    return [
      unavailableConnection('TikHub', 'REST API'),
      unavailableConnection('AI API', '结构化输出'),
      webhookConnection(),
    ] satisfies WorkbenchRuntimeData['connections']
  }

  const activeTikhub = registry.tikhubProfiles.find(
    (profile) => profile.id === registry.activeProfileIds.tikhub,
  )
  const activeAi = registry.aiProfiles.find(
    (profile) => profile.id === registry.activeProfileIds.ai,
  )
  const tikhubFallbackStatus = registry.tikhubProfiles.some(
    (profile) => profile.status === 'needs_rebind',
  )
    ? '需重新绑定'
    : registry.tikhubProfiles.length > 0 ? '待选择' : '未配置'
  const aiFallbackStatus = registry.aiProfiles.some(
    (profile) => profile.status === 'needs_rebind',
  )
    ? '需重新绑定'
    : registry.aiProfiles.length > 0 ? '待选择' : '未配置'

  return [
    {
      name: 'TikHub',
      detail: 'REST API',
      status: activeTikhub ? profileStatusLabel(activeTikhub.status) : tikhubFallbackStatus,
      tone: activeTikhub ? profileStatusTone(activeTikhub.status) : 'warning',
      icon: 'key',
      meta: activeTikhub
        ? [activeTikhub.baseUrl, activeTikhub.maskedKey].filter(Boolean).join(' · ')
        : '在设置中选择当前配置',
    },
    {
      name: 'AI API',
      detail: '结构化输出',
      status: activeAi ? profileStatusLabel(activeAi.status) : aiFallbackStatus,
      tone: activeAi ? profileStatusTone(activeAi.status) : 'info',
      icon: 'bot',
      meta: activeAi
        ? `${activeAi.name} · ${activeAi.defaultModelId}`
        : '请在设置中配置并测试真实 AI 模型',
    },
    webhookConnection(),
  ] satisfies WorkbenchRuntimeData['connections']
}

function unavailableConnection(name: string, detail: string) {
  return {
    name,
    detail,
    status: '配置不可用',
    tone: 'danger',
    icon: name === 'TikHub' ? 'key' : 'bot',
    meta: 'API 配置文件无法读取，历史数据仍可浏览',
  } satisfies WorkbenchRuntimeData['connections'][number]
}

function webhookConnection() {
  return {
    name: 'Webhook',
    detail: 'n8n 轻集成',
    status: '未启用',
    tone: 'warning',
    icon: 'share',
    meta: '仅发送摘要',
  } satisfies WorkbenchRuntimeData['connections'][number]
}

function profileStatusLabel(status: ApiProfileStatus) {
  if (status === 'success') return '已验证'
  if (status === 'failed') return '测试失败'
  if (status === 'needs_rebind') return '需重新绑定'
  return '待测试'
}

function profileStatusTone(status: ApiProfileStatus): Tone {
  if (status === 'success') return 'success'
  if (status === 'failed' || status === 'needs_rebind') return 'danger'
  return 'info'
}

function shortPath(path: string) {
  const parts = path.split('/').filter(Boolean)
  return parts.slice(-2).join('/') || path
}
