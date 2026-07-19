import type { TFunction } from 'i18next'
import { Fragment, useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  backendErrorMessage,
  listTaskResults,
  type TaskResultRecordView,
  type TaskResultsPageView,
} from './backend-api'
import './TaskResultsPanel.css'

const pageSize = 50
const accountFieldGroups = [
  {
    key: 'profile',
    fields: [
      'secure_user_id', 'avatar_url', 'profile_url', 'bio', 'website_url',
      'verification_status', 'verification_reason', 'account_type', 'private_account',
      'language', 'country_region', 'profile_tags',
    ],
  },
  { key: 'demographics', fields: ['gender', 'age'] },
  {
    key: 'statistics',
    fields: [
      'followers_count', 'following_count', 'friends_count', 'posts_count',
      'likes_received_count', 'liked_content_count',
    ],
  },
  {
    key: 'activity',
    fields: [
      'account_created_at', 'last_posted_at', 'live_status', 'live_room_id',
      'username_modified_at', 'nickname_modified_at',
    ],
  },
  {
    key: 'platform_specific',
    fields: [
      'commerce_status', 'commerce_category', 'seller_status', 'organization_status',
      'comments_permission', 'duet_permission', 'stitch_permission', 'download_permission',
      'favorites_visibility', 'following_visibility', 'playlist_visibility', 'live_level',
      'live_badge',
    ],
  },
] as const
const catalogFields: ReadonlySet<string> = new Set(
  accountFieldGroups.flatMap((group) => group.fields),
)

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
  const { t: tCollection } = useTranslation('collection')
  const [offset, setOffset] = useState(0)
  const [retryVersion, setRetryVersion] = useState(0)
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set())
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
  const formatFieldValue = (key: string, value: unknown) => {
    if (value === null || value === undefined || value === '') return notCollected
    if (typeof value === 'boolean') {
      return t(value ? 'taskQueue.results.booleanTrue' : 'taskQueue.results.booleanFalse')
    }
    if (typeof value === 'number') return value.toLocaleString(numberLocale)
    if (Array.isArray(value)) {
      return value.length > 0 ? value.join(numberLocale.startsWith('zh') ? '、' : ', ') : notCollected
    }
    if (typeof value === 'string' && key.endsWith('_at')) return formatDate(value)
    if (typeof value === 'object') return JSON.stringify(value)
    return String(value)
  }
  const toggleDetails = (id: string) => setExpandedIds((current) => {
    const next = new Set(current)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    return next
  })

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
                  <th scope="col">{t('taskQueue.results.columns.details')}</th>
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
                  const accountName = item.username?.trim() || account || notCollected
                  const detailsId = `task-result-details-${item.id.replace(/[^a-zA-Z0-9_-]/g, '-')}`
                  const expanded = expandedIds.has(item.id)
                  return (
                    <Fragment key={item.id}>
                      <tr>
                        <td className="task-results__identity">
                          <strong>{accountName}</strong>
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
                        <td className="task-results__detail-action">
                          {state.page.selected_fields.length > 0 ? (
                            <button
                              aria-controls={detailsId}
                              aria-expanded={expanded}
                              className="task-results__detail-toggle"
                              type="button"
                              onClick={() => toggleDetails(item.id)}
                            >
                              {t(expanded
                                ? 'taskQueue.results.hideFields'
                                : 'taskQueue.results.showFields')}
                            </button>
                          ) : <span>{notConfigured}</span>}
                        </td>
                      </tr>
                      {expanded ? (
                        <tr className="task-results__detail-row">
                          <td colSpan={9}>
                            <AccountFieldDetails
                              accountName={accountName}
                              dateFormatter={formatDate}
                              fields={state.page.selected_fields}
                              formatValue={formatFieldValue}
                              item={item}
                              panelId={detailsId}
                              t={t}
                              tCollection={tCollection}
                            />
                          </td>
                        </tr>
                      ) : null}
                    </Fragment>
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

function AccountFieldDetails({
  accountName,
  dateFormatter,
  fields,
  formatValue,
  item,
  panelId,
  t,
  tCollection,
}: {
  accountName: string
  dateFormatter: (value: string) => string
  fields: string[]
  formatValue: (key: string, value: unknown) => string
  item: TaskResultRecordView
  panelId: string
  t: TFunction<'tasks'>
  tCollection: TFunction<'collection'>
}) {
  const groups = accountFieldGroups.map((group, index) => ({
    key: group.key,
    fields: fields.filter((field) => group.fields.some((candidate) => candidate === field)
      || (index === accountFieldGroups.length - 1 && !catalogFields.has(field))),
  })).filter((group) => group.fields.length > 0)

  return (
    <section
      aria-label={t('taskQueue.results.detailsAriaLabel', { account: accountName })}
      className="task-results__details"
      id={panelId}
    >
      {groups.map((group) => (
        <section className="task-results__detail-group" key={group.key}>
          <h5>{String(tCollection(`accountFieldGroups.${group.key}`))}</h5>
          <dl>
            {group.fields.map((field) => {
              const evidence = recordValue(item.field_evidence_json[field])
              const evidenceTime = textValue(evidence?.collected_at)
              return (
                <div key={field}>
                  <dt>
                    <strong>{String(tCollection(`accountFields.${field}.label`, { defaultValue: field }))}</strong>
                    <code>{field}</code>
                  </dt>
                  <dd>
                    <span>{formatValue(field, item.account_fields_json[field])}</span>
                    {evidence ? (
                      <small>
                        <span>{t('taskQueue.results.evidenceEndpoint')}：{textValue(evidence.endpoint_key) || notAvailable(t)}</span>
                        <span>{t('taskQueue.results.evidencePath')}：{textValue(evidence.raw_path) || notAvailable(t)}</span>
                        <span>{t('taskQueue.results.evidenceTime')}：{evidenceTime ? dateFormatter(evidenceTime) : notAvailable(t)}</span>
                      </small>
                    ) : <small>{t('taskQueue.results.noEvidence')}</small>}
                  </dd>
                </div>
              )
            })}
          </dl>
        </section>
      ))}
    </section>
  )
}

function recordValue(value: unknown) {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}

function textValue(value: unknown) {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function notAvailable(t: TFunction<'tasks'>) {
  return t('taskQueue.results.notCollected')
}

function platformName(platform: string) {
  if (platform === 'tiktok') return 'TikTok'
  if (platform === 'douyin') return '抖音'
  if (platform === 'xiaohongshu') return '小红书'
  return platform
}

export default TaskResultsPanel
