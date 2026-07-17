import { ChevronDown, RotateCcw } from 'lucide-react'
import { useId, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { listTaskLogs, type TaskLogView } from './backend-api'

type TaskRunLogPanelProps = {
  runId: string
  loadLogs?: (runId: string) => Promise<TaskLogView[]>
}

const logLevelTranslationKeys: Record<string, string> = {
  debug: 'taskQueue.logs.level.debug',
  error: 'taskQueue.logs.level.error',
  info: 'taskQueue.logs.level.info',
  warning: 'taskQueue.logs.level.warning',
}

function TaskRunLogPanel({ runId, loadLogs = listTaskLogs }: TaskRunLogPanelProps) {
  const { t, i18n } = useTranslation('tasks')
  const [isOpen, setIsOpen] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [loadError, setLoadError] = useState(false)
  const [logs, setLogs] = useState<TaskLogView[] | null>(null)
  const regionId = useId()
  const toggleId = `${regionId}-toggle`
  const numberLocale = i18n.resolvedLanguage ?? i18n.language
  const timeFormatter = new Intl.DateTimeFormat(numberLocale, {
    dateStyle: 'medium',
    timeStyle: 'medium',
  })

  const load = async () => {
    setIsLoading(true)
    setLoadError(false)
    try {
      setLogs(await loadLogs(runId))
    } catch {
      setLoadError(true)
    } finally {
      setIsLoading(false)
    }
  }

  const toggle = () => {
    const nextOpen = !isOpen
    setIsOpen(nextOpen)
    if (nextOpen && logs === null && !isLoading) {
      void load()
    }
  }

  return (
    <div aria-busy={isLoading} className="task-run-logs">
      <button
        aria-controls={regionId}
        aria-expanded={isOpen}
        className="task-run-logs__toggle"
        id={toggleId}
        type="button"
        onClick={toggle}
      >
        <span>{t(isOpen ? 'taskQueue.logs.hide' : 'taskQueue.logs.show')}</span>
        <ChevronDown aria-hidden="true" data-open={isOpen} size={15} />
      </button>

      {isOpen ? (
        <div
          aria-labelledby={toggleId}
          className="task-run-logs__body"
          id={regionId}
          role="region"
        >
          {isLoading ? (
            <p className="task-run-logs__state" role="status">{t('taskQueue.logs.loading')}</p>
          ) : loadError ? (
            <div className="task-run-logs__state task-run-logs__state--error" role="alert">
              <p>{t('taskQueue.logs.error')}</p>
              <button className="ghost-button" type="button" onClick={() => void load()}>
                <RotateCcw aria-hidden="true" size={14} />
                {t('taskQueue.logs.retry')}
              </button>
            </div>
          ) : logs?.length === 0 ? (
            <p className="task-run-logs__state">{t('taskQueue.logs.empty')}</p>
          ) : logs ? (
            <ol className="task-run-logs__list">
              {logs.map((log) => {
                const safeDetails = formatSafeDetails(log.safe_details_json)
                const levelKey = logLevelTranslationKeys[log.level]
                return (
                  <li data-level={log.level} key={log.id}>
                    <article className="task-run-logs__entry">
                      <header>
                        <strong>{log.stage}</strong>
                        <span>{levelKey ? t(levelKey) : log.level}</span>
                        <time dateTime={log.created_at}>
                          {timeFormatter.format(new Date(log.created_at))}
                        </time>
                      </header>
                      <p>{log.message}</p>
                      {safeDetails ? (
                        <pre aria-label={t('taskQueue.logs.safeDetails')}>{safeDetails}</pre>
                      ) : null}
                    </article>
                  </li>
                )
              })}
            </ol>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

function formatSafeDetails(value: unknown) {
  if (value === null || value === undefined || value === '') return null
  if (typeof value === 'object' && Object.keys(value).length === 0) return null
  if (typeof value === 'string') return value
  return JSON.stringify(value, null, 2)
}

export default TaskRunLogPanel
