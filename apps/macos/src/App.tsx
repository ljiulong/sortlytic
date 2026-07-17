import { useMemo, useState } from 'react'
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  Bot,
  BookOpen,
  KeyRound,
  MonitorCheck,
  RefreshCcw,
  Settings,
  Share2,
} from 'lucide-react'
import './App.css'
import './App.responsive.css'
import { type WorkbenchRuntimeData, useWorkbenchBackend } from './use-workbench-backend'
import { CollectionBuilder, StatusPill } from './CollectionBuilder'
import ExportPanel from './ExportPanel'
import AppLogo from './AppLogo'
import GuidePage from './GuidePage'
import ModelSettingsPanel from './ModelSettingsPanel'
import ThemeToggle from './ThemeToggle'
import TikhubSettingsPanel from './TikhubSettingsPanel'
import UpdateSettingsPanel from './UpdateSettingsPanel'
import {
  type NavKey,
  type SocialRecord,
  type TaskStatus,
} from './workbench-data'
const queryClient = new QueryClient()
const navItems = [
  { key: 'overview', label: '工作台', icon: MonitorCheck },
  { key: 'settings', label: '设置', icon: Settings },
] satisfies Array<{ key: NavKey; label: string; icon: typeof MonitorCheck }>
const appIdentifier = 'com.steven.sortlytic'
const defaultWorkspaceDirectory = 'default-workspace'
const connectionIcons = {
  key: KeyRound,
  bot: Bot,
  share: Share2,
}
function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Workbench />
    </QueryClientProvider>
  )
}
function Workbench() {
  const backend = useWorkbenchBackend()
  const data = backend.data
  const [activeNav, setActiveNav] = useState<NavKey>('overview')
  const [selectedRecordId, setSelectedRecordId] = useState('rec-104')
  return (
    <div className="app-shell" lang="zh-CN">
      <a className="skip-link" href="#main-content">跳至主要内容</a>
      <aside className="sidebar" aria-label="主导航">
        <div className="brand-block">
          <div className="brand-mark">
            <AppLogo />
          </div>
          <div>
            <p className="brand-name">Sortlytic</p>
            <p className="brand-subtitle">macOS 本地工作区</p>
          </div>
        </div>

        <nav className="nav-list">
          {navItems.map((item) => {
            const Icon = item.icon
            return (
              <button
                className="nav-item"
                data-active={activeNav === item.key}
                key={item.key}
                type="button"
                onClick={() => setActiveNav(item.key)}
              >
                <Icon size={17} aria-hidden="true" />
                <span>{item.label}</span>
              </button>
            )
          })}
        </nav>

        <div className="sidebar-footer">
          <StatusPill tone="success" label="本地优先" />
          <p>密钥仅保存为系统安全存储引用。</p>
        </div>
      </aside>
      <main className="workspace" id="main-content" tabIndex={-1}>
        <TopBar
          actionMessage={backend.actionMessage}
          isInitializing={backend.isInitializing}
          onOpenGuide={() => setActiveNav('guide')}
          workspace={data.workspace}
        />
        {activeNav === 'guide' ? (
          <GuidePage onOpenSettings={() => setActiveNav('settings')} />
        ) : activeNav === 'settings' ? (
          <section className="main-grid" aria-label="连接与本地设置">
            <div className="main-column">
              <LocalWorkspacePanel
                health={data.workspace.health}
                storage={data.workspace.storage}
              />
              <ConnectionStrip
                connections={data.connections}
                isBusy={backend.isBusy}
                onRefresh={backend.refresh}
              />
              <TikhubSettingsPanel
                connector={data.tikhubConnector}
                isBusy={backend.isBusy}
                result={backend.tikhubTestResult}
                onSaveAndTest={backend.saveAndTestTikhubToken}
              />
              <ModelSettingsPanel
                {...backend}
                isPending={backend.isModelSettingsPending}
                providers={data.modelProviders}
                result={backend.modelValidationResult}
              />
              <UpdateSettingsPanel
                {...backend}
                isTauriApp={data.runtimeMode === 'backend'}
              />
            </div>
          </section>
        ) : (
          <>
            <section className="metric-grid" aria-label="工作区指标">
              {data.metrics.map((metric) => (
                <MetricCard key={metric.label} {...metric} />
              ))}
            </section>
            <section className="main-grid">
              <div className="main-column">
                <ConnectionStrip
                  connections={data.connections}
                  isBusy={backend.isBusy}
                  onRefresh={backend.refresh}
                />
                <CollectionBuilder
                  actionMessage={backend.actionMessage}
                  activePlan={backend.activePlan}
                  isBusy={backend.isBusy}
                  onConfirmPlan={backend.confirmActivePlan}
                  onGenerateFormPlan={backend.generateFormPlan}
                  onGenerateNaturalPlan={backend.generateNaturalPlan}
                />
                <TaskQueue tasks={data.tasks} />
                <RecordTable
                  records={data.records}
                  selectedRecordId={selectedRecordId}
                  onSelectRecord={setSelectedRecordId}
                />
              </div>
              <aside className="inspector" aria-label="详情与证据">
                <EvidencePanel records={data.records} selectedRecordId={selectedRecordId} />
                <PromptRegressionPanel runs={data.promptRuns} />
                <ExportPanel
                  isBusy={backend.isBusy}
                  latestExports={backend.latestExports}
                  onExport={backend.exportLatestReport}
                />
              </aside>
            </section>
          </>
        )}
      </main>
    </div>
  )
}
function TopBar({
  actionMessage,
  isInitializing,
  onOpenGuide,
  workspace,
}: {
  actionMessage: string
  isInitializing: boolean
  onOpenGuide: () => void
  workspace: WorkbenchRuntimeData['workspace']
}) {
  return (
    <header className="topbar">
      <div>
        <p className="eyebrow">工作区</p>
        <h1>{workspace.name}</h1>
        <p className="muted-text">{isInitializing ? '正在连接本地后端' : actionMessage}</p>
      </div>
      <div className="topbar-actions">
        <ThemeToggle />
        <button
          aria-label="打开使用指南"
          className="toolbar-icon-button"
          title="使用指南"
          type="button"
          onClick={onOpenGuide}
        >
          <BookOpen size={18} aria-hidden="true" />
        </button>
      </div>
    </header>
  )
}
function LocalWorkspacePanel({
  health,
  storage,
}: {
  health: string
  storage: string
}) {
  const healthTone = health === '后端不可用'
    ? 'danger'
    : health === '未连接本地后端' || health === '正在加载'
      ? 'warning'
      : 'success'

  return (
    <section className="glass-panel local-workspace-panel" aria-label="本地工作区">
      <div className="section-heading">
        <div>
          <p className="eyebrow">本地工作区</p>
          <h2>应用身份与运行状态</h2>
        </div>
        <StatusPill tone={healthTone} label={health} />
      </div>
      <dl className="workspace-detail-grid">
        <div>
          <dt>应用标识</dt>
          <dd>{appIdentifier}</dd>
        </div>
        <div>
          <dt>工作区目录</dt>
          <dd>{defaultWorkspaceDirectory}</dd>
        </div>
        <div>
          <dt>本地路径</dt>
          <dd>{storage}</dd>
        </div>
        <div>
          <dt>后端状态</dt>
          <dd>{health}</dd>
        </div>
      </dl>
    </section>
  )
}
function MetricCard({
  label,
  value,
  delta,
  tone,
}: {
  label: string
  value: string
  delta: string
  tone: string
}) {
  return (
    <article className="metric-card" data-tone={tone}>
      <p>{label}</p>
      <strong>{value}</strong>
      <span>{delta}</span>
    </article>
  )
}

function ConnectionStrip({
  connections,
  isBusy,
  onRefresh,
}: {
  connections: WorkbenchRuntimeData['connections']
  isBusy: boolean
  onRefresh: () => void
}) {
  return (
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">连接状态</p>
          <h2>TikHub、模型与自动化</h2>
        </div>
        <button className="ghost-button" disabled={isBusy} type="button" onClick={onRefresh}>
          <RefreshCcw size={16} aria-hidden="true" />
          <span>重新测试</span>
        </button>
      </div>
      <div className="connection-grid">
        {connections.map((item) => {
          const Icon = connectionIcons[item.icon]
          return (
            <article className="connection-card" key={item.name}>
              <div className="connection-icon" data-tone={item.tone}>
                <Icon size={18} aria-hidden="true" />
              </div>
              <div>
                <p className="connection-name">{item.name}</p>
                <p className="muted-text">{item.detail}</p>
              </div>
              <div className="connection-status">
                <StatusPill tone={item.tone} label={item.status} />
                <span>{item.meta}</span>
              </div>
            </article>
          )
        })}
      </div>
    </section>
  )
}

function TaskQueue({
  tasks,
}: {
  tasks: WorkbenchRuntimeData['tasks']
}) {
  return (
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">任务队列</p>
          <h2>运行、失败与待确认边界</h2>
        </div>
      </div>
      <div className="task-list">
        {tasks.length === 0 ? <p className="muted-text">暂无任务，生成采集计划后会出现在这里。</p> : null}
        {tasks.map((task) => (
          <article className="task-row" key={task.id}>
            <div>
              <h3>{task.name}</h3>
              <p>
                {task.platform} · {task.source} · {task.records.toLocaleString()} 条
              </p>
            </div>
            <StatusPill tone={toneForStatus(task.status)} label={task.status} />
            <div className="progress-cell">
              <div className="progress-bar" aria-label={`${task.name} 进度 ${task.progress}%`}>
                <span style={{ width: `${task.progress}%` }} />
              </div>
              <strong>{task.cost}</strong>
            </div>
          </article>
        ))}
      </div>
    </section>
  )
}

function RecordTable({
  records,
  selectedRecordId,
  onSelectRecord,
}: {
  records: SocialRecord[]
  selectedRecordId: string
  onSelectRecord: (recordId: string) => void
}) {
  const columns = useMemo<ColumnDef<SocialRecord>[]>(
    () => [
      {
        accessorKey: 'id',
        header: '记录 ID',
        cell: ({ row }) => <span className="mono">{row.original.id}</span>,
      },
      {
        accessorKey: 'platform',
        header: '平台',
      },
      {
        accessorKey: 'title',
        header: '内容摘要',
        cell: ({ row }) => <span className="title-cell">{row.original.title}</span>,
      },
      {
        accessorKey: 'region',
        header: '国家/地区',
      },
      {
        accessorKey: 'sentiment',
        header: '情绪',
      },
      {
        accessorKey: 'confidence',
        header: '置信度',
        cell: ({ row }) => <span>{Math.round(row.original.confidence * 100)}%</span>,
      },
      {
        accessorKey: 'status',
        header: '校验状态',
        cell: ({ row }) => <StatusPill tone={toneForRecord(row.original.status)} label={row.original.status} />,
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
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">数据资产</p>
          <h2>原始数据、AI 结果与来源联动</h2>
        </div>
      </div>
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

  if (!selectedRecord) {
    return (
      <section className="glass-panel compact-panel">
        <div className="section-heading">
          <div>
            <p className="eyebrow">来源追溯</p>
            <h2>暂无真实记录</h2>
          </div>
          <StatusPill tone="info" label="等待采集" />
        </div>
        <p className="muted-text">完成真实采集并入库后，这里会显示来源、模型运行和转换理由。</p>
      </section>
    )
  }

  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">来源追溯</p>
          <h2>{selectedRecord.id}</h2>
        </div>
        <StatusPill tone={toneForRecord(selectedRecord.status)} label={selectedRecord.status} />
      </div>
      <div className="evidence-body">
        <h3>{selectedRecord.insight}</h3>
        <p>{selectedRecord.evidence}</p>
        <dl>
          <div>
            <dt>原始链接</dt>
            <dd>{selectedRecord.source}</dd>
          </div>
          <div>
            <dt>模型运行</dt>
            <dd>run-ai-20260705-014，提示词 v1.3.2</dd>
          </div>
          <div>
            <dt>转换理由</dt>
            <dd>字段证据、评论集合与平台元数据一致。</dd>
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
  const hasRuns = runs.length > 0

  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">提示词回归</p>
          <h2>版本与 Schema</h2>
        </div>
        <StatusPill
          tone={!hasRuns ? 'info' : failedCount ? 'warning' : 'success'}
          label={!hasRuns ? '未运行' : `${failedCount} 项失败`}
        />
      </div>
      <div className="regression-list">
        {!hasRuns ? <p className="muted-text">尚无真实回归执行结果。</p> : null}
        {runs.map((run) => (
          <article className="regression-row" key={run.name}>
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

function toneForStatus(status: TaskStatus) {
  if (status === '成功') return 'success'
  if (status === '失败') return 'danger'
  if (status === '待人工确认' || status === '等待确认') return 'warning'
  return 'info'
}
function toneForRecord(status: SocialRecord['status']) {
  if (status === '已校验') return 'success'
  if (status === '证据不足') return 'danger'
  return 'warning'
}

export default App
