import { Bot, ChevronRight, HardDrive, KeyRound, Server } from 'lucide-react'
import { useEffect, useReducer, useState, type CSSProperties } from 'react'
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
  activatePromptVersion,
  createPromptVersion,
  listPromptTemplates,
  listPromptVersions,
  type PromptTemplateView,
  type PromptVersionView,
} from './backend-api'
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
        <PromptSettings />
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

type PromptSettingsPhase = 'loading' | 'ready' | 'empty' | 'error'

const promptSettingsCopy = {
  'zh-CN': {
    eyebrow: 'AI 提示词',
    title: '自然语言采集计划',
    description: '管理发送给当前 AI 模型的计划生成指令。',
    chain: '提示词 → AI 结构化计划 → Schema / 能力校验 → 用户确认 → TikHub 真实 API',
    boundary: '提示词不保存 API Key，也不能绕过预算校验和用户确认。',
    currentVersion: '当前启用版本',
    schema: '输出 Schema',
    versionLabel: '查看或编辑版本',
    editorLabel: '提示词正文',
    noteLabel: '版本说明',
    notePlaceholder: '说明本次修改目的，例如：补充地区与年龄约束',
    loading: '正在读取提示词',
    loadError: '提示词读取失败，请确认本地后端可用后重试。',
    empty: '未找到自然语言采集提示词模板。',
    noActive: '尚无启用版本',
    active: '当前启用 v{{version}}',
    selected: '正在编辑：{{status}} v{{version}}',
    save: '保存为新版本',
    saving: '正在保存',
    activate: '激活当前草稿',
    activating: '正在激活',
    saved: '草稿 v{{version}} 已保存，确认内容后再激活。',
    activated: 'v{{version}} 已通过回归校验并激活。',
    saveError: '新版本保存失败，请检查正文和版本说明。',
    activateError: '激活失败，提示词回归样例可能未通过。',
    status: {
      active: '已启用',
      draft: '草稿',
      archived: '已归档',
      failed_regression: '回归失败',
    },
  },
  'en-US': {
    eyebrow: 'AI prompt',
    title: 'Natural-language collection plan',
    description: 'Manage the planning instructions sent to the active AI model.',
    chain: 'Prompt → AI structured plan → Schema / capability validation → User confirmation → Live TikHub API',
    boundary: 'Prompts never store API keys and cannot bypass budget validation or user confirmation.',
    currentVersion: 'Active version',
    schema: 'Output schema',
    versionLabel: 'View or edit version',
    editorLabel: 'Prompt content',
    noteLabel: 'Version note',
    notePlaceholder: 'Describe the purpose, for example: add region and age constraints',
    loading: 'Loading prompts',
    loadError: 'Prompts could not be loaded. Check the local backend and try again.',
    empty: 'The natural-language collection prompt template was not found.',
    noActive: 'No active version',
    active: 'Active v{{version}}',
    selected: 'Editing: {{status}} v{{version}}',
    save: 'Save as new version',
    saving: 'Saving',
    activate: 'Activate this draft',
    activating: 'Activating',
    saved: 'Draft v{{version}} was saved. Review it before activation.',
    activated: 'v{{version}} passed regression checks and is now active.',
    saveError: 'The new version could not be saved. Check the content and version note.',
    activateError: 'Activation failed. Prompt regression cases may not have passed.',
    status: {
      active: 'Active',
      draft: 'Draft',
      archived: 'Archived',
      failed_regression: 'Regression failed',
    },
  },
} as const

const promptBodyStyle: CSSProperties = {
  display: 'grid',
  gap: 14,
  padding: '16px 18px 18px',
}

const promptChainStyle: CSSProperties = {
  display: 'grid',
  gap: 5,
  margin: 0,
  padding: '12px 14px',
  color: 'var(--text)',
  background: 'var(--surface-raised)',
  border: '1px solid var(--border-subtle)',
  borderRadius: 'var(--radius-md)',
  fontSize: 11,
  lineHeight: 1.6,
}

const promptEditorStyle: CSSProperties = {
  width: '100%',
  minHeight: 190,
  padding: '10px 12px',
  resize: 'vertical',
  color: 'var(--text-strong)',
  background: 'var(--canvas)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-md)',
  fontFamily: 'ui-monospace, "SFMono-Regular", Menlo, Monaco, Consolas, monospace',
  fontSize: 11,
  lineHeight: 1.65,
}

const promptActionsStyle: CSSProperties = {
  display: 'flex',
  flexWrap: 'wrap',
  gap: 8,
}

function PromptSettings() {
  useTranslation('settings')
  const language = normalizeLanguage(i18n.resolvedLanguage)
  const copy = promptSettingsCopy[language]
  const [phase, setPhase] = useState<PromptSettingsPhase>('loading')
  const [template, setTemplate] = useState<PromptTemplateView | null>(null)
  const [versions, setVersions] = useState<PromptVersionView[]>([])
  const [selectedVersionId, setSelectedVersionId] = useState('')
  const [content, setContent] = useState('')
  const [changeNote, setChangeNote] = useState('')
  const [isSaving, setIsSaving] = useState(false)
  const [isActivating, setIsActivating] = useState(false)
  const [feedback, setFeedback] = useState('')

  useEffect(() => {
    let isCurrent = true

    void (async () => {
      try {
        const templates = await listPromptTemplates()
        const collectionTemplate = templates.find(
          (candidate) => candidate.template_key === 'collection_plan_from_text',
        )
        if (!isCurrent) return
        if (!collectionTemplate) {
          setPhase('empty')
          return
        }

        const promptVersions = await listPromptVersions(collectionTemplate.id)
        if (!isCurrent) return
        const initialVersion = promptVersions.find((version) => version.status === 'active')
          ?? promptVersions[0]
        setTemplate(collectionTemplate)
        setVersions(promptVersions)
        setSelectedVersionId(initialVersion?.id ?? '')
        setContent(initialVersion?.content ?? '')
        setPhase(promptVersions.length > 0 ? 'ready' : 'empty')
      } catch {
        if (isCurrent) setPhase('error')
      }
    })()

    return () => {
      isCurrent = false
    }
  }, [])

  const activeVersion = versions.find((version) => version.status === 'active')
  const selectedVersion = versions.find((version) => version.id === selectedVersionId)
  const statusText = phase === 'loading'
    ? copy.loading
    : phase === 'error'
      ? copy.loadError
      : phase === 'empty'
        ? copy.empty
        : activeVersion
          ? interpolate(copy.active, { version: activeVersion.version })
          : copy.noActive
  const statusTone = phase === 'error'
    ? 'danger'
    : activeVersion
      ? 'success'
      : 'warning'

  async function saveVersion() {
    if (!template || !content.trim() || !changeNote.trim() || isSaving) return
    setIsSaving(true)
    setFeedback('')
    try {
      const version = await createPromptVersion({
        template_id: template.id,
        content: content.trim(),
        change_note: changeNote.trim(),
      })
      setVersions((current) => [
        version,
        ...current.filter((candidate) => candidate.id !== version.id),
      ])
      setSelectedVersionId(version.id)
      setContent(version.content)
      setChangeNote('')
      setFeedback(interpolate(copy.saved, { version: version.version }))
    } catch {
      setFeedback(copy.saveError)
    } finally {
      setIsSaving(false)
    }
  }

  async function activateVersion() {
    if (!selectedVersion || selectedVersion.status === 'active' || isActivating) return
    setIsActivating(true)
    setFeedback('')
    try {
      const activated = await activatePromptVersion(selectedVersion.id)
      setVersions((current) => current.map((version) => {
        if (version.id === activated.id) return activated
        return version.status === 'active' ? { ...version, status: 'archived' } : version
      }))
      setSelectedVersionId(activated.id)
      setContent(activated.content)
      setFeedback(interpolate(copy.activated, { version: activated.version }))
    } catch {
      setFeedback(copy.activateError)
    } finally {
      setIsActivating(false)
    }
  }

  return (
    <section className="workspace-settings" aria-labelledby="prompt-settings-heading">
      <header>
        <div>
          <p className="eyebrow">{copy.eyebrow}</p>
          <h3 id="prompt-settings-heading">{copy.title}</h3>
          <p className="muted-text">{copy.description}</p>
        </div>
        <StatusPill tone={statusTone} label={statusText} />
      </header>

      {phase === 'ready' && template ? (
        <>
          <dl>
            <div>
              <dt>{copy.currentVersion}</dt>
              <dd>{activeVersion ? `v${activeVersion.version}` : copy.noActive}</dd>
            </div>
            <div>
              <dt>{copy.schema}</dt>
              <dd>{template.output_schema_id ?? copy.noActive}</dd>
            </div>
          </dl>
          <div style={promptBodyStyle}>
            <p style={promptChainStyle}>
              <strong>{copy.chain}</strong>
              <span>{copy.boundary}</span>
            </p>
            <label className="field">
              <span>{copy.versionLabel}</span>
              <select
                value={selectedVersionId}
                onChange={(event) => {
                  const version = versions.find(
                    (candidate) => candidate.id === event.currentTarget.value,
                  )
                  if (!version) return
                  setSelectedVersionId(version.id)
                  setContent(version.content)
                  setChangeNote('')
                  setFeedback('')
                }}
              >
                {versions.map((version) => (
                  <option key={version.id} value={version.id}>
                    {`v${version.version} · ${promptVersionStatus(version.status, copy.status)}`}
                  </option>
                ))}
              </select>
            </label>
            {selectedVersion ? (
              <p className="muted-text" data-prompt-status>
                {interpolate(copy.selected, {
                  status: promptVersionStatus(selectedVersion.status, copy.status),
                  version: selectedVersion.version,
                })}
              </p>
            ) : null}
            <label className="field">
              <span>{copy.editorLabel}</span>
              <textarea
                data-prompt-content
                style={promptEditorStyle}
                value={content}
                onChange={(event) => setContent(event.currentTarget.value)}
              />
            </label>
            <label className="field">
              <span>{copy.noteLabel}</span>
              <input
                data-prompt-change-note
                value={changeNote}
                placeholder={copy.notePlaceholder}
                onChange={(event) => setChangeNote(event.currentTarget.value)}
              />
            </label>
            <div style={promptActionsStyle}>
              <button
                className="ghost-button"
                type="button"
                disabled={isSaving || !content.trim() || !changeNote.trim()}
                onClick={() => void saveVersion()}
              >
                {isSaving ? copy.saving : copy.save}
              </button>
              <button
                className="primary-button"
                type="button"
                disabled={isActivating || !selectedVersion || selectedVersion.status === 'active'}
                onClick={() => void activateVersion()}
              >
                {isActivating ? copy.activating : copy.activate}
              </button>
            </div>
            <p className="muted-text" aria-live="polite">{feedback}</p>
          </div>
        </>
      ) : (
        <div style={promptBodyStyle}>
          <p className="muted-text" role={phase === 'error' ? 'alert' : undefined}>
            {statusText}
          </p>
        </div>
      )}
    </section>
  )
}

function promptVersionStatus(
  status: string,
  copy: typeof promptSettingsCopy['zh-CN']['status'] | typeof promptSettingsCopy['en-US']['status'],
) {
  return copy[status as keyof typeof copy] ?? status
}

function interpolate(template: string, values: Record<string, string | number>) {
  return Object.entries(values).reduce(
    (result, [key, value]) => result.replace(`{{${key}}}`, String(value)),
    template,
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
  const languageLabel = language === 'zh-CN'
    ? t('language.chinese')
    : t('language.english')
  const options = supportedLanguages.map((value) => ({
    value,
    label: value === 'zh-CN' ? t('language.chinese') : t('language.english'),
    description: value === 'zh-CN' ? 'zh-CN' : 'en-US',
  }))

  return (
    <section
      className="workspace-settings language-settings"
      aria-busy={isChanging}
      aria-labelledby="language-settings-heading"
    >
      <header>
        <div>
          <p className="eyebrow">{t('language.eyebrow')}</p>
          <h3 id="language-settings-heading">{languageLabel}</h3>
        </div>
      </header>
      <div
        className="language-settings__body"
        role="group"
        aria-labelledby="app-language-label"
      >
        <span
          className="language-settings__field-label"
          id="app-language-label"
        >
          {t('language.title')}
        </span>
        <p
          className="language-settings__description muted-text"
          id="app-language-description"
        >
          {t('language.description')}
        </p>
        <AppSelect
          id="app-language"
          ariaDescribedBy="app-language-description"
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
        <span className="language-settings__feedback" aria-live="polite">
          {isChanging ? t('language.switching') : ''}
        </span>
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
