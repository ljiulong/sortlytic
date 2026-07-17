import type { TFunction } from 'i18next'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { Bot, Database, KeyRound, RefreshCcw, Share2 } from 'lucide-react'
import { StatusPill } from './CollectionBuilder'
import type { WorkbenchRuntimeData } from './use-workbench-backend'
import type { SocialRecord, Tone } from './workbench-data'
import './i18n'
import './Dashboard.css'

type DashboardT = TFunction<'dashboard'>

type DashboardProps = {
  workspace: WorkbenchRuntimeData['workspace']
  connections: WorkbenchRuntimeData['connections']
  metrics: WorkbenchRuntimeData['metrics']
  records: SocialRecord[]
  promptRuns: WorkbenchRuntimeData['promptRuns']
  isBusy: boolean
  selectedRecordId: string
  onCreateTask: () => void
  onRefresh: () => void
  onSelectRecord: (recordId: string) => void
}

const connectionIcons = {
  key: KeyRound,
  bot: Bot,
  share: Share2,
}

function Dashboard({
  workspace,
  connections,
  metrics,
  records,
  promptRuns,
  isBusy,
  selectedRecordId,
  onCreateTask,
  onRefresh,
  onSelectRecord,
}: DashboardProps) {
  const { t } = useTranslation('dashboard')
  const hasInspector = records.length > 0 || promptRuns.length > 0

  return (
    <div className="dashboard">
      <OverviewSummary metrics={metrics} health={workspace.health} />
      <div className="dashboard__content" data-with-inspector={hasInspector}>
        <div className="dashboard__main">
          <ConnectionStatusList
            connections={connections}
            isBusy={isBusy}
            onRefresh={onRefresh}
          />
          <WorkspaceSummary workspace={workspace} />
          <RecordTable
            records={records}
            selectedRecordId={selectedRecordId}
            onCreateTask={onCreateTask}
            onSelectRecord={onSelectRecord}
          />
        </div>
        {hasInspector ? (
          <aside className="dashboard__inspector" aria-label={t('aria.inspector')}>
            {records.length > 0 ? (
              <EvidencePanel records={records} selectedRecordId={selectedRecordId} />
            ) : null}
            {promptRuns.length > 0 ? <PromptRegressionPanel runs={promptRuns} /> : null}
          </aside>
        ) : null}
      </div>
    </div>
  )
}

function OverviewSummary({
  metrics,
  health,
}: {
  metrics: WorkbenchRuntimeData['metrics']
  health: string
}) {
  const { t } = useTranslation('dashboard')
  const primaryMetric = metrics[0] ?? {
    label: 'localTasks',
    value: '—',
    delta: 'waiting_for_real_data',
    tone: 'info' as const,
  }

  return (
    <section className="overview-summary" aria-label={t('overview.ariaLabel')}>
      <header className="overview-summary__heading">
        <div>
          <p className="eyebrow">{t('overview.eyebrow')}</p>
          <h2>{t('overview.title')}</h2>
        </div>
        <StatusPill tone={toneForHealth(health)} label={translateHealth(t, health)} />
      </header>
      <div className="overview-summary__body">
        <dl className="overview-facts">
          <div className="overview-fact overview-fact--lead" data-tone={primaryMetric.tone}>
            <dt>{translateMetricLabel(t, primaryMetric.label)}</dt>
            <dd data-available={metricIsAvailable(primaryMetric.value)}>
              {translateMetricValue(t, primaryMetric.value)}
            </dd>
            <span>{translateMetricDelta(t, primaryMetric.delta)}</span>
          </div>
          {metrics.slice(1).map((metric) => (
            <div className="overview-fact" key={metric.label}>
              <dt>{translateMetricLabel(t, metric.label)}</dt>
              <dd data-available={metricIsAvailable(metric.value)}>
                {translateMetricValue(t, metric.value)}
              </dd>
              <span>{translateMetricDelta(t, metric.delta)}</span>
            </div>
          ))}
        </dl>
      </div>
    </section>
  )
}

function ConnectionStatusList({
  connections,
  isBusy,
  onRefresh,
}: {
  connections: WorkbenchRuntimeData['connections']
  isBusy: boolean
  onRefresh: () => void
}) {
  const { t } = useTranslation('dashboard')
  return (
    <section className="connection-status-list" aria-labelledby="connection-status-heading">
      <header className="dashboard-section-heading">
        <div>
          <p className="eyebrow">{t('section.connections.eyebrow')}</p>
          <h2 id="connection-status-heading">{t('section.connections.title')}</h2>
        </div>
        <button className="ghost-button" disabled={isBusy} type="button" onClick={onRefresh}>
          <RefreshCcw size={16} aria-hidden="true" />
          {t('button.retestConnections')}
        </button>
      </header>
      <div className="connection-status-list__rows">
        {connections.length === 0 ? (
          <p className="connection-status-list__empty">{t('empty.noConnections')}</p>
        ) : null}
        {connections.map((item) => {
          const Icon = connectionIcons[item.icon]
          return (
            <div className="connection-status-row" key={item.name}>
              <span className="connection-status-row__icon" data-tone={item.tone}>
                <Icon size={17} aria-hidden="true" />
              </span>
              <div className="connection-status-row__identity">
                <strong>{item.name}</strong>
                <span>{translateConnectionDetail(t, item.detail)}</span>
              </div>
              <span className="connection-status-row__meta">
                {translateConnectionMeta(t, item.meta)}
              </span>
              <StatusPill tone={item.tone} label={translateConnectionStatus(t, item.status)} />
            </div>
          )
        })}
      </div>
    </section>
  )
}

function WorkspaceSummary({
  workspace,
}: {
  workspace: WorkbenchRuntimeData['workspace']
}) {
  const { t } = useTranslation('dashboard')
  return (
    <section className="workspace-summary" aria-labelledby="workspace-summary-heading">
      <div className="workspace-summary__lead">
        <div>
          <p className="eyebrow">{t('section.workspace.eyebrow')}</p>
          <h2 id="workspace-summary-heading">{t('section.workspace.title')}</h2>
        </div>
        <StatusPill
          tone={toneForHealth(workspace.health)}
          label={translateHealth(t, workspace.health)}
        />
      </div>
      <dl className="workspace-summary__facts">
        <div>
          <dt>{t('workspace.localDataPath')}</dt>
          <dd>{translateWorkspaceStorage(t, workspace.storage)}</dd>
        </div>
        <div>
          <dt>{t('workspace.latestBackup')}</dt>
          <dd>{translateWorkspaceBackup(t, workspace.lastBackup)}</dd>
        </div>
      </dl>
    </section>
  )
}

function RecordTable({
  records,
  selectedRecordId,
  onCreateTask,
  onSelectRecord,
}: {
  records: SocialRecord[]
  selectedRecordId: string
  onCreateTask: () => void
  onSelectRecord: (recordId: string) => void
}) {
  const { t } = useTranslation('dashboard')
  const columns = useMemo<ColumnDef<SocialRecord>[]>(
    () => [
      {
        accessorKey: 'id',
        header: t('table.recordId'),
        cell: ({ row }) => <span className="mono">{row.original.id}</span>,
      },
      { accessorKey: 'platform', header: t('table.platform') },
      {
        accessorKey: 'title',
        header: t('table.contentSummary'),
        cell: ({ row }) => <span className="title-cell">{row.original.title}</span>,
      },
      { accessorKey: 'region', header: t('table.region') },
      { accessorKey: 'sentiment', header: t('table.sentiment') },
      {
        accessorKey: 'confidence',
        header: t('table.confidence'),
        cell: ({ row }) => <span>{Math.round(row.original.confidence * 100)}%</span>,
      },
      {
        accessorKey: 'status',
        header: t('table.validationStatus'),
        cell: ({ row }) => (
          <StatusPill
            tone={toneForRecord(row.original.status)}
            label={translateRecordStatus(t, row.original.status)}
          />
        ),
      },
    ],
    [t],
  )

  const table = useReactTable({
    data: records,
    columns,
    getCoreRowModel: getCoreRowModel(),
  })

  return (
    <section className="data-asset" aria-labelledby="data-asset-heading">
      <header className="dashboard-section-heading">
        <div>
          <p className="eyebrow">{t('section.records.eyebrow')}</p>
          <h2 id="data-asset-heading">{t('section.records.title')}</h2>
        </div>
      </header>
      {records.length === 0 ? (
        <div className="dashboard-empty-state">
          <Database size={21} strokeWidth={1.7} aria-hidden="true" />
          <div>
            <h3>{t('empty.noRecords.title')}</h3>
            <p>{t('empty.noRecords.description')}</p>
          </div>
          <button className="primary-button" type="button" onClick={onCreateTask}>
            {t('button.createTask')}
          </button>
        </div>
      ) : (
        <div className="table-shell" role="region" aria-label={t('aria.normalizedRecordsTable')}>
          <table>
            <thead>
              {table.getHeaderGroups().map((headerGroup) => (
                <tr key={headerGroup.id}>
                  {headerGroup.headers.map((header) => (
                    <th key={header.id}>
                      {flexRender(header.column.columnDef.header, header.getContext())}
                    </th>
                  ))}
                </tr>
              ))}
            </thead>
            <tbody>
              {table.getRowModel().rows.map((row) => (
                <tr
                  data-active={selectedRecordId === row.original.id}
                  key={row.id}
                  onClick={() => onSelectRecord(row.original.id)}
                >
                  {row.getVisibleCells().map((cell) => (
                    <td key={cell.id}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

function EvidencePanel({
  records,
  selectedRecordId,
}: {
  records: SocialRecord[]
  selectedRecordId: string
}) {
  const { t } = useTranslation('dashboard')
  const selectedRecord = records.find((record) => record.id === selectedRecordId) ?? records[0]
  if (!selectedRecord) return null

  return (
    <section className="evidence-panel" aria-labelledby="evidence-panel-heading">
      <header>
        <div>
          <p className="eyebrow">{t('section.evidence.eyebrow')}</p>
          <h2 id="evidence-panel-heading">{selectedRecord.id}</h2>
        </div>
        <StatusPill
          tone={toneForRecord(selectedRecord.status)}
          label={translateRecordStatus(t, selectedRecord.status)}
        />
      </header>
      <div className="evidence-panel__body">
        <h3>{selectedRecord.insight}</h3>
        <p>{selectedRecord.evidence}</p>
        <dl>
          <div>
            <dt>{t('evidence.originalSource')}</dt>
            <dd>{selectedRecord.source}</dd>
          </div>
        </dl>
      </div>
    </section>
  )
}

function PromptRegressionPanel({
  runs,
}: {
  runs: WorkbenchRuntimeData['promptRuns']
}) {
  const { t } = useTranslation('dashboard')
  const failedCount = runs.filter((run) => isPromptRunFailed(run.status)).length

  return (
    <section className="regression-panel" aria-labelledby="regression-panel-heading">
      <header>
        <div>
          <p className="eyebrow">{t('section.promptRegression.eyebrow')}</p>
          <h2 id="regression-panel-heading">{t('section.promptRegression.title')}</h2>
        </div>
        <StatusPill
          tone={failedCount ? 'warning' : 'success'}
          label={
            failedCount
              ? t('promptRegression.failedCount', { count: failedCount })
              : t('promptRegression.allPassed')
          }
        />
      </header>
      <div className="regression-panel__list">
        {runs.map((run) => (
          <article key={run.name}>
            <div>
              <strong>{run.name}</strong>
              <span>{run.provider}</span>
            </div>
            <StatusPill
              tone={toneForPromptRun(run.status)}
              label={translatePromptRunStatus(t, run.status)}
            />
            <p>{run.diff}</p>
          </article>
        ))}
      </div>
    </section>
  )
}

function metricIsAvailable(value: string) {
  return (
    value.trim() !== '' &&
    value !== '—' &&
    value !== '未计算' &&
    value !== 'not_available' &&
    value !== 'not_calculated'
  )
}

const connectionStatusKeys: Record<string, string> = {
  connected: 'status.connection.connected',
  已连接: 'status.connection.connected',
  disabled: 'status.connection.disabled',
  未启用: 'status.connection.disabled',
  configuration_unavailable: 'status.connection.configurationUnavailable',
  配置不可用: 'status.connection.configurationUnavailable',
  verified: 'status.connection.verified',
  已验证: 'status.connection.verified',
  failed: 'status.connection.testFailed',
  测试失败: 'status.connection.testFailed',
  needs_rebind: 'status.connection.rebindRequired',
  需重新绑定: 'status.connection.rebindRequired',
  pending_test: 'status.connection.pendingTest',
  待测试: 'status.connection.pendingTest',
  pending_selection: 'status.connection.pendingSelection',
  待选择: 'status.connection.pendingSelection',
  not_configured: 'status.connection.notConfigured',
  未配置: 'status.connection.notConfigured',
  local_rules: 'status.connection.localRules',
  本地规则: 'status.connection.localRules',
}

const connectionDetailKeys: Record<string, string> = {
  结构化输出: 'connection.detail.structuredOutput',
  'n8n 轻集成': 'connection.detail.webhookIntegration',
}

const connectionMetaKeys: Record<string, string> = {
  结构化输出: 'connection.meta.structuredOutput',
  仅发送摘要: 'connection.meta.summaryOnly',
  在设置中选择当前配置: 'connection.meta.chooseInSettings',
  当前自然语言计划仍使用本地规则: 'connection.meta.localRules',
  'API 配置文件无法读取，历史数据仍可浏览': 'connection.meta.configurationUnavailable',
}

const healthKeys: Record<string, string> = {
  available: 'status.health.available',
  可用: 'status.health.available',
  backend_unavailable: 'status.health.backendUnavailable',
  后端不可用: 'status.health.backendUnavailable',
  backend_not_connected: 'status.health.backendNotConnected',
  未连接本地后端: 'status.health.backendNotConnected',
  loading: 'status.health.loading',
  正在加载: 'status.health.loading',
}

const metricLabelKeys: Record<string, string> = {
  localTasks: 'metric.label.localTasks',
  本地任务: 'metric.label.localTasks',
  todayTasks: 'metric.label.todayTasks',
  今日任务: 'metric.label.todayTasks',
  storedRecords: 'metric.label.storedRecords',
  入库记录: 'metric.label.storedRecords',
  estimatedRequests: 'metric.label.estimatedRequests',
  预计请求: 'metric.label.estimatedRequests',
  estimatedCost: 'metric.label.estimatedCost',
  预计成本: 'metric.label.estimatedCost',
  evidenceCoverage: 'metric.label.evidenceCoverage',
  证据覆盖: 'metric.label.evidenceCoverage',
}

const metricDeltaKeys: Record<string, string> = {
  waiting_for_real_data: 'metric.delta.waitingForRealData',
  等待读取真实数据: 'metric.delta.waitingForRealData',
  reading_real_data: 'metric.delta.readingRealData',
  正在读取真实数据: 'metric.delta.readingRealData',
  backend_read_failed: 'metric.delta.backendReadFailed',
  后端读取失败: 'metric.delta.backendReadFailed',
  packaged_app_only: 'metric.delta.packagedAppOnly',
  '仅打包后的 macOS 应用可读取': 'metric.delta.packagedAppOnly',
  records_ingestion_unavailable: 'metric.delta.recordsIngestionUnavailable',
  真实记录读取尚未接入: 'metric.delta.recordsIngestionUnavailable',
  no_real_records: 'metric.delta.noRealRecords',
  暂无真实记录: 'metric.delta.noRealRecords',
  core_insights_have_sources: 'metric.delta.coreInsightsHaveSources',
  核心洞察有来源: 'metric.delta.coreInsightsHaveSources',
}

const recordStatusKeys: Record<string, string> = {
  validated: 'status.record.validated',
  已校验: 'status.record.validated',
  needs_review: 'status.record.needsReview',
  待人工确认: 'status.record.needsReview',
  insufficient_evidence: 'status.record.insufficientEvidence',
  证据不足: 'status.record.insufficientEvidence',
}

const recordStatusTones: Record<string, Tone> = {
  validated: 'success',
  已校验: 'success',
  needs_review: 'warning',
  待人工确认: 'warning',
  insufficient_evidence: 'danger',
  证据不足: 'danger',
}

const promptRunStatusKeys: Record<string, string> = {
  passed: 'status.prompt.passed',
  通过: 'status.prompt.passed',
  failed: 'status.prompt.failed',
  失败: 'status.prompt.failed',
}

function translateHealth(t: DashboardT, health: string) {
  const runningMatch = health.match(/^可用，运行 (\d+) 秒$/) ?? health.match(/^available_running:(\d+)$/)
  if (runningMatch) {
    return t('status.health.availableRunning', { seconds: runningMatch[1] })
  }

  return t(healthKeys[health] ?? 'status.unknown')
}

function toneForHealth(health: string): Tone {
  if (health === '后端不可用' || health === 'backend_unavailable') return 'danger'
  if (
    health === '未连接本地后端' ||
    health === 'backend_not_connected' ||
    health === '正在加载' ||
    health === 'loading'
  ) return 'warning'
  if (
    health === '可用' ||
    health === 'available' ||
    /^可用，运行 \d+ 秒$/.test(health) ||
    /^available_running:\d+$/.test(health)
  ) return 'success'
  return 'warning'
}

function toneForRecord(status: SocialRecord['status']): Tone {
  return recordStatusTones[status] ?? 'warning'
}

function translateConnectionStatus(t: DashboardT, status: string) {
  return t(connectionStatusKeys[status] ?? 'status.unknown')
}

function translateConnectionDetail(t: DashboardT, detail: string) {
  const key = connectionDetailKeys[detail]
  return key ? t(key) : detail
}

function translateConnectionMeta(t: DashboardT, meta: string) {
  const key = connectionMetaKeys[meta]
  const keySuffixMatch = meta.match(/^尾号 (.+)$/) ?? meta.match(/^key_suffix:(.+)$/)

  if (keySuffixMatch) {
    return t('connection.meta.keySuffix', { suffix: keySuffixMatch[1] })
  }

  return key ? t(key) : meta
}

function translateMetricLabel(t: DashboardT, label: string) {
  return t(metricLabelKeys[label] ?? 'metric.label.unknown')
}

function translateMetricDelta(t: DashboardT, delta: string) {
  const pendingMatch = delta.match(/^(\d+) 个待确认$/) ?? delta.match(/^pending_confirmation:(\d+)$/)
  if (pendingMatch) {
    return t('metric.delta.pendingConfirmation', { count: pendingMatch[1] })
  }

  const queuedMatch = delta.match(/^(\d+) 个已入队$/) ?? delta.match(/^queued:(\d+)$/)
  if (queuedMatch) {
    return t('metric.delta.queued', { count: queuedMatch[1] })
  }

  const validatedMatch =
    delta.match(/^(\d+(?:\.\d+)?)% 已校验$/) ?? delta.match(/^validated:(\d+(?:\.\d+)?)$/)
  if (validatedMatch) {
    return t('metric.delta.validatedPercent', { percent: validatedMatch[1] })
  }

  const belowLimitMatch =
    delta.match(/^低于上限 (\d+(?:\.\d+)?)%$/) ?? delta.match(/^below_limit:(\d+(?:\.\d+)?)$/)
  if (belowLimitMatch) {
    return t('metric.delta.belowLimit', { percent: belowLimitMatch[1] })
  }

  return t(metricDeltaKeys[delta] ?? 'metric.delta.unknown')
}

function translateMetricValue(t: DashboardT, value: string) {
  if (value.trim() === '' || value === '—' || value === 'not_available') {
    return t('metric.value.notAvailable')
  }
  if (value === '未计算' || value === 'not_calculated') {
    return t('metric.value.notCalculated')
  }
  return value
}

function translateWorkspaceStorage(t: DashboardT, storage: string) {
  return storage === '尚未读取' || storage === '未读取' ? t('workspace.notRead') : storage
}

function translateWorkspaceBackup(t: DashboardT, lastBackup: string) {
  if (lastBackup === '尚未读取') return t('workspace.notRead')
  if (lastBackup === '未创建备份') return t('workspace.backupNotCreated')
  if (lastBackup === '尚未备份') return t('workspace.noBackup')
  return lastBackup
}

function translateRecordStatus(t: DashboardT, status: SocialRecord['status']) {
  return t(recordStatusKeys[status] ?? 'status.unknown')
}

function isPromptRunFailed(status: string) {
  return status === '失败' || status === 'failed'
}

function toneForPromptRun(status: string): Tone {
  if (isPromptRunFailed(status)) return 'danger'
  if (status === '通过' || status === 'passed') return 'success'
  return 'warning'
}

function translatePromptRunStatus(t: DashboardT, status: string) {
  return t(promptRunStatusKeys[status] ?? 'status.unknown')
}

export default Dashboard
