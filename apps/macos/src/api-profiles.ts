import { invoke } from '@tauri-apps/api/core'

export type ApiProfileKind = 'tikhub' | 'ai'
export type ApiProfileStatus = 'needs_rebind' | 'untested' | 'success' | 'failed'
export type AiProviderType =
  | 'openai'
  | 'anthropic'
  | 'gemini'
  | 'custom_openai_compatible'
  | 'ollama'
export type AiApiFormat =
  | 'openai_compatible'
  | 'anthropic_messages'
  | 'gemini'
  | 'ollama'

export type TikhubSafeTestSummary = {
  maskedAccount: string | null
  balance: number | null
  freeCredit: number | null
  availableCredit: number | null
  todayUsage: number | null
}

type ApiProfileViewBase = {
  id: string
  name: string
  revision: number
  status: ApiProfileStatus
  maskedKey: string | null
  hasCredential: boolean
  isActive: boolean
  lastTestedAt: string | null
  createdAt: string
  updatedAt: string
}

export type TikhubApiProfileView = ApiProfileViewBase & {
  kind: 'tikhub'
  baseUrl: string
  testSummary: TikhubSafeTestSummary | null
}

export type AiApiProfileView = ApiProfileViewBase & {
  kind: 'ai'
  providerType: AiProviderType
  apiFormat: AiApiFormat
  baseUrl: string
  defaultModelId: string
}

export type ApiProfileView = TikhubApiProfileView | AiApiProfileView

export type ApiProfileRegistryView = {
  activeProfileIds: Record<ApiProfileKind, string | null>
  tikhubProfiles: TikhubApiProfileView[]
  aiProfiles: AiApiProfileView[]
}

export type SaveTikhubApiProfileInput = {
  kind: 'tikhub'
  id?: string | null
  name: string
  baseUrl: string
  apiKey?: string | null
}

export type SaveAiApiProfileInput = {
  kind: 'ai'
  id?: string | null
  name: string
  providerType: AiProviderType
  apiFormat: AiApiFormat
  baseUrl: string
  defaultModelId: string
  apiKey?: string | null
}

export type SaveApiProfileInput = SaveTikhubApiProfileInput | SaveAiApiProfileInput

export type ApiProfileTestResult = {
  success: boolean
  message: string
  registry: ApiProfileRegistryView
}

export const API_PROFILE_ERROR_MESSAGE = 'API 配置操作失败，请检查配置后重试'

export async function getApiProfileRegistry(): Promise<ApiProfileRegistryView> {
  const response = await invokeApiProfileCommand('get_api_profile_registry', {
    rootPath: null,
  })
  return normalizeRegistry(response)
}

export async function saveApiProfile(
  input: SaveApiProfileInput,
): Promise<ApiProfileRegistryView> {
  const response = await invokeApiProfileCommand(
    'save_api_profile',
    { input, rootPath: null },
    input.apiKey ? [input.apiKey] : [],
  )
  return normalizeRegistry(response)
}

export async function testApiProfile(
  kind: ApiProfileKind,
  profileId: string,
): Promise<ApiProfileTestResult> {
  const response = asRecord(await invokeApiProfileCommand('test_api_profile', {
    kind,
    profileId,
    rootPath: null,
  }))
  return {
    success: readBoolean(response, 'success'),
    message: sanitizeSensitiveText(readString(response, 'message')),
    registry: normalizeRegistry(readValue(response, 'registry')),
  }
}

export async function activateApiProfile(
  kind: ApiProfileKind,
  profileId: string,
): Promise<ApiProfileRegistryView> {
  const response = await invokeApiProfileCommand('activate_api_profile', {
    kind,
    profileId,
    rootPath: null,
  })
  return normalizeRegistry(response)
}

export async function deleteApiProfile(
  kind: ApiProfileKind,
  profileId: string,
): Promise<ApiProfileRegistryView> {
  const response = await invokeApiProfileCommand('delete_api_profile', {
    kind,
    profileId,
    rootPath: null,
  })
  return normalizeRegistry(response)
}

async function invokeApiProfileCommand(
  command: string,
  args: Record<string, unknown>,
  secrets: string[] = [],
): Promise<unknown> {
  try {
    return await invoke<unknown>(command, args)
  } catch (error) {
    throw safeApiProfileError(error, secrets)
  }
}

function safeApiProfileError(error: unknown, secrets: string[]) {
  const rawMessage = typeof error === 'string'
    ? error
    : error && typeof error === 'object' && 'message' in error
      ? String((error as { message: unknown }).message)
      : API_PROFILE_ERROR_MESSAGE
  const message = sanitizeSensitiveText(rawMessage, secrets)
  return new Error(message || API_PROFILE_ERROR_MESSAGE)
}

function sanitizeSensitiveText(value: string, secrets: string[] = []) {
  let sanitized = value
  for (const secret of secrets) {
    if (secret) sanitized = sanitized.split(secret).join('[已隐藏]')
  }
  sanitized = sanitized
    .replace(
      /((?:api[ _-]?key|token|secret|密钥|令牌)\s*(?:为|是|[:=])?\s*)[^\s,，;；]+/giu,
      '$1[已隐藏]',
    )
    .replace(/\b(?:sk|tk|tikhub)[-_][A-Za-z0-9_./+=-]{8,}\b/giu, '[已隐藏]')
  return sanitized.slice(0, 500)
}

function normalizeRegistry(value: unknown): ApiProfileRegistryView {
  const registry = asRecord(value)
  const activeIds = asRecord(readValue(registry, 'activeProfileIds', 'active_profile_ids'))
  const activeProfileIds = {
    tikhub: readNullableString(activeIds, 'tikhub'),
    ai: readNullableString(activeIds, 'ai'),
  }
  return {
    activeProfileIds,
    tikhubProfiles: readCollection(registry, 'tikhubProfiles', 'tikhub_profiles')
      .map((profile) => normalizeTikhubProfile(profile, activeProfileIds.tikhub)),
    aiProfiles: readCollection(registry, 'aiProfiles', 'ai_profiles')
      .map((profile) => normalizeAiProfile(profile, activeProfileIds.ai)),
  }
}

function normalizeTikhubProfile(
  value: unknown,
  activeProfileId: string | null,
): TikhubApiProfileView {
  const profile = asRecord(value)
  const id = readString(profile, 'id')
  const testSummaryValue = readOptionalValue(profile, 'testSummary', 'test_summary')
  return {
    kind: 'tikhub',
    id,
    name: readString(profile, 'name'),
    baseUrl: readString(profile, 'baseUrl', 'base_url'),
    revision: readNumber(profile, 'revision'),
    status: readProfileStatus(profile),
    maskedKey: readMaskedKey(profile),
    hasCredential: readBoolean(profile, 'hasCredential', 'has_credential'),
    isActive: readOptionalBoolean(profile, 'isActive', 'is_active') ?? id === activeProfileId,
    lastTestedAt: readNullableString(profile, 'lastTestedAt', 'last_tested_at'),
    testSummary: testSummaryValue == null ? null : normalizeTestSummary(testSummaryValue),
    createdAt: readString(profile, 'createdAt', 'created_at'),
    updatedAt: readString(profile, 'updatedAt', 'updated_at'),
  }
}

function normalizeAiProfile(
  value: unknown,
  activeProfileId: string | null,
): AiApiProfileView {
  const profile = asRecord(value)
  const id = readString(profile, 'id')
  return {
    kind: 'ai',
    id,
    name: readString(profile, 'name'),
    providerType: readAiProviderType(profile),
    apiFormat: readAiApiFormat(profile),
    baseUrl: readString(profile, 'baseUrl', 'base_url'),
    defaultModelId: readString(profile, 'defaultModelId', 'default_model_id'),
    revision: readNumber(profile, 'revision'),
    status: readProfileStatus(profile),
    maskedKey: readMaskedKey(profile),
    hasCredential: readBoolean(profile, 'hasCredential', 'has_credential'),
    isActive: readOptionalBoolean(profile, 'isActive', 'is_active') ?? id === activeProfileId,
    lastTestedAt: readNullableString(profile, 'lastTestedAt', 'last_tested_at'),
    createdAt: readString(profile, 'createdAt', 'created_at'),
    updatedAt: readString(profile, 'updatedAt', 'updated_at'),
  }
}

function normalizeTestSummary(value: unknown): TikhubSafeTestSummary {
  const summary = asRecord(value)
  return {
    maskedAccount: readNullableString(summary, 'maskedAccount', 'masked_account'),
    balance: readNullableNumber(summary, 'balance'),
    freeCredit: readNullableNumber(summary, 'freeCredit', 'free_credit'),
    availableCredit: readNullableNumber(summary, 'availableCredit', 'available_credit'),
    todayUsage: readNullableNumber(summary, 'todayUsage', 'today_usage'),
  }
}

function readProfileStatus(value: Record<string, unknown>): ApiProfileStatus {
  const status = readString(value, 'status')
  if (['needs_rebind', 'untested', 'success', 'failed'].includes(status)) {
    return status as ApiProfileStatus
  }
  throw new Error(API_PROFILE_ERROR_MESSAGE)
}

function readAiProviderType(value: Record<string, unknown>): AiProviderType {
  const providerType = readString(value, 'providerType', 'provider_type')
  if (['openai', 'anthropic', 'gemini', 'custom_openai_compatible', 'ollama'].includes(providerType)) {
    return providerType as AiProviderType
  }
  throw new Error(API_PROFILE_ERROR_MESSAGE)
}

function readAiApiFormat(value: Record<string, unknown>): AiApiFormat {
  const apiFormat = readString(value, 'apiFormat', 'api_format')
  if (['openai_compatible', 'anthropic_messages', 'gemini', 'ollama'].includes(apiFormat)) {
    return apiFormat as AiApiFormat
  }
  throw new Error(API_PROFILE_ERROR_MESSAGE)
}

function readMaskedKey(value: Record<string, unknown>) {
  const maskedKey = readNullableString(value, 'maskedKey', 'masked_key')
  if (maskedKey == null) return null
  return /\[REDACTED\]|[*•]/u.test(maskedKey) ? maskedKey : '[REDACTED]'
}

function readCollection(
  value: Record<string, unknown>,
  key: string,
  fallbackKey: string,
) {
  const collection = readValue(value, key, fallbackKey)
  if (Array.isArray(collection)) return collection
  return Object.values(asRecord(collection))
}

function asRecord(value: unknown): Record<string, unknown> {
  if (value && typeof value === 'object' && !Array.isArray(value)) {
    return value as Record<string, unknown>
  }
  throw new Error(API_PROFILE_ERROR_MESSAGE)
}

function readValue(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  const result = value[key] ?? (fallbackKey ? value[fallbackKey] : undefined)
  if (result === undefined) throw new Error(API_PROFILE_ERROR_MESSAGE)
  return result
}

function readOptionalValue(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  return value[key] ?? (fallbackKey ? value[fallbackKey] : undefined)
}

function readString(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  const result = readValue(value, key, fallbackKey)
  if (typeof result !== 'string') throw new Error(API_PROFILE_ERROR_MESSAGE)
  return result
}

function readNullableString(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  const result = readOptionalValue(value, key, fallbackKey)
  if (result == null) return null
  if (typeof result !== 'string') throw new Error(API_PROFILE_ERROR_MESSAGE)
  return result
}

function readBoolean(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  const result = readValue(value, key, fallbackKey)
  if (typeof result !== 'boolean') throw new Error(API_PROFILE_ERROR_MESSAGE)
  return result
}

function readOptionalBoolean(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  const result = readOptionalValue(value, key, fallbackKey)
  if (result == null) return null
  if (typeof result !== 'boolean') throw new Error(API_PROFILE_ERROR_MESSAGE)
  return result
}

function readNumber(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  const result = readValue(value, key, fallbackKey)
  if (typeof result !== 'number' || !Number.isFinite(result)) {
    throw new Error(API_PROFILE_ERROR_MESSAGE)
  }
  return result
}

function readNullableNumber(value: Record<string, unknown>, key: string, fallbackKey?: string) {
  const result = readOptionalValue(value, key, fallbackKey)
  if (result == null) return null
  if (typeof result !== 'number' || !Number.isFinite(result)) {
    throw new Error(API_PROFILE_ERROR_MESSAGE)
  }
  return result
}
