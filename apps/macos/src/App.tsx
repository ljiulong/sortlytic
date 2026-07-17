import { useState } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  BookOpen,
  CirclePlus,
  House,
  ListTodo,
  Settings,
} from 'lucide-react'
import './App.css'
import './App.responsive.css'
import { useWorkbenchBackend } from './use-workbench-backend'
import { CollectionBuilder, StatusPill } from './CollectionBuilder'
import AppLogo from './AppLogo'
import Dashboard from './Dashboard'
import GuidePage from './GuidePage'
import SettingsPage from './SettingsPage'
import TaskQueue from './TaskQueue'
import ThemeToggle from './ThemeToggle'
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
          <p>密钥明文写入当前工作区私有 JSON，不进入数据库、日志或导出。</p>
        </div>
      </aside>
      <main className="workspace" id="main-content" tabIndex={-1}>
        <TopBar
          activeNav={activeNav}
          actionMessage={backend.actionMessage}
          isInitializing={backend.isInitializing}
          onOpenGuide={() => setActiveNav('guide')}
        />
        {activeNav === 'guide' ? (
          <div className={pageLayoutClassName}>
            <GuidePage onOpenSettings={() => setActiveNav('settings')} />
          </div>
        ) : activeNav === 'settings' ? (
          <section className={pageLayoutClassName} aria-label="连接与本地设置">
            <SettingsPage backend={backend} />
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
                onDeleteTask={backend.deleteTask}
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
}: {
  activeNav: NavKey
  actionMessage: string
  isInitializing: boolean
  onOpenGuide: () => void
}) {
  const currentPage = pageMeta[activeNav]

  return (
    <header className="topbar">
      <div className="topbar-copy">
        <p className="eyebrow">{currentPage.context}</p>
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
export default App
