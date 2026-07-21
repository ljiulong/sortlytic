import {
  createCollectionTask,
  generateCollectionPlanFromText,
  type CollectionIntentV1,
  type CollectionPlanView,
} from './backend-api'
import { normalizeBackendProblem, type BackendProblem } from './backend-problem'

export type SuccessfulNaturalTaskAttempt = {
  taskId: string
  intent?: CollectionIntentV1 | null
  plan: CollectionPlanView
}

export async function createNaturalTaskAttempt(
  intentText: string,
  onTaskCreated?: (taskId: string) => void,
) {
  const task = await createCollectionTask({
    name: intentText.trim().slice(0, 42) || '自然语言采集任务',
    source_type: 'natural_language',
    platforms: [],
    data_types: [],
  }, intentText)
  onTaskCreated?.(task.id)
  return parseNaturalTaskAttempt(task.id, intentText)
}

export async function parseNaturalTaskAttempt(
  taskId: string,
  intentText: string,
): Promise<SuccessfulNaturalTaskAttempt> {
  const result = await generateCollectionPlanFromText({
    task_id: taskId,
    intent_text: intentText,
    provider_id: null,
    model_id: null,
  })
  if (!result.collection_plan) {
    throw new NaturalParseNeedsReviewError(
      taskId,
      result.issues,
      result.parsed_intent,
      result.issues.length > 0
        ? `解析完成，需要补充信息：${result.issues.join('；')}`
        : '解析完成，需要补充信息；请切换到表单修正任务',
    )
  }
  return {
    taskId,
    intent: result.parsed_intent,
    plan: result.collection_plan,
  }
}

export function describeNaturalParseFailure(error: unknown): {
  phase: 'needs_review' | 'failed'
  taskId?: string
  problem: BackendProblem
} {
  if (error instanceof NaturalParseNeedsReviewError) {
    return {
      phase: 'needs_review',
      taskId: error.taskId,
      problem: normalizeBackendProblem({
        code: 'VALIDATION_ERROR',
        stage: 'validating_intent',
        message: error.message,
        retryable: false,
        safe_details: {
          issues: error.issues,
          missing_fields: error.intent?.missing_fields ?? [],
          intent: error.intent ?? null,
        },
      }),
    }
  }
  return {
    phase: 'failed',
    problem: normalizeBackendProblem(error),
  }
}

class NaturalParseNeedsReviewError extends Error {
  readonly taskId: string
  readonly issues: string[]
  readonly intent?: CollectionIntentV1 | null

  constructor(
    taskId: string,
    issues: string[],
    intent: CollectionIntentV1 | null | undefined,
    message: string,
  ) {
    super(message)
    this.name = 'NaturalParseNeedsReviewError'
    this.taskId = taskId
    this.issues = issues
    this.intent = intent
  }
}
