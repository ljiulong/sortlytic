import { type ReactNode, useMemo, useState } from 'react'
import * as Tabs from '@radix-ui/react-tabs'
import { zodResolver } from '@hookform/resolvers/zod'
import {
  type ColumnDef,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { QueryClient, QueryClientProvider, useQuery } from '@tanstack/react-query'
import {
  Activity,
  AlertTriangle,
  Archive,
  BadgeCheck,
  Bot,
  CheckCircle2,
  ChevronRight,
  Clock3,
  Database,
  Download,
  FileSpreadsheet,
  FileText,
  Gauge,
  KeyRound,
  Layers3,
  ListChecks,
  MessageSquareText,
  MonitorCheck,
  Network,
  Pause,
  Play,
  RefreshCcw,
  Search,
  Settings,
  Share2,
  ShieldCheck,
  Sparkles,
  Table2,
  Wrench,
} from 'lucide-react'
import { useForm } from 'react-hook-form'
import { create } from 'zustand'
import { z } from 'zod'
import './App.css'
import {
  type CollectionPlan,
  type NavKey,
  type SocialRecord,
  type TaskStatus,
  dataTypeOptions,
  platformOptions,
  workspaceSnapshot,
} from './workbench-data'

const queryClient = new QueryClient()

type WorkbenchStore = {
  activeNav: NavKey
  selectedRecordId: string
  setActiveNav: (activeNav: NavKey) => void
  setSelectedRecordId: (selectedRecordId: string) => void
}

const useWorkbenchStore = create<WorkbenchStore>((set) => ({
  activeNav: 'overview',
  selectedRecordId: 'rec-104',
  setActiveNav: (activeNav) => set({ activeNav }),
  setSelectedRecordId: (selectedRecordId) => set({ selectedRecordId }),
}))

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
  { key: 'overview', label: '概览', icon: MonitorCheck },
  { key: 'tasks', label: '任务', icon: ListChecks },
  { key: 'data', label: '数据', icon: Database },
  { key: 'prompts', label: '提示词', icon: Sparkles },
  { key: 'exports', label: '导出', icon: Download },
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
  const { data = workspaceSnapshot } = useQuery({
    queryKey: ['workspace', 'local-mvp'],
    queryFn: async () => workspaceSnapshot,
    staleTime: Number.POSITIVE_INFINITY,
  })
  const activeNav = useWorkbenchStore((state) => state.activeNav)
  const setActiveNav = useWorkbenchStore((state) => state.setActiveNav)

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
        <TopBar workspace={data.workspace} />
        <section className="metric-grid" aria-label="工作区指标">
          {data.metrics.map((metric) => (
            <MetricCard key={metric.label} {...metric} />
          ))}
        </section>

        <section className="main-grid">
          <div className="main-column">
            <ConnectionStrip connections={data.connections} />
            <CollectionBuilder />
            <TaskQueue tasks={data.tasks} />
            <RecordTable records={data.records} />
          </div>
          <aside className="inspector" aria-label="详情与证据">
            <EvidencePanel records={data.records} />
            <PromptRegressionPanel runs={data.promptRuns} />
            <ExportPanel />
          </aside>
        </section>
      </main>
    </div>
  )
}

function TopBar({
  workspace,
}: {
  workspace: typeof workspaceSnapshot.workspace
}) {
  return (
    <header className="topbar">
      <div>
        <p className="eyebrow">工作区</p>
        <h1>{workspace.name}</h1>
      </div>
      <div className="topbar-actions">
        <label className="search-box">
          <Search size={16} aria-hidden="true" />
          <span className="sr-only">全局搜索</span>
          <input placeholder="搜索任务、记录或报告" type="search" />
        </label>
        <button className="ghost-button" type="button">
          <Archive size={16} aria-hidden="true" />
          <span>{workspace.storage}</span>
        </button>
        <button className="primary-button" type="button">
          <Play size={16} aria-hidden="true" />
          <span>新建任务</span>
        </button>
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
}: {
  connections: typeof workspaceSnapshot.connections
}) {
  return (
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">连接状态</p>
          <h2>TikHub、模型与自动化</h2>
        </div>
        <button className="ghost-button" type="button">
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

function CollectionBuilder() {
  const [plan, setPlan] = useState<CollectionPlan>({
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

  const submitForm = (values: CollectionFormValues) => {
    setPlan({
      ...values,
      status: values.budget > 120 ? '待人工确认' : '等待确认',
      missing: values.regionCode.trim() ? [] : ['国家/地区'],
    })
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
            <button className="primary-button form-submit" type="submit">
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
              defaultValue="分析小红书近 30 天新能源汽车女性车主评论，重点看安全感、补能和售后体验，成本控制在 35 美元以内。"
            />
            <div className="action-row">
              <button className="primary-button" type="button">
                <Sparkles size={16} aria-hidden="true" />
                解析为计划
              </button>
              <button className="ghost-button" type="button">
                <RefreshCcw size={16} aria-hidden="true" />
                重新生成
              </button>
            </div>
          </div>
        </Tabs.Content>
      </Tabs.Root>

      <CollectionPlanPreview plan={plan} />
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

function CollectionPlanPreview({ plan }: { plan: CollectionPlan }) {
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
        <InfoLine label="成本" value={`预计 $${(plan.budget * 0.72).toFixed(2)}，上限 $${plan.budget}`} />
        <InfoLine label="缺失条件" value={plan.missing.length ? plan.missing.join('、') : '无'} />
      </div>
      <div className="action-row">
        <button className="primary-button" type="button">
          <CheckCircle2 size={16} aria-hidden="true" />
          确认运行
        </button>
        <button className="ghost-button" type="button">
          <Pause size={16} aria-hidden="true" />
          保存草稿
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
  tasks: typeof workspaceSnapshot.tasks
}) {
  return (
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">任务队列</p>
          <h2>运行、失败与待确认边界</h2>
        </div>
        <button className="ghost-button" type="button">
          <Clock3 size={16} aria-hidden="true" />
          <span>运行日志</span>
        </button>
      </div>
      <div className="task-list">
        {tasks.map((task) => (
          <article className="task-row" key={task.name}>
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
            <button className="icon-text-button" type="button">
              详情
              <ChevronRight size={15} aria-hidden="true" />
            </button>
          </article>
        ))}
      </div>
    </section>
  )
}

function RecordTable({ records }: { records: SocialRecord[] }) {
  const setSelectedRecordId = useWorkbenchStore((state) => state.setSelectedRecordId)
  const selectedRecordId = useWorkbenchStore((state) => state.selectedRecordId)

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
        <button className="ghost-button" type="button">
          <Table2 size={16} aria-hidden="true" />
          <span>字段设置</span>
        </button>
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
                onClick={() => setSelectedRecordId(row.original.id)}
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

function EvidencePanel({ records }: { records: SocialRecord[] }) {
  const selectedRecordId = useWorkbenchStore((state) => state.selectedRecordId)
  const selectedRecord = records.find((record) => record.id === selectedRecordId) ?? records[0]

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
  runs: typeof workspaceSnapshot.promptRuns
}) {
  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">提示词回归</p>
          <h2>版本与 Schema</h2>
        </div>
        <StatusPill tone="warning" label="1 项失败" />
      </div>
      <div className="regression-list">
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

function ExportPanel() {
  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">导出中心</p>
          <h2>Excel 与 PDF 门禁</h2>
        </div>
        <StatusPill tone="info" label="待检查" />
      </div>
      <div className="export-grid">
        <ExportItem
          icon={FileSpreadsheet}
          label="Excel 工作簿"
          meta="7 个工作表，含成本明细"
          tone="success"
        />
        <ExportItem icon={FileText} label="PDF 报告" meta="中文字体可用性待检" tone="warning" />
        <ExportItem icon={Network} label="Webhook 摘要" meta="不发送密钥与完整 Header" tone="info" />
      </div>
      <button className="primary-button wide-button" type="button">
        <ShieldCheck size={16} aria-hidden="true" />
        执行导出检查
      </button>
    </section>
  )
}

function ExportItem({
  icon: Icon,
  label,
  meta,
  tone,
}: {
  icon: typeof FileText
  label: string
  meta: string
  tone: string
}) {
  return (
    <article className="export-item">
      <div className="connection-icon" data-tone={tone}>
        <Icon size={17} aria-hidden="true" />
      </div>
      <div>
        <strong>{label}</strong>
        <span>{meta}</span>
      </div>
    </article>
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
