import { useState } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  Bot,
  BookOpen,
  CirclePlus,
  House,
  KeyRound,
  ListTodo,
  RefreshCcw,
  Settings,
  Share2,
} from 'lucide-react'
import './App.css'
import './App.responsive.css'
import { type WorkbenchRuntimeData, useWorkbenchBackend } from './use-workbench-backend'
import { CollectionBuilder, StatusPill } from './CollectionBuilder'
import AppLogo from './AppLogo'
import Dashboard from './Dashboard'
import GuidePage from './GuidePage'
import ModelSettingsPanel from './ModelSettingsPanel'
import TaskQueue from './TaskQueue'
import ThemeToggle from './ThemeToggle'
import TikhubSettingsPanel from './TikhubSettingsPanel'
import UpdateSettingsPanel from './UpdateSettingsPanel'
import {
  type NavKey,
  type PrimaryNavKey,
  primaryNavigation,
} from './navigation'
import { pageMeta } from './page-meta'
const queryClient = new QueryClient()
const navigationIcons: Record<PrimaryNavKey, typeof House> = {
  overview: House,
  'new-task': CirclePlus,
  tasks: ListTodo,
  settings: Settings,
}
const navItems = primaryNavigation.map((item) => ({
  ...item,
  icon: navigationIcons[item.key],
}))
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
  const [selectedRecordId, setSelectedRecordId] = useState('')
  const pageLayoutClassName = `page-layout page-layout--${pageMeta[activeNav].layout}`
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
          activeNav={activeNav}
          actionMessage={backend.actionMessage}
          isInitializing={backend.isInitializing}
          onOpenGuide={() => setActiveNav('guide')}
          workspace={data.workspace}
        />
        {activeNav === 'guide' ? (
          <div className={pageLayoutClassName}>
            <GuidePage onOpenSettings={() => setActiveNav('settings')} />
          </div>
        ) : activeNav === 'settings' ? (
          <section className={pageLayoutClassName} aria-label="连接与本地设置">
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
        ) : activeNav === 'new-task' ? (
          <section className={pageLayoutClassName} aria-label="新建任务">
            <div className="main-column">
              <CollectionBuilder
                actionMessage={backend.actionMessage}
                activePlan={backend.activePlan}
                isBusy={backend.isBusy}
                onConfirmPlan={backend.confirmActivePlan}
                onGenerateFormPlan={backend.generateFormPlan}
                onGenerateNaturalPlan={backend.generateNaturalPlan}
              />
            </div>
          </section>
        ) : activeNav === 'tasks' ? (
          <section className={pageLayoutClassName} aria-label="任务">
            <div className="main-column">
              <TaskQueue
                isBusy={backend.isBusy}
                tasks={data.tasks}
                onCancelTask={backend.cancelTask}
                onConfirmTask={backend.confirmTask}
                onExportTask={backend.exportTask}
                onUpdateTask={backend.updateTask}
              />
            </div>
          </section>
        ) : (
          <Dashboard
            connections={data.connections}
            isBusy={backend.isBusy}
            metrics={data.metrics}
            promptRuns={data.promptRuns}
            records={data.records}
            selectedRecordId={selectedRecordId}
            workspace={data.workspace}
            onCreateTask={() => setActiveNav('new-task')}
            onRefresh={backend.refresh}
            onSelectRecord={setSelectedRecordId}
          />
        )}
      </main>
    </div>
  )
}
function TopBar({
  activeNav,
  actionMessage,
  isInitializing,
  onOpenGuide,
  workspace,
}: {
  activeNav: NavKey
  actionMessage: string
  isInitializing: boolean
  onOpenGuide: () => void
  workspace: WorkbenchRuntimeData['workspace']
}) {
  const currentPage = pageMeta[activeNav]

  return (
    <header className="topbar">
      <div className="topbar-copy">
        <p className="eyebrow">{workspace.name}</p>
        <h1>{currentPage.title}</h1>
        <p className="page-description">{currentPage.description}</p>
      </div>
      <div className="topbar-actions">
        <p className="topbar-status" aria-live="polite">
          {isInitializing ? '正在连接本地后端' : actionMessage}
        </p>
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

export default App
