import { normalizeBackendProblem } from './backend-problem'

export function isMissingCollectionPlanProblem(error: unknown) {
  const problem = normalizeBackendProblem(error)
  return problem.code === 'VALIDATION_ERROR'
    && problem.stage === 'validation'
    && problem.safeDetails.reason === 'no_plan'
    && problem.safeDetails.entity === 'collection_plan'
}
