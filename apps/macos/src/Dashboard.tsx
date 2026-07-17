import { useMemo } from 'react'
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { Bot, Database, KeyRound, RefreshCcw, Share2 } from 'lucide-react'
import { StatusPill } from './CollectionBuilder'
import type { WorkbenchRuntimeData } from './use-workbench-backend'
import type { SocialRecord } from './workbench-data'
import './Dashboard.css'

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
          <aside className="dashboard__inspector" aria-label="详情与证据">
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
  const primaryMetric = metrics[0] ?? {
    label: '本地任务',
    value: '—',
    delta: '等待读取真实数据',
    tone: 'info' as const,
  }

  return (
    <section className="overview-summary" aria-label="工作区运行概览">
      <header className="overview-summary__heading">
        <div>
          <p className="eyebrow">运行概览</p>
          <h2>本地工作流</h2>
        </div>
        <StatusPill tone={toneForHealth(health)} label={health} />
      </header>
      <div className="overview-summary__body">
        <div className="overview-fact overview-fact--lead" data-tone={primaryMetric.tone}>
          <span>{primaryMetric.label}</span>
          <strong data-available={metricIsAvailable(primaryMetric.value)}>{primaryMetric.value}</strong>
          <p>{primaryMetric.delta}</p>
        </div>
        <dl className="overview-facts">
          {metrics.slice(1).map((metric) => (
            <div className="overview-fact" key={metric.label}>
              <dt>{metric.label}</dt>
              <dd data-available={metricIsAvailable(metric.value)}>{metric.value}</dd>
              <span>{metric.delta}</span>
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
  return (
    <section className="connection-status-list" aria-labelledby="connection-status-heading">
      <header className="dashboard-section-heading">
        <div>
          <p className="eyebrow">连接状态</p>
          <h2 id="connection-status-heading">数据来源与处理能力</h2>
        </div>
        <button className="ghost-button" disabled={isBusy} type="button" onClick={onRefresh}>
          <RefreshCcw size={16} aria-hidden="true" />
          重新测试
        </button>
      </header>
      <div className="connection-status-list__rows">
        {connections.length === 0 ? (
          <p className="connection-status-list__empty">尚未读取到真实连接状态。</p>
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
                <span>{item.detail}</span>
              </div>
              <span className="connection-status-row__meta">{item.meta}</span>
              <StatusPill tone={item.tone} label={item.status} />
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
  return (
    <section className="workspace-summary" aria-labelledby="workspace-summary-heading">
      <div className="workspace-summary__lead">
        <div>
          <p className="eyebrow">本机存储</p>
          <h2 id="workspace-summary-heading">数据保留在当前 Mac</h2>
        </div>
        <StatusPill tone={toneForHealth(workspace.health)} label={workspace.health} />
      </div>
      <dl className="workspace-summary__facts">
        <div>
          <dt>本地数据路径</dt>
          <dd>{workspace.storage}</dd>
        </div>
        <div>
          <dt>最近备份</dt>
          <dd>{workspace.lastBackup}</dd>
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
  const columns = useMemo<ColumnDef<SocialRecord>[]>(
    () => [
      {
        accessorKey: 'id',
        header: '记录 ID',
        cell: ({ row }) => <span className="mono">{row.original.id}</span>,
      },
      { accessorKey: 'platform', header: '平台' },
      {
        accessorKey: 'title',
        header: '内容摘要',
        cell: ({ row }) => <span className="title-cell">{row.original.title}</span>,
      },
      { accessorKey: 'region', header: '国家/地区' },
      { accessorKey: 'sentiment', header: '情绪' },
      {
        accessorKey: 'confidence',
        header: '置信度',
        cell: ({ row }) => <span>{Math.round(row.original.confidence * 100)}%</span>,
      },
      {
        accessorKey: 'status',
        header: '校验状态',
        cell: ({ row }) => (
          <StatusPill tone={toneForRecord(row.original.status)} label={row.original.status} />
        ),
      },
    ],
    [],
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
          <p className="eyebrow">数据资产</p>
          <h2 id="data-asset-heading">真实记录与来源联动</h2>
        </div>
      </header>
      {records.length === 0 ? (
        <div className="dashboard-empty-state">
          <Database size={21} strokeWidth={1.7} aria-hidden="true" />
          <div>
            <h3>尚无真实记录</h3>
            <p>完成一个任务并成功入库后，这里会显示标准化记录、来源证据和模型结果。</p>
          </div>
          <button className="primary-button" type="button" onClick={onCreateTask}>新建任务</button>
        </div>
      ) : (
        <div className="table-shell" role="region" aria-label="标准化记录表">
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
  const selectedRecord = records.find((record) => record.id === selectedRecordId) ?? records[0]
  if (!selectedRecord) return null

  return (
    <section className="evidence-panel" aria-labelledby="evidence-panel-heading">
      <header>
        <div>
          <p className="eyebrow">来源追溯</p>
          <h2 id="evidence-panel-heading">{selectedRecord.id}</h2>
        </div>
        <StatusPill tone={toneForRecord(selectedRecord.status)} label={selectedRecord.status} />
      </header>
      <div className="evidence-panel__body">
        <h3>{selectedRecord.insight}</h3>
        <p>{selectedRecord.evidence}</p>
        <dl>
          <div>
            <dt>原始来源</dt>
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
  const failedCount = runs.filter((run) => run.status === '失败').length

  return (
    <section className="regression-panel" aria-labelledby="regression-panel-heading">
      <header>
        <div>
          <p className="eyebrow">提示词回归</p>
          <h2 id="regression-panel-heading">版本与 Schema</h2>
        </div>
        <StatusPill
          tone={failedCount ? 'warning' : 'success'}
          label={failedCount ? `${failedCount} 项失败` : '全部通过'}
        />
      </header>
      <div className="regression-panel__list">
        {runs.map((run) => (
          <article key={run.name}>
            <div>
              <strong>{run.name}</strong>
              <span>{run.provider}</span>
            </div>
            <StatusPill tone={run.status === '通过' ? 'success' : 'danger'} label={run.status} />
            <p>{run.diff}</p>
          </article>
        ))}
      </div>
    </section>
  )
}

function metricIsAvailable(value: string) {
  return value.trim() !== '' && value !== '—' && value !== '未计算'
}

function toneForHealth(health: string) {
  if (health === '后端不可用') return 'danger'
  if (health === '未连接本地后端' || health === '正在加载') return 'warning'
  return 'success'
}

function toneForRecord(status: SocialRecord['status']) {
  if (status === '已校验') return 'success'
  if (status === '证据不足') return 'danger'
  return 'warning'
}

export default Dashboard
