import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  backendErrorMessage,
  listTaskResults,
  type TaskResultsPageView,
} from './backend-api'
import './TaskResultsPanel.css'

const pageSize = 50

type ResultsState =
  | { status: 'loading' }
  | { status: 'error'; reason: string }
  | { status: 'ready'; page: TaskResultsPageView }

type TaskResultsPanelProps = {
  taskId: string
  taskName: string
}

function TaskResultsPanel({ taskId, taskName }: TaskResultsPanelProps) {
  const { t, i18n } = useTranslation('tasks')
  const [offset, setOffset] = useState(0)
  const [retryVersion, setRetryVersion] = useState(0)
  const [state, setState] = useState<ResultsState>({ status: 'loading' })
  const numberLocale = i18n.resolvedLanguage ?? i18n.language
  const dateFormatter = useMemo(() => new Intl.DateTimeFormat(numberLocale, {
    dateStyle: 'medium',
    timeStyle: 'short',
  }), [numberLocale])

  useEffect(() => {
    let cancelled = false
    setState({ status: 'loading' })
    void listTaskResults(taskId, pageSize, offset)
      .then((page) => {
        if (!cancelled) setState({ status: 'ready', page })
      })
      .catch((error) => {
        if (!cancelled) {
          setState({ status: 'error', reason: backendErrorMessage(error) })
        }
      })
    return () => {
      cancelled = true
    }
  }, [offset, retryVersion, taskId])

  const notConfigured = t('taskQueue.results.notConfigured')
  const notCollected = t('taskQueue.results.notCollected')
  const formatNumber = (value?: number | null) => value == null
    ? notCollected
    : value.toLocaleString(numberLocale)
  const formatDate = (value: string) => {
    const date = new Date(value)
    return Number.isNaN(date.getTime()) ? value : dateFormatter.format(date)
  }

  return (
    <section
      aria-label={t('taskQueue.results.ariaLabel', { taskName })}
      className="task-results"
    >
      <header className="task-results__header">
        <div>
          <h4>{t('taskQueue.results.title')}</h4>
          {state.status === 'ready' ? (
            <p>{t('taskQueue.results.summary', { count: state.page.total_count })}</p>
          ) : null}
          {state.status === 'loading' ? (
            <p>{t('taskQueue.results.loading')}</p>
          ) : null}
        </div>
        {state.status === 'ready' && state.page.total_count > 0 ? (
          <span>{t('taskQueue.results.page', {
            page: Math.floor(state.page.offset / state.page.limit) + 1,
            count: state.page.total_count,
          })}</span>
        ) : null}
      </header>

      {state.status === 'loading' ? (
        <div className="task-results__state" role="status">
          <span className="task-results__loading-mark" aria-hidden="true" />
          <p>{t('taskQueue.results.loading')}</p>
        </div>
      ) : null}

      {state.status === 'error' ? (
        <div className="task-results__state task-results__state--error" role="alert">
          <p>{t('taskQueue.results.error', { reason: state.reason })}</p>
          <button
            aria-label={t('taskQueue.results.retry')}
            className="ghost-button"
            type="button"
            onClick={() => setRetryVersion((version) => version + 1)}
          >
            {t('taskQueue.results.retry')}
          </button>
        </div>
      ) : null}

      {state.status === 'ready' && state.page.items.length === 0 ? (
        <div className="task-results__state task-results__state--empty" role="status">
          <strong>{t('taskQueue.results.emptyTitle')}</strong>
          <p>{t('taskQueue.results.emptyDescription')}</p>
        </div>
      ) : null}

      {state.status === 'ready' && state.page.items.length > 0 ? (
        <>
          <div className="task-results__table-wrap">
            <table>
              <thead>
                <tr>
                  <th scope="col">{t('taskQueue.results.columns.identity')}</th>
                  <th scope="col">{t('taskQueue.results.columns.platform')}</th>
                  <th scope="col">{t('taskQueue.results.columns.region')}</th>
                  <th scope="col">{t('taskQueue.results.columns.genderAge')}</th>
                  <th className="task-results__number" scope="col">
                    {t('taskQueue.results.columns.followers')}
                  </th>
                  <th className="task-results__number" scope="col">
                    {t('taskQueue.results.columns.posts')}
                  </th>
                  <th scope="col">{t('taskQueue.results.columns.profile')}</th>
                  <th scope="col">{t('taskQueue.results.columns.collectedAt')}</th>
                </tr>
              </thead>
              <tbody>
                {state.page.items.map((item) => {
                  const account = item.account?.trim()
                  const gender = item.gender?.trim()
                    || (state.page.gender_filter_configured ? notCollected : notConfigured)
                  const age = item.age == null
                    ? (state.page.age_filter_configured ? notCollected : notConfigured)
                    : item.age.toString()
                  return (
                    <tr key={item.id}>
                      <td className="task-results__identity">
                        <strong>{item.username?.trim() || account || notCollected}</strong>
                        {account ? <span>@{account.replace(/^@/, '')}</span> : null}
                        {item.platform_user_id ? <small>{item.platform_user_id}</small> : null}
                      </td>
                      <td>{platformName(item.platform)}</td>
                      <td>{item.country_region?.trim() || notCollected}</td>
                      <td>
                        <div>{t('taskQueue.results.genderLabel')}：{gender}</div>
                        <div>{t('taskQueue.results.ageLabel')}：{age}</div>
                      </td>
                      <td className="task-results__number">{formatNumber(item.followers_count)}</td>
                      <td className="task-results__number">{formatNumber(item.posts_count)}</td>
                      <td className="task-results__profile">
                        {item.profile_text?.trim() || item.notes?.trim() || notCollected}
                      </td>
                      <td>
                        <time dateTime={item.collected_at}>{formatDate(item.collected_at)}</time>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
          <footer className="task-results__pager">
            <button
              aria-label={t('taskQueue.results.previous')}
              className="ghost-button"
              disabled={state.page.offset === 0}
              type="button"
              onClick={() => setOffset(Math.max(0, state.page.offset - state.page.limit))}
            >
              {t('taskQueue.results.previous')}
            </button>
            <button
              aria-label={t('taskQueue.results.next')}
              className="ghost-button"
              disabled={state.page.offset + state.page.items.length >= state.page.total_count}
              type="button"
              onClick={() => setOffset(state.page.offset + state.page.limit)}
            >
              {t('taskQueue.results.next')}
            </button>
          </footer>
        </>
      ) : null}
    </section>
  )
}

function platformName(platform: string) {
  if (platform === 'tiktok') return 'TikTok'
  if (platform === 'douyin') return '抖音'
  if (platform === 'xiaohongshu') return '小红书'
  return platform
}

export default TaskResultsPanel
