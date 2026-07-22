import type { BackendWorkbenchData } from './workbench-backend-mapper'
import type { NaturalParseState } from './natural-parse-state'

const naturalParseRefetchIntervalMs = 1_000
const activeTaskRefetchIntervalMs = 2_000

export function workbenchRefetchInterval(
  data: BackendWorkbenchData | undefined,
  localPhase: NaturalParseState['phase'],
): number | false {
  if (naturalParseInProgress(localPhase)
    || data?.currentNaturalParseAttempts.some((attempt) => attempt.parse_status === 'running')
    || data?.naturalParseAttempts.some((attempt) => attempt.parse_status === 'running')) {
    return naturalParseRefetchIntervalMs
  }
  return data?.tasks.some((task) => ['已排队', '运行中'].includes(task.status))
    ? activeTaskRefetchIntervalMs
    : false
}

function naturalParseInProgress(phase: NaturalParseState['phase']) {
  return ['preparing', 'requesting_ai', 'validating_intent', 'building_plan'].includes(phase)
}
