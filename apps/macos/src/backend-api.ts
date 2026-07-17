import { invoke } from '@tauri-apps/api/core'

export type BackendStatus = {
  service: string
  backend_version: string
  has_active_workspace: boolean
  uptime_ms: number
}

export type AppUpdateInfo = {
  version: string
  date?: string
  body?: string
}

type PendingAppUpdate = AppUpdateInfo & {
  downloadAndInstall: () => Promise<void>
}

let pendingAppUpdate: PendingAppUpdate | null = null

export type WorkspaceSummary = {
  id: string
  name: string
  root_path: string
  database_path: string
  schema_version: number
  created_at: string
  updated_at: string
  last_opened_at: string
}

export type TikhubPriceQuote = {
  endpoint: string
  request_per_day: number
  base_unit_price?: number | null
  total_price: number
  currency: string
  quote_json: Record<string, unknown>
}

export type CollectionTaskView = {
  id: string
  name: string
  source_type: string
  status: string
  platforms_json: unknown
  data_types_json: unknown
  created_at: string
  updated_at: string
  confirmed_at?: string | null
  completed_at?: string | null
  cancelled_at?: string | null
  cost_estimate_json: Record<string, unknown>
  actual_cost_json: Record<string, unknown>
}

export type UpdateCollectionTaskInput = {
  name?: string
  platforms?: string[]
  data_types?: string[]
}

export type CollectionPlanDraftView = {
  source: string
  schema_version: number
  plan_json: Record<string, unknown>
  validation_status: string
  validation_errors_json: unknown
  cost_estimate_json: Record<string, unknown>
}

export type CollectionPlanView = {
  id: string
  task_id: string
  source: string
  schema_version: number
  plan_json: Record<string, unknown>
  validation_status: string
  validation_errors_json: unknown
  cost_estimate_json: Record<string, unknown>
  confirmed_by_user: boolean
  created_at: string
  updated_at: string
}

export type TaskRunView = {
  id: string
  task_id: string
  status: string
  started_at: string
  ended_at?: string | null
  current_stage?: string | null
  error_code?: string | null
  error_message?: string | null
  retryable: boolean
  cost_actual_json: Record<string, unknown>
}

export type PromptTemplateView = {
  id: string
  template_key: string
  name: string
  task_type: string
  description?: string | null
  output_schema_id?: string | null
  is_builtin: boolean
  created_at: string
  updated_at: string
}

export type ExportJobView = {
  id: string
  report_id: string
  export_type: string
  status: string
  file_path?: string | null
  file_hash?: string | null
  file_size?: number | null
  error_code?: string | null
  error_message?: string | null
  created_at: string
  completed_at?: string | null
}

export type ReportView = {
  id: string
  task_id: string
  report_type: string
  title: string
  report_model_json: Record<string, unknown>
  status: string
  created_at: string
  updated_at: string
}

export type GenerateFormPlanInput = {
  platform: string
  data_type?: string
  data_types?: string[]
  params: Record<string, unknown>
  age_range?: { min: number; max: number } | null
  request_limit?: number
  record_limit?: number
  budget_limit_micros?: number
}

export type SavePlanInput = {
  task_id: string
  source: string
  plan_json: Record<string, unknown>
  validation_status: string
  validation_errors_json?: unknown
  cost_estimate_json?: Record<string, unknown>
}

export function getBackendStatus() {
  return invoke<BackendStatus>('get_backend_status')
}

export function ensureDefaultWorkspace() {
  return invoke<WorkspaceSummary>('ensure_default_workspace')
}

export function quoteTikhubConnectorPrice(endpoint: string, requestPerDay: number) {
  return invoke<TikhubPriceQuote>('quote_tikhub_connector_price', {
    endpoint,
    requestPerDay,
    rootPath: null,
  })
}

export function listTasks(status?: string) {
  return invoke<CollectionTaskView[]>('list_tasks', {
    status: status ?? null,
    rootPath: null,
  })
}

export function createCollectionTask(input: {
  name: string
  source_type: 'form' | 'natural_language'
  platforms: string[]
  data_types: string[]
}) {
  return invoke<CollectionTaskView>('create_collection_task', {
    input,
    rootPath: null,
  })
}

export function getLatestCollectionPlan(taskId: string) {
  return invoke<CollectionPlanView>('get_latest_collection_plan', {
    taskId,
    rootPath: null,
  })
}

export function updateCollectionTask(taskId: string, input: UpdateCollectionTaskInput) {
  return invoke<CollectionTaskView>('update_collection_task', {
    taskId,
    input,
    rootPath: null,
  })
}

export function cancelTask(taskId: string) {
  return invoke<CollectionTaskView>('cancel_task', {
    taskId,
    rootPath: null,
  })
}

export function deleteTask(taskId: string) {
  return invoke<void>('delete_task', {
    taskId,
    rootPath: null,
  })
}

export function generateFormCollectionPlan(request: GenerateFormPlanInput) {
  return invoke<CollectionPlanDraftView>('generate_form_collection_plan', { request })
}

export function saveCollectionPlan(input: SavePlanInput) {
  return invoke<CollectionPlanView>('save_collection_plan', {
    input,
    rootPath: null,
  })
}

export function generateCollectionPlanFromText(input: {
  task_id: string
  intent_text: string
  provider_id?: string | null
  model_id?: string | null
}) {
  return invoke<{
    collection_plan: CollectionPlanView
  }>('generate_collection_plan_from_text', {
    input,
    rootPath: null,
  })
}

export function confirmCollectionPlan(taskId: string, planId: string) {
  return invoke<CollectionTaskView>('confirm_collection_plan', {
    taskId,
    planId,
    rootPath: null,
  })
}

export function enqueueTask(taskId: string) {
  return invoke<TaskRunView>('enqueue_task', {
    taskId,
    rootPath: null,
  })
}

export function listPromptTemplates() {
  return invoke<PromptTemplateView[]>('list_prompt_templates', { rootPath: null })
}

export function buildReportModel(taskId: string, reportType = 'summary') {
  return invoke<ReportView>('build_report_model', {
    taskId,
    reportType,
    rootPath: null,
  })
}

export function createExportJob(reportId: string, exportType: 'xlsx' | 'pdf') {
  return invoke<ExportJobView>('create_export_job', {
    reportId,
    exportType,
    targetPath: null,
    rootPath: null,
  })
}

export async function checkForAppUpdate(): Promise<AppUpdateInfo | null> {
  const { check } = await import('@tauri-apps/plugin-updater')
  const update = await check()
  pendingAppUpdate = update
    ? {
        version: update.version,
        date: update.date,
        body: update.body,
        downloadAndInstall: () => update.downloadAndInstall(),
      }
    : null
  return pendingAppUpdate
    ? {
        version: pendingAppUpdate.version,
        date: pendingAppUpdate.date,
        body: pendingAppUpdate.body,
      }
    : null
}

export async function installAppUpdate() {
  if (!pendingAppUpdate) {
    throw new Error('请先检查更新')
  }
  await pendingAppUpdate.downloadAndInstall()
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}

export function backendErrorMessage(error: unknown) {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message)
  }
  return '后端调用失败'
}
