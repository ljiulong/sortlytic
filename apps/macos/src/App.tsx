import { useState } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
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
const navigationLabelKeys: Record<PrimaryNavKey, string> = {
  overview: 'overview',
  'new-task': 'newTask',
  tasks: 'tasks',
  settings: 'settings',
}
const pageMetaTranslationKeys: Record<NavKey, { context: string; title: string; description: string }> = {
  overview: {
    context: 'overviewContext',
    title: 'overviewTitle',
    description: 'overviewDescription',
  },
  'new-task': {
    context: 'newTaskContext',
    title: 'newTaskTitle',
    description: 'newTaskDescription',
  },
  tasks: {
    context: 'tasksContext',
    title: 'tasksTitle',
    description: 'tasksDescription',
  },
  settings: {
    context: 'settingsContext',
    title: 'settingsTitle',
    description: 'settingsDescription',
  },
  guide: {
    context: 'guideContext',
    title: 'guideTitle',
    description: 'guideDescription',
  },
}
function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <Workbench />
    </QueryClientProvider>
  )
}
function Workbench() {
  const { i18n, t } = useTranslation(['navigation', 'common'])
  const backend = useWorkbenchBackend()
  const data = backend.data
  const [activeNav, setActiveNav] = useState<NavKey>('overview')
  const [selectedRecordId, setSelectedRecordId] = useState('')
  const pageLayoutClassName = `page-layout page-layout--${pageMeta[activeNav].layout}`
  return (
    <div className="app-shell" lang={i18n.resolvedLanguage ?? i18n.language}>
      <a className="skip-link" href="#main-content">{t('skipToContent')}</a>
      <aside className="sidebar" aria-label={t('mainNavigation')}>
        <div className="brand-block">
          <div className="brand-mark">
            <AppLogo />
          </div>
          <div>
            <p className="brand-name">{t('common:appName')}</p>
            <p className="brand-subtitle">{t('common:brandSubtitle')}</p>
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
                <span>{t(navigationLabelKeys[item.key])}</span>
              </button>
            )
          })}
        </nav>

        <div className="sidebar-footer">
          <StatusPill tone="success" label={t('localFirst')} />
          <p>{t('secretsNotice')}</p>
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
          <section className={pageLayoutClassName} aria-label={t('settingsSection')}>
            <SettingsPage backend={backend} />
          </section>
        ) : activeNav === 'new-task' ? (
          <section className={pageLayoutClassName} aria-label={t('newTaskSection')}>
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
          <section className={pageLayoutClassName} aria-label={t('tasksSection')}>
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
  const { t } = useTranslation('navigation')
  const pageCopy = pageMetaTranslationKeys[activeNav]

  return (
    <header className="topbar">
      <div className="topbar-copy">
        <p className="eyebrow">{t(pageCopy.context)}</p>
        <h1>{t(pageCopy.title)}</h1>
        <p className="page-description">{t(pageCopy.description)}</p>
      </div>
      <div className="topbar-actions">
        <p className="topbar-status" aria-live="polite">
          {isInitializing ? t('initializingBackend') : actionMessage}
        </p>
        <ThemeToggle />
        <button
          aria-label={t('openGuide')}
          className="toolbar-icon-button"
          title={t('guide')}
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
