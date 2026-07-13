import { invoke } from '@tauri-apps/api/core'

export type BackendStatus = {
  service: string
  backend_version: string
  has_active_workspace: boolean
  uptime_ms: number
}

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

export type SecretRefView = {
  id: string
  provider_type: string
  provider_id: string
  alias?: string | null
  masked_hint: string
  last_test_status?: string | null
}

export type SecretConnectionTestResult = {
  secret_ref_id: string
  success: boolean
  message: string
  tested_at: string
}

export type TikhubConnectionTestResult = {
  success: boolean
  base_url: string
  masked_email?: string | null
  balance?: number | null
  free_credit?: number | null
  email_verified?: boolean | null
  api_key_status?: number | null
  daily_usage_json: Record<string, unknown>
  message: string
}

export type TikhubConnectorInput = {
  secret_ref_id?: string | null
  base_url: string
  enabled: boolean
}

export type TikhubConnectorView = {
  id: string
  workspace_id: string
  secret_ref_id?: string | null
  base_url: string
  enabled: boolean
  config_version: number
  last_tested_at?: string | null
  last_test_status?: string | null
  created_at: string
  updated_at: string
}

export type ModelProviderView = {
  id: string
  provider_id: string
  display_name: string
  enabled: boolean
  auth_type: string
  secret_ref_id?: string | null
  base_url?: string | null
  api_format: string
  region?: string | null
  default_model_id?: string | null
  cost_policy_json: Record<string, unknown>
  rate_limit_policy_json: Record<string, unknown>
  health_check_json: Record<string, unknown>
  created_at: string
  updated_at: string
}

export type ModelProviderInput = {
  provider_id: string
  display_name: string
  enabled?: boolean
  auth_type: 'api_key' | 'none'
  secret_ref_id?: string | null
  base_url?: string | null
  api_format: 'openai_compatible' | 'anthropic_messages' | 'gemini' | 'ollama'
  region?: string | null
  cost_policy_json?: Record<string, unknown> | null
  rate_limit_policy_json?: Record<string, unknown> | null
  health_check_json?: Record<string, unknown> | null
}

export type ModelProfileInput = {
  provider_id: string
  model_id: string
  display_name: string
  capabilities_json?: Record<string, unknown> | null
  context_window?: number | null
  supports_structured_output?: boolean
  supports_streaming?: boolean
  supports_tools?: boolean
  supports_vision?: boolean
  enabled?: boolean
}

export type ModelProfileView = {
  id: string
  provider_id: string
  model_id: string
  display_name: string
  capabilities_json: Record<string, unknown>
  context_window?: number | null
  supports_structured_output: boolean
  supports_streaming: boolean
  supports_tools: boolean
  supports_vision: boolean
  enabled: boolean
  created_at: string
  updated_at: string
}

export type ProviderTestResult = {
  provider_id: string
  success: boolean
  message: string
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
  data_type: string
  params: Record<string, unknown>
  request_limit?: number
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

export function listSecretRefs(providerType?: string) {
  return invoke<SecretRefView[]>('list_secret_refs', {
    providerType: providerType ?? null,
    rootPath: null,
  })
}

export function saveSecret(input: {
  provider_type: string
  provider_id: string
  secret: string
  alias?: string | null
}) {
  return invoke<SecretRefView>('save_secret', {
    providerType: input.provider_type,
    providerId: input.provider_id,
    secret: input.secret,
    alias: input.alias ?? null,
    rootPath: null,
  })
}

export function updateSecret(secretRefId: string, secret: string) {
  return invoke<SecretRefView>('update_secret', {
    secretRefId,
    secret,
    rootPath: null,
  })
}

export function testSecretConnection(secretRefId: string) {
  return invoke<SecretConnectionTestResult>('test_secret_connection', {
    secretRefId,
    rootPath: null,
  })
}

export function testTikhubConnection(secretRefId: string, baseUrl: string) {
  return invoke<TikhubConnectionTestResult>('test_tikhub_connection', {
    secretRefId,
    baseUrl,
    rootPath: null,
  })
}

export function getTikhubConnector() {
  return invoke<TikhubConnectorView | null>('get_tikhub_connector', {
    rootPath: null,
  })
}

export function saveTikhubConnector(input: TikhubConnectorInput) {
  return invoke<TikhubConnectorView>('save_tikhub_connector', {
    input,
    rootPath: null,
  })
}

export function testTikhubConnector() {
  return invoke<TikhubConnectionTestResult>('test_tikhub_connector', {
    rootPath: null,
  })
}

export function listModelProviders(enabled?: boolean) {
  return invoke<ModelProviderView[]>('list_model_providers', {
    enabled: enabled ?? null,
    rootPath: null,
  })
}

export function createModelProvider(input: ModelProviderInput) {
  return invoke<ModelProviderView>('create_model_provider', {
    input,
    rootPath: null,
  })
}

export function updateModelProvider(providerId: string, input: ModelProviderInput) {
  return invoke<ModelProviderView>('update_model_provider', {
    providerId,
    input,
    rootPath: null,
  })
}

export function upsertModelProfile(input: ModelProfileInput) {
  return invoke<ModelProfileView>('upsert_model_profile', {
    input,
    rootPath: null,
  })
}

export function setDefaultModel(providerId: string, modelId: string) {
  return invoke<boolean>('set_default_model', {
    providerId,
    modelId,
    rootPath: null,
  })
}

export function testModelProvider(providerId: string, modelId?: string) {
  return invoke<ProviderTestResult>('test_model_provider', {
    providerId,
    modelId: modelId ?? null,
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

export function backendErrorMessage(error: unknown) {
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message)
  }
  return '后端调用失败'
}
