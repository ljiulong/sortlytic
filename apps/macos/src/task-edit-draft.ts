import type {
  CollectionIntentV1,
  CollectionPlanView,
  CollectionTaskView,
  NaturalParseAttemptView,
} from './backend-api'

type GenderFilter = 'male' | 'female' | 'other'

export type TaskEditDraft = {
  taskId: string
  planId?: string
  name: string
  sourceType: string
  editorMode: 'natural_language' | 'form'
  originalIntent: string
  platform: string
  accountSource: string
  sourceInput: string
  queryLocale: string
  regionCode: string
  timeRangeDays: string
  recordLimit?: number
  budgetLimitMicros?: number
  selectedFields: string[]
  ageRange?: { min: number; max: number }
  genderFilter: GenderFilter[]
  schemaVersion?: number
  planJson?: Record<string, unknown>
  validationIssues: string[]
  missingFields: string[]
  copyOnSave: boolean
  parseProblem?: { kind?: 'needs_review'; code?: string; message?: string }
}

export function createTaskEditDraft(
  task: CollectionTaskView,
  plan?: CollectionPlanView,
  attempt?: NaturalParseAttemptView,
  intent?: CollectionIntentV1,
): TaskEditDraft {
  const planJson = plan?.plan_json
  const planPlatforms = stringArray(planJson?.platforms)
  const taskPlatforms = stringArray(task.platforms_json)
  const selectedFields = stringArray(planJson?.selected_fields)
  const planMissingFields = stringArray(planJson?.missing_fields)
  const intentMissingFields = intent?.missing_fields ?? []
  const currentAttempt = attempt && !attemptWasSuperseded(task, attempt) ? attempt : undefined
  const parseFailed = currentAttempt
    && ['failed', 'interrupted'].includes(currentAttempt.parse_status)
  const needsReview = currentAttempt?.parse_status === 'needs_review'
  const attemptIssues = needsReview
    ? stringArray(currentAttempt.error_safe_details_json.issues)
    : []
  const attemptMissingFields = needsReview
    ? stringArray(currentAttempt.error_safe_details_json.missing_fields)
    : []

  return {
    taskId: task.id,
    planId: plan?.id,
    name: task.name,
    sourceType: task.source_type,
    editorMode: task.source_type === 'natural_language' && (!plan || parseFailed)
      ? 'natural_language'
      : 'form',
    originalIntent: attempt?.intent_text ?? '',
    platform: intent?.platform ?? planPlatforms[0] ?? taskPlatforms[0] ?? '',
    accountSource: intent?.account_source ?? stringValue(planJson?.account_source) ?? '',
    sourceInput: intent?.source_input ?? sourceInputFromPlan(planJson) ?? '',
    queryLocale: intent?.query_locale ?? stringValue(planJson?.query_locale) ?? '',
    regionCode: intent?.region_code ?? regionFromPlan(planJson?.region),
    timeRangeDays: intent?.time_range_days?.toString() ?? timeRangeFromPlan(planJson?.time_range),
    recordLimit: intent?.record_limit ?? positiveInteger(planJson?.record_limit),
    budgetLimitMicros: intent?.budget_limit_micros ?? budgetFromPlan(planJson?.budget_limit),
    selectedFields: selectedFields.length > 0 ? selectedFields : intent?.selected_fields ?? [],
    ageRange: intent?.age_range ?? ageRangeFromPlan(planJson?.age_range),
    genderFilter: normalizeGenders(intent?.gender_filter ?? planJson?.gender_filter),
    schemaVersion: plan?.schema_version,
    planJson,
    validationIssues: [...new Set([
      ...stringArray(plan?.validation_errors_json),
      ...attemptIssues,
    ])],
    missingFields: [...new Set([
      ...planMissingFields,
      ...intentMissingFields,
      ...attemptMissingFields,
    ])],
    copyOnSave: ['success', 'partial_success'].includes(task.status),
    parseProblem: parseFailed || needsReview
      ? {
          ...(needsReview ? { kind: 'needs_review' as const } : {}),
          code: currentAttempt?.error_code ?? undefined,
          message: currentAttempt?.error_message ?? undefined,
        }
      : undefined,
  }
}

export function collectionIntentFromJson(value: unknown): CollectionIntentV1 | undefined {
  if (!isRecord(value)
    || value.schema_version !== 1
    || !nullablePlatform(value.platform)
    || !nullableString(value.account_source)
    || !nullableString(value.source_input)
    || !nullableString(value.query_locale)
    || !nullableString(value.region_code)
    || !stringArrayValue(value.selected_fields)
    || !nullableTimeRange(value.time_range_days)
    || !nullableAgeRange(value.age_range)
    || !nullableGenderFilter(value.gender_filter)
    || !nullablePositiveInteger(value.record_limit)
    || !nullablePositiveInteger(value.budget_limit_micros)
    || !stringArrayValue(value.missing_fields)
    || typeof value.confidence !== 'number'
    || !Number.isFinite(value.confidence)
    || value.confidence < 0
    || value.confidence > 1) {
    return undefined
  }
  return value as CollectionIntentV1
}

function attemptWasSuperseded(
  task: CollectionTaskView,
  attempt: NaturalParseAttemptView,
) {
  const taskUpdatedAt = Date.parse(task.updated_at)
  const attemptUpdatedAt = Date.parse(attempt.updated_at)
  return Number.isFinite(taskUpdatedAt)
    && Number.isFinite(attemptUpdatedAt)
    && taskUpdatedAt > attemptUpdatedAt
}

function sourceInputFromPlan(planJson?: Record<string, unknown>) {
  const direct = stringValue(planJson?.source_input)
  if (direct) return direct
  if (!Array.isArray(planJson?.steps)) return undefined
  for (const step of planJson.steps) {
    if (!isRecord(step) || !isRecord(step.params)) continue
    for (const key of ['source_input', 'keyword', 'account_id', 'item_id', 'share_text']) {
      const value = stringValue(step.params[key])
      if (value) return value
    }
  }
  return undefined
}

function regionFromPlan(value: unknown) {
  if (isRecord(value)) return stringValue(value.value) ?? ''
  return stringValue(value) ?? ''
}

function timeRangeFromPlan(value: unknown) {
  if (typeof value === 'number' && [1, 7, 30, 180].includes(value)) return String(value)
  return stringValue(value) ?? ''
}

function budgetFromPlan(value: unknown) {
  return isRecord(value) ? positiveInteger(value.amount_micros) : undefined
}

function ageRangeFromPlan(value: unknown) {
  if (!isRecord(value)) return undefined
  const min = nonNegativeInteger(value.min)
  const max = nonNegativeInteger(value.max)
  if (min === undefined || max === undefined || min > max || max > 130) return undefined
  return { min, max }
}

function normalizeGenders(value: unknown): GenderFilter[] {
  return stringArray(value).filter(
    (gender): gender is GenderFilter => ['male', 'female', 'other'].includes(gender),
  )
}

function stringArray(value: unknown) {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === 'string' && Boolean(item.trim()))
    : []
}

function stringValue(value: unknown) {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function nullableString(value: unknown) {
  return value === undefined || value === null || typeof value === 'string'
}

function nullablePlatform(value: unknown) {
  return value === undefined
    || value === null
    || (typeof value === 'string' && ['tiktok', 'douyin', 'xiaohongshu'].includes(value))
}

function stringArrayValue(value: unknown) {
  return Array.isArray(value) && value.every((item) => typeof item === 'string')
}

function nullableTimeRange(value: unknown) {
  return value === undefined
    || value === null
    || (typeof value === 'number' && [1, 7, 30, 180].includes(value))
}

function nullableAgeRange(value: unknown) {
  if (value === undefined || value === null) return true
  if (!isRecord(value)) return false
  const min = nonNegativeInteger(value.min)
  const max = nonNegativeInteger(value.max)
  return min !== undefined && max !== undefined && min <= max && max <= 130
}

function nullableGenderFilter(value: unknown) {
  return value === undefined
    || value === null
    || (Array.isArray(value)
      && value.every((gender) => typeof gender === 'string'
        && ['male', 'female', 'other'].includes(gender)))
}

function nullablePositiveInteger(value: unknown) {
  return value === undefined || value === null || positiveInteger(value) !== undefined
}

function positiveInteger(value: unknown) {
  return Number.isInteger(value) && Number(value) > 0 ? Number(value) : undefined
}

function nonNegativeInteger(value: unknown) {
  return Number.isInteger(value) && Number(value) >= 0 ? Number(value) : undefined
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}
