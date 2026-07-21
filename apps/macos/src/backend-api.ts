import { invoke } from '@tauri-apps/api/core'
import { normalizeBackendProblem } from './backend-problem'

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
  schema_version: number
  database_path?: string
  created_at?: string
  updated_at?: string
  last_opened_at?: string
}

export type WorkspaceHealthCheckView = {
  workspace_id: string
  database_quick_check: string
  foreign_keys_enabled: boolean
  journal_mode: string
  missing_directories: string[]
  database_writable: boolean
}

export type TikhubPriceQuote = {
  endpoint: string
  request_per_day: number
  base_unit_price?: number | null
  total_price: number
  currency: string
  quote_json: Record<string, unknown>
}

export type CostEstimateView = {
  request_count_estimate: number
  platform_count: number
  data_type_count: number
  requires_confirmation: boolean
  cost_estimate_json: Record<string, unknown>
}

export type CollectionTaskView = {
  id: string
  name: string
  source_type: string
  status: string
  platforms_json: unknown
  data_types_json: unknown
  account_source?: string | null
  selected_fields_json?: unknown
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

export type CollectionDataTypeCapabilityView = {
  platform: string
  data_type: string
  required_params: string[]
  optional_params: string[]
  pagination_mode: string
  region_filter: string
  time_range_filter: string
  provider_time_ranges: string[]
  max_page_size: number
  max_request_count: number
}

export type AccountSourceInputKind = 'keyword' | 'account' | 'item'
export type AccountFieldAvailability = 'direct' | 'enrichment' | 'conditional' | 'unsupported'
export type AccountFieldValueType = 'text' | 'integer' | 'boolean' | 'text_list' | 'timestamp'
export type FilterExecution = 'provider' | 'local' | 'unsupported'

export type AccountSourceCapabilityView = {
  key: string
  label?: string
  display_name: string
  description: string
  input_kind: AccountSourceInputKind
  endpoint_key: string
  pagination_mode: 'single' | 'cursor'
  region_filter?: FilterExecution
  time_range_filter?: FilterExecution
  time_ranges?: string[]
  max_page_size: number
  max_request_count: number
}

export type AccountFieldGroupView = {
  key: string
  display_name: string
}

export type AccountFieldCapabilityView = {
  key: string
  group: string
  label?: string
  display_name: string
  description: string
  value_type: AccountFieldValueType
  availability: AccountFieldAvailability
  default_selected: boolean
  required_operation_keys: string[]
  missing_reason?: string | null
  supported_platforms?: string[]
  covered_by_source_keys?: string[]
}

export type AccountCollectionCapabilityView = {
  catalog_version: number
  platform: string
  display_name: string
  account_sources: AccountSourceCapabilityView[]
  field_groups: AccountFieldGroupView[]
  fields: AccountFieldCapabilityView[]
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

export type ReviseCollectionTaskInput = {
  task_id: string
  name: string
  platforms: string[]
  data_types: string[]
  source: 'user_edited'
  plan_json: Record<string, unknown>
}

export type RevisedCollectionTaskView = {
  task: CollectionTaskView
  collection_plan: CollectionPlanView
  copied_from_task_id?: string | null
}

export type TaskRunView = {
  id: string
  task_id: string
  plan_id?: string | null
  attempt_number: number
  claimed_at?: string | null
  status: string
  started_at: string
  ended_at?: string | null
  current_stage?: string | null
  current_stage_code: string
  error_code?: string | null
  error_message?: string | null
  retryable: boolean
  cost_actual_json: Record<string, unknown>
}

export type NaturalParseAttemptView = {
  id: string
  task_id: string
  intent_text: string
  language?: string | null
  parse_status: 'running' | 'valid' | 'needs_review' | 'failed' | 'interrupted'
  parse_phase?: string | null
  ai_run_id?: string | null
  error_code?: string | null
  error_message?: string | null
  retryable?: boolean | null
  error_safe_details_json: Record<string, unknown>
  provider_id?: string | null
  model_id?: string | null
  prompt_version_id?: string | null
  created_at: string
  updated_at: string
}

export type AiRunView = {
  id: string
  task_id: string
  runtime_snapshot_id: string
  run_type: string
  input_record_set_id?: string | null
  input_summary?: string | null
  output_json?: Record<string, unknown> | null
  raw_output_path?: string | null
  schema_valid: boolean
  validation_status: string
  error_code?: string | null
  error_message?: string | null
  input_tokens?: number | null
  output_tokens?: number | null
  latency_ms?: number | null
  first_token_latency_ms?: number | null
  retry_count: number
  cost_estimate_json: Record<string, unknown>
  created_at: string
}

export type CollectionIntentV1 = {
  schema_version: 1
  platform?: 'tiktok' | 'douyin' | 'xiaohongshu' | null
  account_source?: string | null
  source_input?: string | null
  query_locale?: string | null
  region_code?: string | null
  selected_fields: string[]
  time_range_days?: 1 | 7 | 30 | 180 | null
  age_range?: { min: number; max: number } | null
  gender_filter?: Array<'male' | 'female' | 'other'> | null
  record_limit?: number | null
  budget_limit_micros?: number | null
  missing_fields: string[]
  confidence: number
}

export type TaskLogView = {
  id: string
  task_run_id: string
  stage: string
  stage_code: string
  level: string
  message: string
  message_code: string
  safe_details_json: unknown
  created_at: string
}

export type TaskRecordCountView = {
  task_id: string
  record_count: number
}

export type TaskResultRecordView = {
  id: string
  platform: string
  username?: string | null
  account?: string | null
  platform_user_id?: string | null
  profile_text?: string | null
  country_region?: string | null
  region_source?: string | null
  region_confidence?: string | null
  gender?: string | null
  age?: number | null
  followers_count?: number | null
  posts_count?: number | null
  last_posted_at?: string | null
  profile_url?: string | null
  data_source: string
  collected_at: string
  notes?: string | null
  account_fields_json: Record<string, unknown>
  field_evidence_json: Record<string, unknown>
}

export type TaskResultsPageView = {
  task_id: string
  task_run_id: string
  run_status: string
  age_filter_configured: boolean
  gender_filter_configured: boolean
  selected_fields: string[]
  total_count: number
  offset: number
  limit: number
  items: TaskResultRecordView[]
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

export type PromptVersionView = {
  id: string
  template_id: string
  version: number
  content: string
  change_note: string
  status: string
  created_at: string
  activated_at?: string | null
  rollback_from_version?: number | null
  content_hash: string
}

export type CreatePromptVersionInput = {
  template_id: string
  content: string
  change_note: string
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

export type GenerateAccountPlanInput = {
  platform: string
  account_source: string
  selected_fields: string[]
  enrichment_policy: 'auto_costed'
  params: Record<string, unknown>
  age_range?: { min: number; max: number } | null
  gender_filter?: Array<'male' | 'female' | 'other'> | null
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

export function getActiveWorkspace() {
  return invoke<WorkspaceSummary | null>('get_active_workspace')
}

export function runWorkspaceHealthCheck() {
  return invoke<WorkspaceHealthCheckView>('run_workspace_health_check', { rootPath: null })
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

export function listLatestTaskRuns() {
  return invoke<TaskRunView[]>('list_latest_task_runs', { rootPath: null })
}

export function listLatestTaskIntents() {
  return invoke<NaturalParseAttemptView[]>('list_latest_task_intents', { rootPath: null })
}

export function listTaskIntents(taskId: string) {
  return invoke<NaturalParseAttemptView[]>('list_task_intents', {
    taskId,
    rootPath: null,
  })
}

export function getAiRun(aiRunId: string) {
  return invoke<AiRunView>('get_ai_run', { aiRunId, rootPath: null })
}

export function listAiRuns(taskId: string, runType?: string) {
  return invoke<AiRunView[]>('list_ai_runs', {
    taskId,
    runType: runType ?? null,
    rootPath: null,
  })
}

export function listTaskLogs(taskRunId: string) {
  return invoke<TaskLogView[]>('list_task_logs', {
    taskRunId,
    rootPath: null,
  })
}

export function listTaskRecordCounts() {
  return invoke<TaskRecordCountView[]>('list_task_record_counts', { rootPath: null })
}

export function listTaskResults(taskId: string, limit = 100, offset = 0) {
  return invoke<TaskResultsPageView>('list_task_results', {
    taskId,
    limit,
    offset,
    rootPath: null,
  })
}

export function createCollectionTask(input: {
  name: string
  source_type: 'form' | 'natural_language'
  platforms: string[]
  data_types: string[]
}, intentText?: string) {
  return invoke<CollectionTaskView>('create_collection_task', {
    input,
    ...(intentText === undefined ? {} : { intentText }),
    rootPath: null,
  })
}

export function getLatestCollectionPlan(taskId: string) {
  return invoke<CollectionPlanView>('get_latest_collection_plan', {
    taskId,
    rootPath: null,
  })
}

export function getTask(taskId: string) {
  return invoke<CollectionTaskView>('get_task', {
    taskId,
    rootPath: null,
  })
}

export function estimateTaskCost(taskId: string) {
  return invoke<CostEstimateView>('estimate_task_cost', {
    taskId,
    planJson: null,
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

export function retryTask(taskId: string, stage?: string) {
  return invoke<TaskRunView>('retry_task', {
    taskId,
    stage: stage ?? null,
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

export function generateAccountCollectionPlan(request: GenerateAccountPlanInput) {
  return invoke<CollectionPlanDraftView>('generate_account_collection_plan', { request })
}

export function getAccountCollectionCapabilities(platform: string) {
  return invoke<AccountCollectionCapabilityView>('get_account_collection_capabilities', { platform })
}

export function listPlatformDataTypes(platform: string) {
  return invoke<CollectionDataTypeCapabilityView[]>('list_platform_data_types', { platform })
}

export function saveCollectionPlan(input: SavePlanInput) {
  return invoke<CollectionPlanView>('save_collection_plan', {
    input,
    rootPath: null,
  })
}

export function reviseCollectionTask(input: ReviseCollectionTaskInput) {
  return invoke<RevisedCollectionTaskView>('revise_collection_task', {
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
    parsed_intent?: CollectionIntentV1 | null
    issues: string[]
    collection_plan?: CollectionPlanView | null
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

export function listPromptVersions(templateId: string) {
  return invoke<PromptVersionView[]>('list_prompt_versions', {
    templateId,
    rootPath: null,
  })
}

export function createPromptVersion(input: CreatePromptVersionInput) {
  return invoke<PromptVersionView>('create_prompt_version', {
    input,
    rootPath: null,
  })
}

export function activatePromptVersion(promptVersionId: string) {
  return invoke<PromptVersionView>('activate_prompt_version', {
    promptVersionId,
    rootPath: null,
  })
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

function isTauriRuntime() {
  return typeof window !== 'undefined'
    && '__TAURI_INTERNALS__' in window
}

export async function getCurrentAppVersion(): Promise<string | null> {
  if (!isTauriRuntime()) return null

  const { getVersion } = await import('@tauri-apps/api/app')
  return getVersion()
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

export async function prepareAppUpdate(): Promise<void> {
  if (!pendingAppUpdate) {
    throw new Error('请先检查更新')
  }
  await pendingAppUpdate.downloadAndInstall()
}

export async function relaunchAfterAppUpdate(): Promise<void> {
  const { relaunch } = await import('@tauri-apps/plugin-process')
  await relaunch()
}

export function backendErrorMessage(error: unknown) {
  return normalizeBackendProblem(error).message
}
