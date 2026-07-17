import { Bot, ChevronRight, HardDrive, KeyRound, Server } from 'lucide-react'
import { useReducer, useState } from 'react'
import { useTranslation } from 'react-i18next'
import ApiProfilesDialog from './ApiProfilesDialog'
import AppSelect from './AppSelect'
import { StatusPill } from './CollectionBuilder'
import UpdateSettingsPanel from './UpdateSettingsPanel'
import type {
  ApiProfileKind,
  ApiProfileRegistryView,
  ApiProfileView,
} from './api-profiles'
import { useApiProfiles } from './use-api-profiles'
import type { useWorkbenchBackend } from './use-workbench-backend'
import {
  changeAppLanguage,
  i18n,
  normalizeLanguage,
  supportedLanguages,
  type AppLanguage,
} from './i18n'
import './SettingsPage.css'

type SettingsBackend = ReturnType<typeof useWorkbenchBackend>
type WorkspaceHealthStatus = {
  value: string
  tone: 'success' | 'warning' | 'danger'
}
type SettingsApiDialogAction =
  | { type: 'open'; kind: ApiProfileKind }
  | { type: 'close' }

function settingsApiDialogReducer(
  _state: ApiProfileKind | null,
  action: SettingsApiDialogAction,
): ApiProfileKind | null {
  return action.type === 'open' ? action.kind : null
}

function SettingsPage({ backend }: { backend: SettingsBackend }) {
  const { t } = useTranslation('settings')
  const data = backend.data
  const apiProfiles = useApiProfiles()
  const [apiDialogKind, dispatchApiDialog] = useReducer(
    settingsApiDialogReducer,
    null,
  )
  const tikhubStatus = apiProfileStatus(
    'tikhub',
    apiProfiles.registry,
    apiProfiles.registryQuery.isLoading,
    Boolean(apiProfiles.registryQuery.error),
    t,
  )
  const aiStatus = apiProfileStatus(
    'ai',
    apiProfiles.registry,
    apiProfiles.registryQuery.isLoading,
    Boolean(apiProfiles.registryQuery.error),
    t,
  )
  const workspaceHealth = workspaceHealthStatus(data.workspace.health, t)

  return (
    <div className="settings-page">
      <section className="settings-page__status" aria-labelledby="settings-status-heading">
        <header>
          <p className="eyebrow">{t('status.eyebrow')}</p>
          <h2 id="settings-status-heading">{t('status.heading')}</h2>
        </header>
        <dl>
          <SettingsStatus
            label={t('status.dataSource')}
            value={tikhubStatus.value}
            tone={tikhubStatus.tone}
            statusLabel={statusLabel(tikhubStatus.tone, t)}
          />
          <SettingsStatus
            label={t('status.aiApi')}
            value={aiStatus.value}
            tone={aiStatus.tone}
            statusLabel={statusLabel(aiStatus.tone, t)}
          />
          <SettingsStatus
            label={t('status.localBackend')}
            value={workspaceHealth.value}
            tone={workspaceHealth.tone}
            statusLabel={statusLabel(workspaceHealth.tone, t)}
          />
        </dl>
      </section>

      <SettingsGroup
        icon={KeyRound}
        eyebrow={t('api.eyebrow')}
        title={t('api.title')}
        description={t('api.description')}
      >
        <div className="settings-page__api-actions">
          <button
            aria-haspopup="dialog"
            className="settings-page__api-button"
            data-api-profile-kind="tikhub"
            type="button"
            onClick={() => dispatchApiDialog({ type: 'open', kind: 'tikhub' })}
          >
            <span className="settings-page__api-button-icon" aria-hidden="true">
              <Server size={17} />
            </span>
            <span>{t('api.configureTikHub')}</span>
            <ChevronRight size={16} aria-hidden="true" />
          </button>
          <button
            aria-haspopup="dialog"
            className="settings-page__api-button"
            data-api-profile-kind="ai"
            type="button"
            onClick={() => dispatchApiDialog({ type: 'open', kind: 'ai' })}
          >
            <span className="settings-page__api-button-icon" aria-hidden="true">
              <Bot size={17} />
            </span>
            <span>{t('api.configureAi')}</span>
            <ChevronRight size={16} aria-hidden="true" />
          </button>
        </div>
      </SettingsGroup>

      <SettingsGroup
        icon={HardDrive}
        eyebrow={t('local.eyebrow')}
        title={t('local.title')}
        description={t('local.description')}
      >
        <WorkspaceSettings
          t={t}
          healthStatus={workspaceHealth}
          lastBackup={data.workspace.lastBackup}
          storage={data.workspace.storage}
        />
        <LanguageSettings />
        <UpdateSettingsPanel
          {...backend}
          isTauriApp={data.runtimeMode === 'backend'}
        />
      </SettingsGroup>

      {apiDialogKind ? (
        <ApiProfilesDialog
          isOpen
          kind={apiDialogKind}
          onClose={() => dispatchApiDialog({ type: 'close' })}
        />
      ) : null}
    </div>
  )
}

function apiProfileStatus(
  kind: ApiProfileKind,
  registry: ApiProfileRegistryView | undefined,
  isLoading: boolean,
  hasError: boolean,
  t: SettingsTranslate,
) {
  const nounKey = kind === 'tikhub' ? 'tikhub' : 'ai'
  if (isLoading) return { value: t(`status.${nounKey}Loading`), tone: 'warning' }
  if (hasError || !registry) return { value: t(`status.${nounKey}ReadFailed`), tone: 'danger' }

  const profiles: ApiProfileView[] = kind === 'tikhub'
    ? registry.tikhubProfiles
    : registry.aiProfiles
  const activeProfileId = registry.activeProfileIds[kind]
  const activeProfile = profiles.find((profile) => profile.id === activeProfileId)
  if (activeProfile?.status === 'success') {
    return { value: t('status.activeProfile', { name: activeProfile.name }), tone: 'success' }
  }
  if (profiles.length === 0) {
    return { value: t(`status.${nounKey}NotConfigured`), tone: 'warning' }
  }
  if (profiles.some((profile) => profile.status === 'needs_rebind')) {
    return { value: t(`status.${nounKey}NeedsRebind`), tone: 'warning' }
  }
  return { value: t(`status.${nounKey}NeedsSelection`), tone: 'warning' }
}

type SettingsTranslate = (key: string, options?: Record<string, unknown>) => string

function statusLabel(tone: string, t: SettingsTranslate) {
  return tone === 'success' ? t('status.available') : tone === 'danger' ? t('status.error') : t('status.pending')
}

function SettingsStatus({
  label,
  value,
  tone,
  statusLabel: labelForStatus,
}: {
  label: string
  value: string
  tone: string
  statusLabel: string
}) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
      <StatusPill tone={tone} label={labelForStatus} />
    </div>
  )
}

function SettingsGroup({
  icon: Icon,
  eyebrow,
  title,
  description,
  children,
}: {
  icon: typeof KeyRound
  eyebrow: string
  title: string
  description: string
  children: React.ReactNode
}) {
  return (
    <section className="settings-page__group">
      <header>
        <span className="settings-page__group-icon"><Icon size={17} aria-hidden="true" /></span>
        <div>
          <p className="eyebrow">{eyebrow}</p>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
      </header>
      <div className="settings-page__group-body">{children}</div>
    </section>
  )
}

function WorkspaceSettings({
  t,
  healthStatus,
  lastBackup,
  storage,
}: {
  t: SettingsTranslate
  healthStatus: WorkspaceHealthStatus
  lastBackup: string
  storage: string
}) {
  return (
    <section className="workspace-settings" aria-labelledby="workspace-settings-heading">
      <header>
        <div>
          <p className="eyebrow">{t('workspace.eyebrow')}</p>
          <h3 id="workspace-settings-heading">{t('workspace.title')}</h3>
        </div>
        <StatusPill tone={healthStatus.tone} label={healthStatus.value} />
      </header>
      <dl>
        <div>
          <dt>{t('workspace.storagePath')}</dt>
          <dd>{workspaceValue(storage, t)}</dd>
        </div>
        <div>
          <dt>{t('workspace.appId')}</dt>
          <dd>com.steven.sortlytic</dd>
        </div>
        <div>
          <dt>{t('workspace.directory')}</dt>
          <dd>default-workspace</dd>
        </div>
        <div>
          <dt>{t('workspace.lastBackup')}</dt>
          <dd>{workspaceValue(lastBackup, t)}</dd>
        </div>
      </dl>
    </section>
  )
}

function LanguageSettings() {
  const { t } = useTranslation('settings')
  const [isChanging, setIsChanging] = useState(false)
  const language = normalizeLanguage(i18n.resolvedLanguage)
  const options = supportedLanguages.map((value) => ({
    value,
    label: value === 'zh-CN' ? t('language.chinese') : t('language.english'),
    description: value === 'zh-CN' ? 'zh-CN' : 'en-US',
  }))

  return (
    <section className="workspace-settings" aria-labelledby="language-settings-heading">
      <header>
        <div>
          <p className="eyebrow">{t('language.eyebrow')}</p>
          <h3 id="language-settings-heading">{t('language.title')}</h3>
        </div>
        <StatusPill tone="info" label={isChanging ? t('language.switching') : language} />
      </header>
      <div>
        <p className="muted-text">{t('language.description')}</p>
        <AppSelect
          id="app-language"
          disabled={isChanging}
          onChange={(value) => {
            if (!isSupportedAppLanguage(value)) return
            setIsChanging(true)
            void changeAppLanguage(value).finally(() => setIsChanging(false))
          }}
          options={options}
          placeholder={t('language.placeholder')}
          value={language}
        />
      </div>
    </section>
  )
}

function isSupportedAppLanguage(value: string): value is AppLanguage {
  return supportedLanguages.some((language) => language === value)
}

type WorkspaceHealthState = 'available' | 'unavailable' | 'disconnected' | 'loading' | 'unknown'

function workspaceHealthState(health: string): WorkspaceHealthState {
  if (health === '后端不可用') return 'unavailable'
  if (health === '未连接本地后端') return 'disconnected'
  if (health === '正在加载') return 'loading'
  if (health === '运行正常' || health === '可用' || health.startsWith('可用')) {
    return 'available'
  }
  return 'unknown'
}

function workspaceHealthStatus(health: string, t: SettingsTranslate): WorkspaceHealthStatus {
  const state = workspaceHealthState(health)
  const keyByState: Record<WorkspaceHealthState, string> = {
    available: 'health.available',
    unavailable: 'health.unavailable',
    disconnected: 'health.disconnected',
    loading: 'health.loading',
    unknown: 'health.unknown',
  }
  const tone = state === 'available'
    ? 'success'
    : state === 'unavailable' || state === 'unknown'
      ? 'danger'
      : 'warning'

  return { value: t(keyByState[state]), tone }
}

function workspaceValue(value: string, t: SettingsTranslate) {
  if (value === '尚未读取') return t('workspace.valueNotRead')
  if (value === '未创建备份') return t('workspace.noBackup')
  return value || t('workspace.valueUnavailable')
}

const SettingsPageWithTestUtils = Object.assign(SettingsPage, {
  testUtils: { settingsApiDialogReducer },
})

export default SettingsPageWithTestUtils
