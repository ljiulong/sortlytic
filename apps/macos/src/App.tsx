import { type ReactNode, useEffect, useMemo, useState } from 'react'
import * as Tabs from '@radix-ui/react-tabs'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  Activity,
  AlertTriangle,
  BadgeCheck,
  Bot,
  BookOpen,
  CheckCircle2,
  Gauge,
  KeyRound,
  Layers3,
  MessageSquareText,
  MonitorCheck,
  RefreshCcw,
  Settings,
  Share2,
  Sparkles,
  Wrench,
} from 'lucide-react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'
import './App.css'
import './App.responsive.css'
import {
  type RuntimeCollectionPlan,
  type WorkbenchRuntimeData,
  useWorkbenchBackend,
} from './use-workbench-backend'
import ExportPanel from './ExportPanel'
import GuidePage from './GuidePage'
import ModelSettingsPanel from './ModelSettingsPanel'
import TikhubSettingsPanel from './TikhubSettingsPanel'
import {
  type NavKey,
  type SocialRecord,
  type TaskStatus,
  dataTypeOptions,
  platformOptions,
} from './workbench-data'

const queryClient = new QueryClient()

const collectionFormSchema = z.object({
  platform: z.enum(platformOptions),
  dataType: z.enum(dataTypeOptions),
  regionCode: z.string().min(2, '国家/地区代码至少 2 位').max(12, '代码过长'),
  keyword: z.string().min(2, '请输入关键词或账号').max(80, '关键词过长'),
  range: z.string().min(4, '请选择时间范围'),
  maxRecords: z.coerce.number().min(10, '至少 10 条').max(5000, 'MVP 单任务上限为 5000 条'),
  budget: z.coerce.number().min(1, '请输入成本上限').max(500, 'MVP 单任务上限为 500'),
})

type CollectionFormInput = z.input<typeof collectionFormSchema>
type CollectionFormValues = z.output<typeof collectionFormSchema>

const navItems = [
  { key: 'overview', label: '工作台', icon: MonitorCheck },
  { key: 'settings', label: '设置', icon: Settings },
] satisfies Array<{ key: NavKey; label: string; icon: typeof MonitorCheck }>

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
      <aside className="sidebar" aria-label="主导航">
        <div className="brand-block">
          <div className="brand-mark">
            <Layers3 size={18} aria-hidden="true" />
          </div>
          <div>
            <p className="brand-name">智能数据整理平台</p>
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

      <main className="workspace">
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
              <ConnectionStrip
                connections={data.connections}
                isBusy={backend.isBusy}
                onRefresh={backend.refresh}
              />
              <TikhubSettingsPanel
                isBusy={backend.isBusy}
                result={backend.tikhubTestResult}
                onSaveAndTest={backend.saveAndTestTikhubToken}
              />
              <ModelSettingsPanel
                isPending={backend.isModelSettingsPending}
                providers={data.modelProviders}
                result={backend.modelValidationResult}
                onSaveAndValidate={backend.saveAndValidateModelProvider}
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
        <button
          aria-label="打开使用指南"
          className="toolbar-icon-button"
          title="使用指南"
          type="button"
          onClick={onOpenGuide}
        >
          <BookOpen size={18} aria-hidden="true" />
        </button>
        <div className="workspace-meta" aria-label="当前工作区状态">
          <span>{workspace.storage}</span>
          <StatusPill
            tone={isInitializing ? 'info' : workspace.health === '浏览器预览' ? 'warning' : 'success'}
            label={workspace.health}
          />
        </div>
      </div>
    </header>
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

function CollectionBuilder({
  actionMessage,
  activePlan,
  isBusy,
  onConfirmPlan,
  onGenerateFormPlan,
  onGenerateNaturalPlan,
}: {
  actionMessage: string
  activePlan?: RuntimeCollectionPlan
  isBusy: boolean
  onConfirmPlan: () => Promise<unknown>
  onGenerateFormPlan: (values: CollectionFormValues) => Promise<RuntimeCollectionPlan>
  onGenerateNaturalPlan: (intentText: string) => Promise<RuntimeCollectionPlan>
}) {
  const [plan, setPlan] = useState<RuntimeCollectionPlan>({
    platform: '小红书',
    dataType: '评论采集',
    regionCode: 'CN',
    keyword: '新能源汽车 女车主 安全感',
    range: '近 30 天',
    maxRecords: 1200,
    budget: 35,
    status: '等待确认',
    missing: [],
  })
  const [naturalText, setNaturalText] = useState(
    '分析中国小红书近 30 天新能源汽车女性车主评论，重点看安全感、补能和售后体验，成本控制在 35 美元以内。',
  )

  useEffect(() => {
    if (activePlan) {
      setPlan(activePlan)
    }
  }, [activePlan])

  const {
    register,
    handleSubmit,
    formState: { errors },
  } = useForm<CollectionFormInput, unknown, CollectionFormValues>({
    resolver: zodResolver(collectionFormSchema),
    defaultValues: {
      platform: plan.platform,
      dataType: plan.dataType,
      regionCode: plan.regionCode,
      keyword: plan.keyword,
      range: plan.range,
      maxRecords: plan.maxRecords,
      budget: plan.budget,
    },
  })

  const submitForm = async (values: CollectionFormValues) => {
    const nextPlan = await onGenerateFormPlan(values)
    setPlan(nextPlan)
  }

  const submitNaturalText = async () => {
    const nextPlan = await onGenerateNaturalPlan(naturalText)
    setPlan(nextPlan)
  }

  return (
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">采集创建</p>
          <h2>表单式采集与自然语言计划</h2>
        </div>
        <StatusPill tone="warning" label="确认前不产生正式采集费用" />
      </div>

      <Tabs.Root className="tabs-root" defaultValue="form">
        <Tabs.List className="tabs-list" aria-label="采集入口">
          <Tabs.Trigger className="tabs-trigger" value="form">
            <Wrench size={15} aria-hidden="true" />
            表单式
          </Tabs.Trigger>
          <Tabs.Trigger className="tabs-trigger" value="natural">
            <MessageSquareText size={15} aria-hidden="true" />
            自然语言
          </Tabs.Trigger>
        </Tabs.List>

        <Tabs.Content className="tabs-content" value="form">
          <form className="collection-form" onSubmit={handleSubmit(submitForm)}>
            <Field label="平台">
              <select {...register('platform')}>
                {platformOptions.map((item) => (
                  <option key={item} value={item}>
                    {item}
                  </option>
                ))}
              </select>
            </Field>
            <Field label="数据类型">
              <select {...register('dataType')}>
                {dataTypeOptions.map((item) => (
                  <option key={item} value={item}>
                    {item}
                  </option>
                ))}
              </select>
            </Field>
            <Field error={errors.regionCode?.message} label="国家/地区">
              <input {...register('regionCode')} placeholder="CN" />
            </Field>
            <Field error={errors.keyword?.message} label="关键词或账号">
              <input {...register('keyword')} />
            </Field>
            <Field error={errors.range?.message} label="时间范围">
              <input {...register('range')} />
            </Field>
            <Field error={errors.maxRecords?.message} label="最大记录数">
              <input type="number" {...register('maxRecords', { valueAsNumber: true })} />
            </Field>
            <Field error={errors.budget?.message} label="成本上限">
              <input type="number" {...register('budget', { valueAsNumber: true })} />
            </Field>
            <button className="primary-button form-submit" disabled={isBusy} type="submit">
              <Gauge size={16} aria-hidden="true" />
              生成计划
            </button>
          </form>
        </Tabs.Content>

        <Tabs.Content className="tabs-content" value="natural">
          <div className="natural-input">
            <label htmlFor="intent">自然语言需求</label>
            <textarea
              id="intent"
              value={naturalText}
              onChange={(event) => setNaturalText(event.target.value)}
            />
            <div className="action-row">
              <button className="primary-button" disabled={isBusy} type="button" onClick={submitNaturalText}>
                <Sparkles size={16} aria-hidden="true" />
                解析为计划
              </button>
              <button className="ghost-button" disabled={isBusy} type="button" onClick={submitNaturalText}>
                <RefreshCcw size={16} aria-hidden="true" />
                重新生成
              </button>
            </div>
          </div>
        </Tabs.Content>
      </Tabs.Root>

      <CollectionPlanPreview
        actionMessage={actionMessage}
        isBusy={isBusy}
        onConfirmPlan={onConfirmPlan}
        plan={plan}
      />
    </section>
  )
}

function Field({
  label,
  error,
  children,
}: {
  label: string
  error?: string
  children: ReactNode
}) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
      {error ? <small>{error}</small> : null}
    </label>
  )
}

function CollectionPlanPreview({
  actionMessage,
  isBusy,
  onConfirmPlan,
  plan,
}: {
  actionMessage: string
  isBusy: boolean
  onConfirmPlan: () => Promise<unknown>
  plan: RuntimeCollectionPlan
}) {
  const canConfirm = Boolean(
    plan.taskId && plan.planId && plan.validationStatus === 'valid' && plan.status === '等待确认',
  )
  const confirmLabel = plan.status === '运行中' ? '已入队' : plan.taskId ? '确认运行' : '先生成计划'

  return (
    <div className="plan-preview">
      <div className="plan-header">
        <div>
          <p className="eyebrow">采集计划</p>
          <h3>{plan.keyword}</h3>
        </div>
        <StatusPill tone={plan.status === '待人工确认' ? 'warning' : 'info'} label={plan.status} />
      </div>
      <div className="plan-grid">
        <InfoLine label="平台" value={plan.platform} />
        <InfoLine label="数据类型" value={plan.dataType} />
        <InfoLine label="国家/地区" value={`${plan.regionCode}，来源：用户输入，置信度 1.00`} />
        <InfoLine label="范围" value={`${plan.range}，最多 ${plan.maxRecords.toLocaleString()} 条`} />
        <InfoLine
          label="成本"
          value={`${plan.costEstimate ?? `预计 $${(plan.budget * 0.72).toFixed(2)}`}，上限 $${plan.budget}`}
        />
        <InfoLine label="缺失条件" value={plan.missing.length ? plan.missing.join('、') : '无'} />
      </div>
      <p className="muted-text">{actionMessage}</p>
      <div className="action-row">
        <button
          className="primary-button"
          disabled={!canConfirm || isBusy}
          type="button"
          onClick={() => {
            void onConfirmPlan()
          }}
        >
          <CheckCircle2 size={16} aria-hidden="true" />
          {confirmLabel}
        </button>
      </div>
    </div>
  )
}

function InfoLine({ label, value }: { label: string; value: string }) {
  return (
    <div className="info-line">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
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

function StatusPill({ tone, label }: { tone: string; label: string }) {
  return (
    <span className="status-pill" data-tone={tone}>
      {iconForTone(tone)}
      {label}
    </span>
  )
}

function iconForTone(tone: string) {
  if (tone === 'success') return <CheckCircle2 size={13} aria-hidden="true" />
  if (tone === 'danger') return <AlertTriangle size={13} aria-hidden="true" />
  if (tone === 'warning') return <Activity size={13} aria-hidden="true" />
  return <BadgeCheck size={13} aria-hidden="true" />
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
