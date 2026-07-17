import { Bot, Check, ChevronLeft, KeyRound, Pencil, Plus, RefreshCw,
  Server, ShieldCheck, Trash2, X } from 'lucide-react'
import { useEffect, useReducer, useRef, useState, type FormEvent } from 'react'
import type {
  AiApiFormat, AiProviderType, ApiProfileKind, ApiProfileStatus,
  ApiProfileView, SaveApiProfileInput,
} from './api-profiles'
import { useApiProfiles } from './use-api-profiles'
import './ApiProfilesDialog.css'
type ApiProfilesDialogProps = {
  isOpen: boolean
  kind: ApiProfileKind
  onClose: () => void
}
type TikhubProfileDraft = {
  kind: 'tikhub'; id: string | null; name: string; baseUrl: string; apiKey: string
}
type AiProfileDraft = {
  kind: 'ai'; id: string | null; name: string; providerType: AiProviderType
  apiFormat: AiApiFormat; baseUrl: string; defaultModelId: string; apiKey: string
}
export type ApiProfileDraft = TikhubProfileDraft | AiProfileDraft
export type ApiProfilesDialogState = {
  view: 'list' | 'form'; editingProfileId: string | null
  confirmingDeleteId: string | null
}
type ApiProfilesDialogAction =
  | { type: 'add' }
  | { type: 'edit'; profileId: string }
  | { type: 'showList' }
  | { type: 'requestDelete'; profileId: string }
  | { type: 'cancelDelete' }
type DialogFeedback = {
  tone: 'success' | 'warning' | 'danger'; text: string
} | null
const KIND_COPY = {
  tikhub: { eyebrow: 'TikHub API', noun: 'TikHub', empty: '尚未保存 TikHub API 配置' },
  ai: { eyebrow: 'AI API', noun: 'AI', empty: '尚未保存 AI API 配置' },
} as const
const STATUS_COPY: Record<ApiProfileStatus, { label: string; tone: string }> = {
  needs_rebind: { label: '需重新输入', tone: 'warning' },
  untested: { label: '待测试', tone: 'neutral' },
  success: { label: '已验证', tone: 'success' },
  failed: { label: '测试失败', tone: 'danger' },
}
type AiProviderPreset = { label: string; apiFormat: AiApiFormat; baseUrl: string }
const AI_PROVIDER_PRESETS: Record<AiProviderType, AiProviderPreset> = {
  openai: { label: 'OpenAI', apiFormat: 'openai_compatible', baseUrl: 'https://api.openai.com/v1' },
  anthropic: { label: 'Anthropic', apiFormat: 'anthropic_messages', baseUrl: 'https://api.anthropic.com' },
  gemini: { label: 'Google Gemini', apiFormat: 'gemini', baseUrl: 'https://generativelanguage.googleapis.com' },
  custom_openai_compatible: { label: '自定义 OpenAI-compatible', apiFormat: 'openai_compatible', baseUrl: '' },
  ollama: { label: 'Ollama', apiFormat: 'ollama', baseUrl: 'http://127.0.0.1:11434' },
}
const API_FORMAT_LABELS: Record<AiApiFormat, string> = {
  openai_compatible: 'OpenAI-compatible',
  anthropic_messages: 'Anthropic Messages',
  gemini: 'Gemini 原生格式',
  ollama: 'Ollama 本地格式',
}
const initialApiProfilesDialogState: ApiProfilesDialogState = {
  view: 'list', editingProfileId: null, confirmingDeleteId: null,
}
function apiProfilesDialogReducer(
  state: ApiProfilesDialogState,
  action: ApiProfilesDialogAction,
): ApiProfilesDialogState {
  switch (action.type) {
    case 'add':
      return { view: 'form', editingProfileId: null, confirmingDeleteId: null }
    case 'edit':
      return { view: 'form', editingProfileId: action.profileId, confirmingDeleteId: null }
    case 'requestDelete':
      return { ...state, confirmingDeleteId: action.profileId }
    case 'cancelDelete':
      return { ...state, confirmingDeleteId: null }
    case 'showList':
      return initialApiProfilesDialogState
  }
}
function createProfileDraft(
  kind: ApiProfileKind,
  profile?: ApiProfileView | null,
): ApiProfileDraft {
  if (kind === 'tikhub') {
    const saved = profile?.kind === 'tikhub' ? profile : null
    return {
      kind,
      id: saved?.id ?? null,
      name: saved?.name ?? '',
      baseUrl: saved?.baseUrl ?? 'https://api.tikhub.io',
      apiKey: '',
    }
  }
  const saved = profile?.kind === 'ai' ? profile : null
  const preset = AI_PROVIDER_PRESETS[saved?.providerType ?? 'openai']
  return {
    kind,
    id: saved?.id ?? null,
    name: saved?.name ?? '',
    providerType: saved?.providerType ?? 'openai',
    apiFormat: saved?.apiFormat ?? preset.apiFormat,
    baseUrl: saved?.baseUrl ?? preset.baseUrl,
    defaultModelId: saved?.defaultModelId ?? '',
    apiKey: '',
  }
}
function buildSaveProfileInput(
  draft: ApiProfileDraft,
  profile?: ApiProfileView | null,
): SaveApiProfileInput {
  const id = profile?.id ?? draft.id
  const apiKey = draft.apiKey.trim()
  const common = {
    ...(id ? { id } : {}),
    name: draft.name.trim(),
    baseUrl: draft.baseUrl.trim(),
    ...(apiKey ? { apiKey } : {}),
  }
  if (draft.kind === 'tikhub') {
    return { kind: 'tikhub', ...common }
  }
  return {
    kind: 'ai',
    ...common,
    providerType: draft.providerType,
    apiFormat: draft.apiFormat,
    defaultModelId: draft.defaultModelId.trim(),
  }
}
function ApiProfilesDialog({ isOpen, kind, onClose }: ApiProfilesDialogProps) {
  const controller = useApiProfiles()
  const [state, dispatch] = useReducer(
    apiProfilesDialogReducer,
    initialApiProfilesDialogState,
  )
  const [draft, setDraft] = useState<ApiProfileDraft>(() => createProfileDraft(kind))
  const [feedback, setFeedback] = useState<DialogFeedback>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  const copy = KIND_COPY[kind]
  const profiles: ApiProfileView[] = kind === 'tikhub'
    ? controller.registry?.tikhubProfiles ?? []
    : controller.registry?.aiProfiles ?? []
  const editingProfile = state.editingProfileId
    ? profiles.find((profile) => profile.id === state.editingProfileId) ?? null
    : null
  const canSubmit = state.view === 'form'
    && canSaveProfile(draft, editingProfile)
  useEffect(() => {
    dispatch({ type: 'showList' })
    setDraft(createProfileDraft(kind))
    setFeedback(null)
  }, [isOpen, kind])
  useEffect(() => {
    if (!isOpen) return
    const previouslyFocused = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null
    const frame = window.requestAnimationFrame(() => dialogRef.current?.focus())
    return () => {
      window.cancelAnimationFrame(frame)
      previouslyFocused?.focus()
    }
  }, [isOpen])
  useEffect(() => {
    if (!isOpen || state.view !== 'form') return
    const frame = window.requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLElement>('[data-dialog-initial-focus]')
        ?.focus()
    })
    return () => window.cancelAnimationFrame(frame)
  }, [isOpen, state.editingProfileId, state.view])
  useEffect(() => {
    if (!isOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        if (!controller.isPending) onClose()
        return
      }
      if (event.key !== 'Tab') return
      const focusable = getFocusableElements(dialogRef.current)
      if (focusable.length === 0) {
        event.preventDefault()
        dialogRef.current?.focus()
        return
      }
      const first = focusable[0]
      const last = focusable[focusable.length - 1]
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault()
        first.focus()
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [controller.isPending, isOpen, onClose])
  if (!isOpen) return null
  const showAddForm = () => {
    setDraft(createProfileDraft(kind))
    setFeedback(null)
    dispatch({ type: 'add' })
  }
  const showEditForm = (profile: ApiProfileView) => {
    setDraft(createProfileDraft(kind, profile))
    setFeedback(null)
    dispatch({ type: 'edit', profileId: profile.id })
  }
  const showList = () => {
    setDraft(createProfileDraft(kind))
    setFeedback(null)
    dispatch({ type: 'showList' })
  }
  const saveProfile = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (!canSubmit || controller.isPending) return
    try {
      const result = await controller.saveAndTestProfile(
        buildSaveProfileInput(draft, editingProfile),
      )
      setDraft(createProfileDraft(kind))
      dispatch({ type: 'showList' })
      setFeedback({
        tone: result.success ? 'success' : 'warning',
        text: result.message,
      })
    } catch {
      setFeedback({
        tone: 'danger',
        text: '保存或验证失败，请检查当前字段后重试。',
      })
    }
  }
  const retest = async (profile: ApiProfileView) => {
    try {
      const result = await controller.retestProfile(kind, profile.id)
      setFeedback({
        tone: result.success ? 'success' : 'warning',
        text: result.message,
      })
    } catch {
      setFeedback({ tone: 'danger', text: '重新测试失败，请编辑配置后重试。' })
    }
  }
  const activate = async (profile: ApiProfileView) => {
    try {
      await controller.activateProfile(kind, profile.id)
      setFeedback({ tone: 'success', text: `“${profile.name}”已设为当前配置。` })
    } catch {
      setFeedback({ tone: 'danger', text: '切换失败，只有已验证配置可以设为当前。' })
    }
  }
  const remove = async (profile: ApiProfileView) => {
    try {
      await controller.deleteProfile(kind, profile.id)
      dispatch({ type: 'cancelDelete' })
      setFeedback({ tone: 'success', text: `“${profile.name}”已删除。` })
    } catch {
      setFeedback({
        tone: 'danger',
        text: '删除失败，运行中或恢复中的任务可能仍在使用此配置。',
      })
    }
  }
  const title = state.view === 'list'
    ? `管理 ${copy.noun} API 配置`
    : `${editingProfile ? '编辑' : '新增'} ${copy.noun} API 配置`

  return (
    <div
      className="api-profile-dialog__backdrop"
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget && !controller.isPending) onClose()
      }}
    >
      <div
        ref={dialogRef}
        aria-busy={controller.isPending}
        aria-describedby="api-profile-dialog-description"
        aria-labelledby="api-profile-dialog-title"
        aria-modal="true"
        className="api-profile-dialog"
        role="dialog"
        tabIndex={-1}
      >
        <header className="api-profile-dialog__header">
          <div className="api-profile-dialog__heading">
            <span className="api-profile-dialog__kind-icon" aria-hidden="true">
              {kind === 'tikhub' ? <Server size={17} /> : <Bot size={17} />}
            </span>
            <div>
              <p className="eyebrow">{copy.eyebrow}</p>
              <h2 id="api-profile-dialog-title">{title}</h2>
              <p id="api-profile-dialog-description">
                {state.view === 'list'
                  ? '查看保存项，再选择编辑、验证或切换当前配置。'
                  : '完整密钥只在当前密码输入框的本地状态中短暂存在。'}
              </p>
            </div>
          </div>
          <button aria-label={`关闭 ${copy.noun} API 配置弹窗`}
            className="api-profile-dialog__icon-button" disabled={controller.isPending}
            type="button" onClick={onClose}>
            <X size={17} aria-hidden="true" />
          </button>
        </header>
        <main className="api-profile-dialog__body" data-view={state.view}>
          {feedback ? (
            <p
              className="api-profile-dialog__feedback"
              data-tone={feedback.tone}
              role={feedback.tone === 'danger' ? 'alert' : 'status'}
            >
              {feedback.text}
            </p>
          ) : null}
          {state.view === 'list' ? (
            <ProfileList
              activeProfileId={controller.registry?.activeProfileIds[kind] ?? null}
              busy={controller.isPending}
              confirmingDeleteId={state.confirmingDeleteId}
              emptyCopy={copy.empty}
              kind={kind}
              loading={controller.registryQuery.isLoading}
              loadError={Boolean(controller.registryQuery.error)}
              profiles={profiles}
              onActivate={(profile) => void activate(profile)}
              onCancelDelete={() => dispatch({ type: 'cancelDelete' })}
              onConfirmDelete={(profile) => void remove(profile)}
              onEdit={showEditForm}
              onRefresh={() => void controller.refreshProfiles()}
              onRequestDelete={(profileId) => dispatch({ type: 'requestDelete', profileId })}
              onRetest={(profile) => void retest(profile)}
            />
          ) : (
            <form className="api-profile-form" onSubmit={(event) => void saveProfile(event)}>
              <ApiProfileFormFields
                canKeepSavedKey={Boolean(
                  editingProfile?.hasCredential && editingProfile.status !== 'needs_rebind',
                )}
                disabled={controller.isPending}
                draft={draft}
                isEditing={Boolean(editingProfile)}
                onChange={setDraft}
              />
              <SecurityNotice kind={kind} />
            </form>
          )}
        </main>
        <footer className="api-profile-dialog__footer">
          {state.view === 'list' ? (
            <>
              <SecurityNotice kind={kind} compact />
              <button className="primary-button" data-dialog-initial-focus="true"
                disabled={controller.isPending} type="button" onClick={showAddForm}>
                <Plus size={16} aria-hidden="true" />
                新增 {copy.noun} 配置
              </button>
            </>
          ) : (
            <>
              <button className="ghost-button" disabled={controller.isPending}
                type="button" onClick={showList}>
                <ChevronLeft size={16} aria-hidden="true" />
                返回列表
              </button>
              <button
                className="primary-button"
                disabled={controller.isPending || !canSubmit}
                type="submit"
                onClick={() => {
                  dialogRef.current?.querySelector<HTMLFormElement>('form')?.requestSubmit()
                }}
              >
                <KeyRound size={16} aria-hidden="true" />
                {controller.isSaving ? '正在保存并测试' : '保存并测试'}
              </button>
            </>
          )}
        </footer>
      </div>
    </div>
  )
}

type ProfileListProps = {
  activeProfileId: string | null; busy: boolean; confirmingDeleteId: string | null
  emptyCopy: string; kind: ApiProfileKind; loading: boolean; loadError: boolean
  profiles: ApiProfileView[]
  onActivate: (profile: ApiProfileView) => void; onCancelDelete: () => void
  onConfirmDelete: (profile: ApiProfileView) => void
  onEdit: (profile: ApiProfileView) => void; onRefresh: () => void
  onRequestDelete: (profileId: string) => void; onRetest: (profile: ApiProfileView) => void
}
function ProfileList({
  activeProfileId, busy, confirmingDeleteId, emptyCopy, kind, loading, loadError,
  profiles, onActivate, onCancelDelete, onConfirmDelete, onEdit, onRefresh,
  onRequestDelete, onRetest,
}: ProfileListProps) {
  if (loading) {
    return (
      <div className="api-profile-dialog__state" role="status">
        <RefreshCw className="api-profile-dialog__loading-icon" size={18} aria-hidden="true" />
        <strong>正在读取保存的配置</strong>
        <p>读取完成前不会显示最终空状态。</p>
      </div>
    )
  }
  if (loadError) {
    return (
      <div className="api-profile-dialog__state" role="alert">
        <strong>无法读取 API 配置</strong>
        <p>历史数据仍可访问，请确认私有 JSON 文件完整后重试。</p>
        <button className="ghost-button" disabled={busy} type="button" onClick={onRefresh}>
          <RefreshCw size={16} aria-hidden="true" />
          重新读取
        </button>
      </div>
    )
  }
  if (profiles.length === 0) {
    return (
      <div className="api-profile-dialog__state api-profile-dialog__state--empty">
        <span aria-hidden="true">{kind === 'tikhub' ? <Server size={20} /> : <Bot size={20} />}</span>
        <strong>{emptyCopy}</strong>
        <p>先返回列表确认当前为空，再从下方新增第一条命名配置。</p>
      </div>
    )
  }
  return (
    <section aria-labelledby="api-profile-saved-list-title">
      <div className="api-profile-dialog__list-heading">
        <div>
          <p className="eyebrow">已保存配置</p>
          <h3 id="api-profile-saved-list-title">
            {profiles.length} 条可管理配置
          </h3>
        </div>
        <span>{activeProfileId ? '已选择当前配置' : '尚未选择当前配置'}</span>
      </div>
      <div className="api-profile-list">
        {profiles.map((profile) => {
          const isActive = profile.isActive || profile.id === activeProfileId
          const status = STATUS_COPY[profile.status]
          const isConfirmingDelete = confirmingDeleteId === profile.id
          return (
            <article className="api-profile-list__item" data-active={isActive} key={profile.id}>
              <header className="api-profile-list__identity">
                <span className="api-profile-list__provider-icon" aria-hidden="true">
                  {profile.kind === 'tikhub' ? <Server size={16} /> : <Bot size={16} />}
                </span>
                <div>
                  <div className="api-profile-list__title-row">
                    <h3>{profile.name}</h3>
                    {isActive ? (
                      <span className="api-profile-list__current">
                        <Check size={12} aria-hidden="true" />
                        当前
                      </span>
                    ) : null}
                  </div>
                  <p>{profileDescriptor(profile)}</p>
                </div>
                <span className="api-profile-list__status" data-tone={status.tone}>
                  {status.label}
                </span>
              </header>
              <dl className="api-profile-list__facts">
                <div>
                  <dt>{profile.kind === 'tikhub' ? 'API 端点' : 'Base URL'}</dt>
                  <dd>{profile.baseUrl}</dd>
                </div>
                {profile.kind === 'ai' ? (
                  <div>
                    <dt>默认模型</dt>
                    <dd>{profile.defaultModelId}</dd>
                  </div>
                ) : null}
                <div>
                  <dt>密钥</dt>
                  <dd className="api-profile-list__masked-key">
                    {maskedKeyLabel(profile)}
                  </dd>
                </div>
              </dl>
              <div className="api-profile-list__actions">
                <button disabled={busy} type="button" onClick={() => onEdit(profile)}>
                  <Pencil size={15} aria-hidden="true" />
                  编辑
                </button>
                <button disabled={busy || profile.status === 'needs_rebind'}
                  type="button" onClick={() => onRetest(profile)}>
                  <RefreshCw size={15} aria-hidden="true" />
                  重新测试
                </button>
                <button disabled={busy || isActive || profile.status !== 'success'}
                  type="button" onClick={() => onActivate(profile)}>
                  <Check size={15} aria-hidden="true" />
                  {isActive ? '当前使用' : '设为当前'}
                </button>
                <button className="api-profile-list__delete-button" disabled={busy}
                  type="button" onClick={() => onRequestDelete(profile.id)}>
                  <Trash2 size={15} aria-hidden="true" />
                  删除
                </button>
              </div>
              {isConfirmingDelete ? (
                <div className="api-profile-list__delete-confirm" role="alert">
                  <div>
                    <strong>确认删除“{profile.name}”？</strong>
                    <p>
                      {isActive
                        ? '删除当前配置后不会自动选择其他配置。'
                        : '配置与对应凭据会从当前工作区私有 JSON 中移除。'}
                    </p>
                  </div>
                  <div>
                    <button disabled={busy} type="button" onClick={onCancelDelete}>
                      取消
                    </button>
                    <button
                      className="api-profile-list__confirm-delete"
                      disabled={busy}
                      type="button"
                      onClick={() => onConfirmDelete(profile)}
                    >
                      确认删除
                    </button>
                  </div>
                </div>
              ) : null}
            </article>
          )
        })}
      </div>
    </section>
  )
}
export function ApiProfileFormFields({
  disabled, draft, isEditing, canKeepSavedKey = isEditing, onChange,
}: {
  canKeepSavedKey?: boolean; disabled: boolean; draft: ApiProfileDraft; isEditing: boolean
  onChange: (draft: ApiProfileDraft) => void
}) {
  const keyRequired = requiresApiKey(draft, canKeepSavedKey)
  return (
    <div className="api-profile-form__fields">
      <label className="api-profile-form__field">
        <span>配置名称</span>
        <input
          data-dialog-initial-focus="true"
          disabled={disabled}
          maxLength={80}
          placeholder={draft.kind === 'tikhub' ? '例如：主数据账号' : '例如：内容整理模型'}
          required
          value={draft.name}
          onChange={(event) => onChange({ ...draft, name: event.target.value })}
        />
        <small>名称在同一配置类型内必须唯一。</small>
      </label>
      {draft.kind === 'tikhub' ? (
        <label className="api-profile-form__field">
          <span>API 端点</span>
          <select disabled={disabled} value={draft.baseUrl}
            onChange={(event) => onChange({ ...draft, baseUrl: event.target.value })}>
            <option value="https://api.tikhub.io">国际端点 api.tikhub.io</option>
            <option value="https://api.tikhub.dev">中国大陆端点 api.tikhub.dev</option>
          </select>
        </label>
      ) : (
        <>
          <label className="api-profile-form__field">
            <span>供应商类型</span>
            <select
              disabled={disabled}
              value={draft.providerType}
              onChange={(event) => {
                const providerType = event.target.value as AiProviderType
                const preset = AI_PROVIDER_PRESETS[providerType]
                onChange({
                  ...draft,
                  providerType,
                  apiFormat: preset.apiFormat,
                  baseUrl: preset.baseUrl,
                })
              }}
            >
              {Object.entries(AI_PROVIDER_PRESETS).map(([value, preset]) => (
                <option key={value} value={value}>{preset.label}</option>
              ))}
            </select>
          </label>
          <label className="api-profile-form__field">
            <span>API 格式</span>
            <select disabled value={draft.apiFormat} aria-describedby="api-format-hint">
              {Object.entries(API_FORMAT_LABELS).map(([value, label]) => (
                <option key={value} value={value}>{label}</option>
              ))}
            </select>
            <small id="api-format-hint">根据供应商类型自动确定，避免协议不匹配。</small>
          </label>
          <label className="api-profile-form__field api-profile-form__field--wide">
            <span>Base URL</span>
            <input
              disabled={disabled}
              placeholder="https://api.example.com/v1"
              required
              type="url"
              value={draft.baseUrl}
              onChange={(event) => onChange({ ...draft, baseUrl: event.target.value })}
            />
          </label>
          <label className="api-profile-form__field api-profile-form__field--wide">
            <span>默认模型 ID</span>
            <input
              disabled={disabled}
              maxLength={160}
              placeholder="例如：gpt-4.1-mini"
              required
              value={draft.defaultModelId}
              onChange={(event) => onChange({ ...draft, defaultModelId: event.target.value })}
            />
          </label>
        </>
      )}
      <label className="api-profile-form__field api-profile-form__field--wide">
        <span>{draft.kind === 'tikhub' ? 'API Token' : 'API Key'}</span>
        <input
          aria-describedby="api-key-hint"
          autoComplete="new-password"
          disabled={disabled}
          minLength={keyRequired ? 8 : undefined}
          placeholder={keyPlaceholder(draft, canKeepSavedKey)}
          required={keyRequired}
          type="password"
          value={draft.apiKey}
          onChange={(event) => onChange({ ...draft, apiKey: event.target.value })}
        />
        <small id="api-key-hint">
          {canKeepSavedKey
            ? '留空会保留原密钥，完整值不会回显或复制。'
            : draft.kind === 'ai' && draft.providerType === 'ollama'
              ? 'Ollama 可不填写密钥。'
              : '至少输入 8 个字符，保存后界面只显示脱敏值。'}
        </small>
      </label>
    </div>
  )
}
function SecurityNotice({ compact = false, kind }: {
  compact?: boolean; kind: ApiProfileKind
}) {
  return (
    <div className="api-profile-dialog__security" data-compact={compact}>
      <ShieldCheck size={16} aria-hidden="true" />
      <p>
        密钥以明文写入当前工作区私有 JSON，不进入数据库、日志、导出或 Webhook。
        {kind === 'ai' ? ' AI 配置只做完整性校验，当前规则引擎不会调用模型。' : ''}
      </p>
    </div>
  )
}
function canSaveProfile(
  draft: ApiProfileDraft,
  profile: ApiProfileView | null,
) {
  if (!draft.name.trim() || !isSupportedUrl(draft.baseUrl)) return false
  const editingHasReusableKey = Boolean(
    profile?.hasCredential && profile.status !== 'needs_rebind',
  )
  const hasRequiredKey = draft.apiKey.trim().length >= 8 || editingHasReusableKey
  if (draft.kind === 'tikhub') {
    return [
      'https://api.tikhub.io',
      'https://api.tikhub.dev',
    ].includes(draft.baseUrl.trim()) && hasRequiredKey
  }
  if (!draft.defaultModelId.trim()) return false
  return draft.providerType === 'ollama' || hasRequiredKey
}
function requiresApiKey(draft: ApiProfileDraft, canKeepSavedKey: boolean) {
  return draft.kind === 'tikhub'
    ? !canKeepSavedKey
    : draft.providerType !== 'ollama' && !canKeepSavedKey
}
function keyPlaceholder(draft: ApiProfileDraft, canKeepSavedKey: boolean) {
  if (canKeepSavedKey) return '留空以保留已保存密钥'
  if (draft.kind === 'ai' && draft.providerType === 'ollama') return 'Ollama 可不填写'
  return draft.kind === 'tikhub' ? '重新输入 TikHub Token' : '重新输入供应商 API Key'
}
function isSupportedUrl(value: string) {
  try {
    const url = new URL(value.trim())
    return url.protocol === 'https:' || url.protocol === 'http:'
  } catch {
    return false
  }
}
function profileDescriptor(profile: ApiProfileView) {
  if (profile.kind === 'tikhub') {
    return profile.testSummary?.maskedAccount ?? 'TikHub 数据接口账号'
  }
  return `${AI_PROVIDER_PRESETS[profile.providerType].label} · ${API_FORMAT_LABELS[profile.apiFormat]}`
}
function maskedKeyLabel(profile: ApiProfileView) {
  if (profile.maskedKey) return profile.maskedKey
  if (profile.kind === 'ai' && profile.providerType === 'ollama') return '无需密钥'
  return profile.status === 'needs_rebind' ? '需重新输入' : '尚未绑定'
}
function getFocusableElements(container: HTMLElement | null) {
  if (!container) return []
  return Array.from(container.querySelectorAll<HTMLElement>([
    'button:not([disabled])',
    'input:not([disabled])',
    'select:not([disabled])',
    'textarea:not([disabled])',
    '[href]',
    '[tabindex]:not([tabindex="-1"])',
  ].join(','))).filter((element) => !element.hasAttribute('hidden'))
}
const ApiProfilesDialogWithTestUtils = Object.assign(ApiProfilesDialog, {
  testUtils: {
    apiProfilesDialogReducer,
    buildSaveProfileInput,
    createProfileDraft,
    initialApiProfilesDialogState,
  },
})

export default ApiProfilesDialogWithTestUtils
