import type { NaturalParseAttemptView } from './backend-api'
import { normalizeBackendProblem, type BackendProblem } from './backend-problem'

export type NaturalParsePhase =
  | 'idle'
  | 'preparing'
  | 'requesting_ai'
  | 'validating_intent'
  | 'building_plan'
  | 'needs_review'
  | 'success'
  | 'failed'

export type NaturalParseState = {
  phase: NaturalParsePhase
  taskId?: string
  attemptId?: string
  intentText: string
  startedAt?: string
  finishedAt?: string
  providerId?: string
  modelId?: string
  promptVersionId?: string
  problem?: BackendProblem
  draftPreserved: boolean
}

export function createIdleNaturalParseState(): NaturalParseState {
  return {
    phase: 'idle',
    intentText: '',
    draftPreserved: true,
  }
}

export function createPreparingNaturalParseState(intentText: string): NaturalParseState {
  return {
    phase: 'preparing',
    intentText: intentText.trim(),
    startedAt: new Date().toISOString(),
    draftPreserved: true,
  }
}

export function naturalParseStateFromAttempt(
  attempt: NaturalParseAttemptView,
): NaturalParseState {
  const phase = phaseFromAttempt(attempt)
  const terminal = ['needs_review', 'success', 'failed'].includes(phase)
  return {
    phase,
    taskId: attempt.task_id,
    attemptId: attempt.id,
    intentText: attempt.intent_text,
    startedAt: attempt.created_at,
    finishedAt: terminal ? attempt.updated_at : undefined,
    providerId: attempt.provider_id ?? undefined,
    modelId: attempt.model_id ?? undefined,
    promptVersionId: attempt.prompt_version_id ?? undefined,
    problem: problemFromAttempt(attempt, phase),
    draftPreserved: true,
  }
}

export function resolveNaturalParseState(
  localState: NaturalParseState,
  attempts: NaturalParseAttemptView[],
): NaturalParseState {
  const latestAttempts = [...attempts].sort(
    (left, right) => Date.parse(right.updated_at) - Date.parse(left.updated_at),
  )
  if (localState.phase === 'idle') {
    return latestAttempts[0]
      ? naturalParseStateFromAttempt(latestAttempts[0])
      : localState
  }
  if (!localState.taskId) return localState
  const persisted = latestAttempts.find((attempt) => attempt.task_id === localState.taskId)
  if (!persisted) return localState
  const persistedUpdatedAt = Date.parse(persisted.updated_at)
  const localStartedAt = Date.parse(localState.startedAt ?? '')
  return !Number.isFinite(localStartedAt) || persistedUpdatedAt >= localStartedAt
    ? naturalParseStateFromAttempt(persisted)
    : localState
}

function phaseFromAttempt(attempt: NaturalParseAttemptView): NaturalParsePhase {
  if (attempt.parse_status === 'valid') return 'success'
  if (attempt.parse_status === 'needs_review') return 'needs_review'
  if (['failed', 'interrupted'].includes(attempt.parse_status)) return 'failed'
  if (attempt.parse_status !== 'running') return 'failed'
  if (isRunningPhase(attempt.parse_phase)) return attempt.parse_phase
  return 'preparing'
}

function isRunningPhase(phase: string | null | undefined): phase is Extract<
  NaturalParsePhase,
  'preparing' | 'requesting_ai' | 'validating_intent' | 'building_plan'
> {
  return ['preparing', 'requesting_ai', 'validating_intent', 'building_plan'].includes(phase ?? '')
}

function problemFromAttempt(
  attempt: NaturalParseAttemptView,
  phase: NaturalParsePhase,
): BackendProblem | undefined {
  if (phase === 'needs_review') {
    return normalizeBackendProblem({
      code: attempt.error_code ?? 'VALIDATION_ERROR',
      stage: attempt.parse_phase ?? 'validating_intent',
      message: attempt.error_message ?? '解析完成，需要补充信息后才能生成安全计划',
      retryable: attempt.retryable ?? false,
      safe_details: attempt.error_safe_details_json,
    })
  }
  if (phase !== 'failed') return undefined
  const interrupted = attempt.parse_status === 'interrupted'
  return normalizeBackendProblem({
    code: attempt.error_code ?? (interrupted ? 'MODEL_REQUEST_INTERRUPTED' : 'UNKNOWN_ERROR'),
    stage: attempt.parse_phase ?? 'unknown',
    message: attempt.error_message
      ?? (interrupted ? '上次自然语言解析被应用退出或重启中断，请重新解析' : '未能读取完整错误详情'),
    retryable: attempt.retryable ?? interrupted,
    safe_details: attempt.error_safe_details_json,
  })
}
