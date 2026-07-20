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
import { CollectionBuilder } from './CollectionBuilder'
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
          <p className="brand-name">{t('common:appName')}</p>
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
      </aside>
      <main className="workspace" id="main-content" tabIndex={-1}>
        <TopBar
          activeNav={activeNav}
          actionMessage={backend.actionMessage}
          isInitializing={backend.isInitializing}
          onOpenGuide={() => setActiveNav('guide')}
        />
        <div className="workspace-scroll">
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
        </div>
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
  const { t: tMessages } = useTranslation('messages')
  const pageCopy = pageMetaTranslationKeys[activeNav]
  const actionCopy = localizeBackendMessage(actionMessage)

  return (
    <header className="topbar">
      <div className="topbar-copy">
        <p className="eyebrow">{t(pageCopy.context)}</p>
        <h1>{t(pageCopy.title)}</h1>
        <p className="page-description">{t(pageCopy.description)}</p>
      </div>
      <div className="topbar-actions">
        <p className="topbar-status" aria-live="polite">
          {isInitializing ? t('initializingBackend') : tMessages(actionCopy.key, actionCopy.options)}
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

type MessageCopy = {
  key: string
  options?: Record<string, string | number>
}

const backendMessageKeys: Record<string, string> = {
  '后端正在初始化本地工作区': 'action.initializing',
  '等待生成': 'action.waitingForPlan',
  '等待确认': 'action.awaitingConfirmation',
  '采集计划已保存到本地 SQLite，等待确认运行': 'action.formPlanSaved',
  '自然语言计划已生成，并保存了提示词运行快照': 'action.naturalPlanSaved',
  '任务已确认并加入本地队列': 'action.confirmed',
  '任务名称已更新': 'action.renamed',
  '任务已取消': 'action.cancelled',
  '任务已删除': 'action.deleted',
  '当前未连接本地后端，不展示预览数据；请打开打包后的 macOS 应用': 'action.backendUnavailable',
  '本地工作区已打开，后端可用': 'action.workspaceReady',
  '计划需要修正': 'action.planNeedsRevision',
  '后端调用失败': 'error.backendCallFailed',
  '后端读取失败': 'error.backendReadFailed',
  '数据库读取失败': 'error.databaseReadFailed',
  '请先生成采集计划': 'error.generatePlanFirst',
  '请先检查更新': 'error.checkUpdateFirst',
  '请在打包后的 macOS 应用内使用后端能力': 'error.packagedAppRequired',
  '任务名称至少需要 2 个字符': 'error.taskNameTooShort',
  'TikHub 实时报价超过计划预算上限': 'error.pricingExceedsBudget',
}

const dynamicBackendMessageKeys: Array<[RegExp, string]> = [
  [/(?:HTTP|code) 429|请求过于频繁/, 'error.pricingRateLimited'],
]

// oxlint-disable-next-line react/only-export-components
export function localizeBackendMessage(message: string): MessageCopy {
  const exportMatch = /^(Excel|PDF) 已导出到本地工作区$/.exec(message)
  if (exportMatch) return { key: 'action.exported', options: { format: exportMatch[1] } }
  const key = backendMessageKeys[message]
  if (key) return { key }
  const dynamicKey = dynamicBackendMessageKeys.find(([pattern]) => pattern.test(message))?.[1]
  if (dynamicKey) return { key: dynamicKey }
  return message.trim()
    ? { key: 'error.raw', options: { message } }
    : { key: 'error.unknown' }
}

export default App
