import { useTranslation } from 'react-i18next'
import TaskProblemPanel from './TaskProblemPanel'
import TaskRunLogPanel from './TaskRunLogPanel'
import type { TaskRemediationAction } from './task-remediation'
import type { WorkbenchRuntimeData } from './use-workbench-backend'

type TaskRun = NonNullable<WorkbenchRuntimeData['tasks'][number]['latestRun']>

export default function TaskRunDetails({
  run,
  isBusy = false,
  onProblemAction,
}: {
  run: TaskRun
  isBusy?: boolean
  onProblemAction: (action: TaskRemediationAction) => void
}) {
  const { t, i18n } = useTranslation('tasks')
  const locale = i18n.resolvedLanguage ?? i18n.language
  const showRawDiagnostics = locale.toLowerCase().startsWith('zh')
  const timeFormatter = new Intl.DateTimeFormat(locale, {
    dateStyle: 'medium',
    timeStyle: 'medium',
  })
  const stageFallback = showRawDiagnostics && run.currentStage
    ? run.currentStage
    : String(t('taskQueue.diagnostics.unknownStage', {
        code: run.currentStageCode ?? 'UNKNOWN_STAGE',
      }))
  const stage = run.currentStageCode && run.currentStageCode !== 'UNKNOWN_STAGE'
    ? String(t(`taskQueue.diagnostics.stage.${run.currentStageCode}`, {
        defaultValue: stageFallback,
      }))
    : stageFallback
  const errorFallback = String(t('taskQueue.diagnostics.unknownError', {
    code: run.errorCode ?? 'UNKNOWN_ERROR',
  }))
  const errorMessage = showRawDiagnostics
    ? run.errorMessage
    : run.errorCode
      ? String(t(`taskQueue.diagnostics.error.${run.errorCode}`, { defaultValue: errorFallback }))
      : errorFallback

  return (
    <section aria-label={t('taskQueue.runDetails')} className="task-card__run-details">
      <header className="task-card__run-heading">
        <h4>{t('taskQueue.runDetails')}</h4>
        <span>{t('taskQueue.attempt', { count: run.attemptNumber })}</span>
      </header>
      <dl className="task-card__run-facts">
        <div><dt>{t('taskQueue.currentStage')}</dt><dd>{stage}</dd></div>
        <div>
          <dt>{t('taskQueue.startedAt')}</dt>
          <dd><time dateTime={run.startedAt}>{timeFormatter.format(new Date(run.startedAt))}</time></dd>
        </div>
        <div>
          <dt>{t('taskQueue.endedAt')}</dt>
          <dd>{run.endedAt
            ? <time dateTime={run.endedAt}>{timeFormatter.format(new Date(run.endedAt))}</time>
            : t('taskQueue.inProgress')}</dd>
        </div>
        <div>
          <dt>{t('taskQueue.retryable')}</dt>
          <dd>{t(run.retryable ? 'taskQueue.retryableYes' : 'taskQueue.retryableNo')}</dd>
        </div>
      </dl>
      {run.errorMessage ? (
        <TaskProblemPanel
          kind="run"
          code={run.errorCode}
          message={errorMessage ?? errorFallback}
          retryable={run.retryable}
          attemptedAt={run.endedAt ?? run.startedAt}
          safeDetails={run.safeDetails}
          isBusy={isBusy}
          onAction={onProblemAction}
        />
      ) : null}
      <TaskRunLogPanel key={run.id} runId={run.id} />
    </section>
  )
}
