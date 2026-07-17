import { HardDrive, KeyRound, Sparkles } from 'lucide-react'
import { StatusPill } from './CollectionBuilder'
import ModelSettingsPanel from './ModelSettingsPanel'
import TikhubSettingsPanel from './TikhubSettingsPanel'
import UpdateSettingsPanel from './UpdateSettingsPanel'
import type { useWorkbenchBackend } from './use-workbench-backend'
import './SettingsPage.css'

type SettingsBackend = ReturnType<typeof useWorkbenchBackend>

function SettingsPage({ backend }: { backend: SettingsBackend }) {
  const data = backend.data
  const hasTikhub = Boolean(data.tikhubConnector?.secret_ref_id)
  const activeModel = data.modelProviders.find((provider) => provider.enabled)

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
            value={hasTikhub ? 'TikHub 已保存' : 'TikHub 待配置'}
            tone={hasTikhub ? 'success' : 'warning'}
          />
          <SettingsStatus
            label="AI 处理"
            value={activeModel ? `${activeModel.display_name} 当前使用` : '模型 API 待配置'}
            tone={activeModel ? 'success' : 'warning'}
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
        eyebrow="数据来源"
        title="TikHub"
        description="管理数据 API、账号状态和可用额度。"
      >
        <TikhubSettingsPanel
          connector={data.tikhubConnector}
          isBusy={backend.isBusy}
          result={backend.tikhubTestResult}
          onSaveAndTest={backend.saveAndTestTikhubToken}
        />
      </SettingsGroup>

      <SettingsGroup
        icon={Sparkles}
        eyebrow="AI 处理"
        title="模型供应商"
        description="配置结构化输出使用的模型、地址和安全密钥。"
      >
        <ModelSettingsPanel
          {...backend}
          isPending={backend.isModelSettingsPending}
          providers={data.modelProviders}
          result={backend.modelValidationResult}
        />
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
    </div>
  )
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

export default SettingsPage
