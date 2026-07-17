import { Bot, ChevronRight, HardDrive, KeyRound, Server } from 'lucide-react'
import { useReducer } from 'react'
import ApiProfilesDialog from './ApiProfilesDialog'
import { StatusPill } from './CollectionBuilder'
import UpdateSettingsPanel from './UpdateSettingsPanel'
import type {
  ApiProfileKind,
  ApiProfileRegistryView,
  ApiProfileView,
} from './api-profiles'
import { useApiProfiles } from './use-api-profiles'
import type { useWorkbenchBackend } from './use-workbench-backend'
import './SettingsPage.css'

type SettingsBackend = ReturnType<typeof useWorkbenchBackend>
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
  )
  const aiStatus = apiProfileStatus(
    'ai',
    apiProfiles.registry,
    apiProfiles.registryQuery.isLoading,
    Boolean(apiProfiles.registryQuery.error),
  )

  return (
    <div className="settings-page">
      <section className="settings-page__status" aria-labelledby="settings-status-heading">
        <header>
          <p className="eyebrow">配置状态</p>
          <h2 id="settings-status-heading">当前工作区能力</h2>
        </header>
        <dl>
          <SettingsStatus
            label="数据来源"
            value={tikhubStatus.value}
            tone={tikhubStatus.tone}
          />
          <SettingsStatus
            label="AI API"
            value={aiStatus.value}
            tone={aiStatus.tone}
          />
          <SettingsStatus
            label="本地后端"
            value={data.workspace.health}
            tone={toneForHealth(data.workspace.health)}
          />
        </dl>
      </section>

      <SettingsGroup
        icon={KeyRound}
        eyebrow="API 配置"
        title="数据来源与 AI"
        description="在独立列表中查看、验证和切换当前配置。"
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
            <span>配置 TikHub API</span>
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
            <span>配置 AI API</span>
            <ChevronRight size={16} aria-hidden="true" />
          </button>
        </div>
      </SettingsGroup>

      <SettingsGroup
        icon={HardDrive}
        eyebrow="本地应用"
        title="工作区与更新"
        description="确认数据位置、应用身份和客户端更新状态。"
      >
        <WorkspaceSettings
          health={data.workspace.health}
          lastBackup={data.workspace.lastBackup}
          storage={data.workspace.storage}
        />
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
) {
  const noun = kind === 'tikhub' ? 'TikHub' : 'AI API'
  if (isLoading) return { value: `${noun} 正在读取`, tone: 'warning' }
  if (hasError || !registry) return { value: `${noun} 配置读取失败`, tone: 'danger' }

  const profiles: ApiProfileView[] = kind === 'tikhub'
    ? registry.tikhubProfiles
    : registry.aiProfiles
  const activeProfileId = registry.activeProfileIds[kind]
  const activeProfile = profiles.find((profile) => profile.id === activeProfileId)
  if (activeProfile?.status === 'success') {
    return { value: `${activeProfile.name} 当前配置`, tone: 'success' }
  }
  if (profiles.length === 0) {
    return { value: `${noun} 待配置`, tone: 'warning' }
  }
  if (profiles.some((profile) => profile.status === 'needs_rebind')) {
    return { value: `${noun} 需重新输入`, tone: 'warning' }
  }
  return { value: `${noun} 待验证或选择`, tone: 'warning' }
}

function SettingsStatus({
  label,
  value,
  tone,
}: {
  label: string
  value: string
  tone: string
}) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>{value}</dd>
      <StatusPill tone={tone} label={tone === 'success' ? '可用' : tone === 'danger' ? '异常' : '待完成'} />
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
  health,
  lastBackup,
  storage,
}: {
  health: string
  lastBackup: string
  storage: string
}) {
  return (
    <section className="workspace-settings" aria-labelledby="workspace-settings-heading">
      <header>
        <div>
          <p className="eyebrow">本地工作区</p>
          <h3 id="workspace-settings-heading">应用身份与数据位置</h3>
        </div>
        <StatusPill tone={toneForHealth(health)} label={health} />
      </header>
      <dl>
        <div>
          <dt>本地数据路径</dt>
          <dd>{storage}</dd>
        </div>
        <div>
          <dt>应用标识</dt>
          <dd>com.steven.sortlytic</dd>
        </div>
        <div>
          <dt>工作区目录</dt>
          <dd>default-workspace</dd>
        </div>
        <div>
          <dt>最近备份</dt>
          <dd>{lastBackup}</dd>
        </div>
      </dl>
    </section>
  )
}

function toneForHealth(health: string) {
  if (health === '后端不可用') return 'danger'
  if (health === '未连接本地后端' || health === '正在加载') return 'warning'
  return 'success'
}

const SettingsPageWithTestUtils = Object.assign(SettingsPage, {
  testUtils: { settingsApiDialogReducer },
})

export default SettingsPageWithTestUtils
