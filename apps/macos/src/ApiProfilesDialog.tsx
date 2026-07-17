import { Bot, Check, ChevronLeft, KeyRound, Pencil, Plus, RefreshCw,
  Server, ShieldCheck, Trash2, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useEffect, useReducer, useRef, useState, type FormEvent } from 'react'
import type {
  AiApiFormat, AiProviderType, ApiProfileKind, ApiProfileStatus,
  ApiProfileView, SaveApiProfileInput,
} from './api-profiles'
import { useApiProfiles } from './use-api-profiles'
import './i18n'
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
  tikhub: { eyebrow: 'kind.tikhub.eyebrow', noun: 'kind.tikhub.noun', empty: 'kind.tikhub.empty' },
  ai: { eyebrow: 'kind.ai.eyebrow', noun: 'kind.ai.noun', empty: 'kind.ai.empty' },
} as const
const STATUS_COPY: Record<ApiProfileStatus, { labelKey: string; tone: string }> = {
  needs_rebind: { labelKey: 'profileStatus.needsRebind', tone: 'warning' },
  untested: { labelKey: 'profileStatus.untested', tone: 'neutral' },
  success: { labelKey: 'profileStatus.success', tone: 'success' },
  failed: { labelKey: 'profileStatus.failed', tone: 'danger' },
}
type AiProviderPreset = { labelKey: string; apiFormat: AiApiFormat; baseUrl: string }
const AI_PROVIDER_PRESETS: Record<AiProviderType, AiProviderPreset> = {
  openai: { labelKey: 'providers.openai', apiFormat: 'openai_compatible', baseUrl: 'https://api.openai.com/v1' },
  anthropic: { labelKey: 'providers.anthropic', apiFormat: 'anthropic_messages', baseUrl: 'https://api.anthropic.com' },
  gemini: { labelKey: 'providers.gemini', apiFormat: 'gemini', baseUrl: 'https://generativelanguage.googleapis.com' },
  custom_openai_compatible: { labelKey: 'providers.customOpenAi', apiFormat: 'openai_compatible', baseUrl: '' },
  ollama: { labelKey: 'providers.ollama', apiFormat: 'ollama', baseUrl: 'http://127.0.0.1:11434' },
}
const API_FORMAT_LABEL_KEYS: Record<AiApiFormat, string> = {
  openai_compatible: 'apiFormats.openAiCompatible',
  anthropic_messages: 'apiFormats.anthropicMessages',
  gemini: 'apiFormats.gemini',
  ollama: 'apiFormats.ollama',
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
  const { t } = useTranslation('settings')
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
        text: result.success
          ? t(kind === 'ai' ? 'feedback.aiSaveSuccess' : 'feedback.saveSuccess')
          : t(kind === 'ai' ? 'feedback.aiSaveFailed' : 'feedback.saveFailed'),
      })
    } catch {
      setFeedback({
        tone: 'danger',
        text: t('feedback.saveError'),
      })
    }
  }
  const retest = async (profile: ApiProfileView) => {
    try {
      const result = await controller.retestProfile(kind, profile.id)
      setFeedback({
        tone: result.success ? 'success' : 'warning',
        text: result.success
          ? t(kind === 'ai' ? 'feedback.aiValidationSuccess' : 'feedback.testSuccess')
          : t(kind === 'ai' ? 'feedback.aiValidationFailed' : 'feedback.testFailed'),
      })
    } catch {
      setFeedback({
        tone: 'danger',
        text: t(kind === 'ai' ? 'feedback.aiValidationError' : 'feedback.testError'),
      })
    }
  }
  const activate = async (profile: ApiProfileView) => {
    try {
      await controller.activateProfile(kind, profile.id)
      setFeedback({ tone: 'success', text: t('feedback.activateSuccess', { name: profile.name }) })
    } catch {
      setFeedback({ tone: 'danger', text: t('feedback.activateError') })
    }
  }
  const remove = async (profile: ApiProfileView) => {
    try {
      await controller.deleteProfile(kind, profile.id)
      dispatch({ type: 'cancelDelete' })
      setFeedback({ tone: 'success', text: t('feedback.deleteSuccess', { name: profile.name }) })
    } catch {
      setFeedback({
        tone: 'danger',
        text: t('feedback.deleteError'),
      })
    }
  }
  const title = state.view === 'list'
    ? t('dialog.manageTitle', { kind: t(copy.noun) })
    : t(editingProfile ? 'dialog.editTitle' : 'dialog.addTitle', { kind: t(copy.noun) })

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
              <p className="eyebrow">{t(copy.eyebrow)}</p>
              <h2 id="api-profile-dialog-title">{title}</h2>
              <p id="api-profile-dialog-description">
                {state.view === 'list'
                  ? t('dialog.listDescription')
                  : t('dialog.formDescription')}
              </p>
            </div>
          </div>
          <button aria-label={t('dialog.close', { kind: t(copy.noun) })}
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
              emptyCopy={t(copy.empty)}
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
                canKeepSavedKey={canReuseSavedCredential(draft, editingProfile)}
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
                {t('dialog.addButton', { kind: t(copy.noun) })}
              </button>
            </>
          ) : (
            <>
              <button className="ghost-button" disabled={controller.isPending}
                type="button" onClick={showList}>
                <ChevronLeft size={16} aria-hidden="true" />
                {t('form.backToList')}
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
                {controller.isSaving
                  ? t(kind === 'ai' ? 'form.savingAndValidating' : 'form.saving')
                  : t(kind === 'ai' ? 'form.saveAndValidate' : 'form.save')}
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
  const { t } = useTranslation('settings')
  if (loading) {
    return (
      <div className="api-profile-dialog__state" role="status">
        <RefreshCw className="api-profile-dialog__loading-icon" size={18} aria-hidden="true" />
        <strong>{t('list.loadingTitle')}</strong>
        <p>{t('list.loadingDescription')}</p>
      </div>
    )
  }
  if (loadError) {
    return (
      <div className="api-profile-dialog__state" role="alert">
        <strong>{t('list.loadErrorTitle')}</strong>
        <p>{t('list.loadErrorDescription')}</p>
        <button className="ghost-button" disabled={busy} type="button" onClick={onRefresh}>
          <RefreshCw size={16} aria-hidden="true" />
          {t('list.refresh')}
        </button>
      </div>
    )
  }
  if (profiles.length === 0) {
    return (
      <div className="api-profile-dialog__state api-profile-dialog__state--empty">
        <span aria-hidden="true">{kind === 'tikhub' ? <Server size={20} /> : <Bot size={20} />}</span>
        <strong>{emptyCopy}</strong>
        <p>{t('list.emptyDescription')}</p>
      </div>
    )
  }
  return (
    <section aria-labelledby="api-profile-saved-list-title">
      <div className="api-profile-dialog__list-heading">
        <div>
          <p className="eyebrow">{t('list.savedEyebrow')}</p>
          <h3 id="api-profile-saved-list-title">
            {t('list.profileCount', { count: profiles.length })}
          </h3>
        </div>
        <span>{activeProfileId ? t('list.activeSelected') : t('list.activeNotSelected')}</span>
      </div>
      <div className="api-profile-list">
        {profiles.map((profile) => {
          const isActive = profile.isActive || profile.id === activeProfileId
          const status = STATUS_COPY[profile.status]
          const statusLabelKey = profile.kind === 'ai' && profile.status === 'success'
            ? 'profileStatus.aiValidated'
            : status.labelKey
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
                        {t('list.current')}
                      </span>
                    ) : null}
                  </div>
                  <p>{profileDescriptor(profile, t)}</p>
                </div>
                <span className="api-profile-list__status" data-tone={status.tone}>
                  {t(statusLabelKey)}
                </span>
              </header>
              <dl className="api-profile-list__facts">
                <div>
                  <dt>{profile.kind === 'tikhub' ? t('form.apiEndpoint') : t('form.baseUrl')}</dt>
                  <dd>{profile.baseUrl}</dd>
                </div>
                {profile.kind === 'ai' ? (
                  <div>
                    <dt>{t('form.defaultModel')}</dt>
                    <dd>{profile.defaultModelId}</dd>
                  </div>
                ) : null}
                <div>
                  <dt>{t('form.credential')}</dt>
                  <dd className="api-profile-list__masked-key">
                    {maskedKeyLabel(profile, t)}
                  </dd>
                </div>
              </dl>
              <div className="api-profile-list__actions">
                <button disabled={busy} type="button" onClick={() => onEdit(profile)}>
                  <Pencil size={15} aria-hidden="true" />
                  {t('list.edit')}
                </button>
                <button disabled={busy || profile.status === 'needs_rebind'}
                  type="button" onClick={() => onRetest(profile)}>
                  <RefreshCw size={15} aria-hidden="true" />
                  {t(profile.kind === 'ai' ? 'list.revalidate' : 'list.retest')}
                </button>
                <button disabled={busy || isActive || profile.status !== 'success'}
                  type="button" onClick={() => onActivate(profile)}>
                  <Check size={15} aria-hidden="true" />
                  {isActive ? t('list.currentlyUsed') : t('list.setActive')}
                </button>
                <button className="api-profile-list__delete-button" disabled={busy}
                  type="button" onClick={() => onRequestDelete(profile.id)}>
                  <Trash2 size={15} aria-hidden="true" />
                  {t('list.delete')}
                </button>
              </div>
              {isConfirmingDelete ? (
                <div className="api-profile-list__delete-confirm" role="alert">
                  <div>
                    <strong>{t('list.confirmDeleteTitle', { name: profile.name })}</strong>
                    <p>
                      {isActive
                        ? t('list.confirmActiveDelete')
                        : t('list.confirmDeleteDescription')}
                    </p>
                  </div>
                  <div>
                    <button disabled={busy} type="button" onClick={onCancelDelete}>
                      {t('list.cancel')}
                    </button>
                    <button
                      className="api-profile-list__confirm-delete"
                      disabled={busy}
                      type="button"
                      onClick={() => onConfirmDelete(profile)}
                    >
                      {t('list.confirmDelete')}
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
  const { t } = useTranslation('settings')
  const keyRequired = requiresApiKey(draft, canKeepSavedKey)
  return (
    <div className="api-profile-form__fields">
      <label className="api-profile-form__field">
        <span>{t('form.profileName')}</span>
        <input
          data-dialog-initial-focus="true"
          disabled={disabled}
          maxLength={80}
          placeholder={draft.kind === 'tikhub' ? t('form.tikhubNamePlaceholder') : t('form.aiNamePlaceholder')}
          required
          value={draft.name}
          onChange={(event) => onChange({ ...draft, name: event.target.value })}
        />
        <small>{t('form.profileNameHint')}</small>
      </label>
      {draft.kind === 'tikhub' ? (
        <label className="api-profile-form__field">
          <span>{t('form.apiEndpoint')}</span>
          <select disabled={disabled} value={draft.baseUrl}
            onChange={(event) => onChange({ ...draft, baseUrl: event.target.value })}>
            <option value="https://api.tikhub.io">{t('form.internationalEndpoint')}</option>
            <option value="https://api.tikhub.dev">{t('form.mainlandEndpoint')}</option>
          </select>
        </label>
      ) : (
        <>
          <label className="api-profile-form__field">
            <span>{t('form.providerType')}</span>
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
                <option key={value} value={value}>{t(preset.labelKey)}</option>
              ))}
            </select>
          </label>
          <label className="api-profile-form__field">
            <span>{t('form.apiFormat')}</span>
            <select disabled value={draft.apiFormat} aria-describedby="api-format-hint">
              {Object.entries(API_FORMAT_LABEL_KEYS).map(([value, labelKey]) => (
                <option key={value} value={value}>{t(labelKey)}</option>
              ))}
            </select>
            <small id="api-format-hint">{t('form.apiFormatHint')}</small>
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
            <span>{t('form.defaultModel')}</span>
            <input
              disabled={disabled}
              maxLength={160}
              placeholder={t('form.modelPlaceholder')}
              required
              value={draft.defaultModelId}
              onChange={(event) => onChange({ ...draft, defaultModelId: event.target.value })}
            />
          </label>
        </>
      )}
      <label className="api-profile-form__field api-profile-form__field--wide">
        <span>{draft.kind === 'tikhub' ? t('form.apiToken') : t('form.apiKey')}</span>
        <input
          aria-describedby="api-key-hint"
          autoComplete="new-password"
          disabled={disabled}
          minLength={keyRequired ? 8 : undefined}
          placeholder={t(keyPlaceholder(draft, canKeepSavedKey))}
          required={keyRequired}
          type="password"
          value={draft.apiKey}
          onChange={(event) => onChange({ ...draft, apiKey: event.target.value })}
        />
        <small id="api-key-hint">
          {canKeepSavedKey
            ? t('form.keepCredentialHint')
            : draft.kind === 'ai' && draft.providerType === 'ollama'
              ? t('form.ollamaCredentialHint')
              : t('form.credentialHint')}
        </small>
      </label>
    </div>
  )
}
function SecurityNotice({ compact = false, kind }: {
  compact?: boolean; kind: ApiProfileKind
}) {
  const { t } = useTranslation('settings')
  return (
    <div className="api-profile-dialog__security" data-compact={compact}>
      <ShieldCheck size={16} aria-hidden="true" />
      <p>
        {t('security.credentials')}
        {kind === 'ai' ? ` ${t('security.aiBoundary')}` : ''}
      </p>
    </div>
  )
}
function canSaveProfile(
  draft: ApiProfileDraft,
  profile: ApiProfileView | null,
) {
  if (!draft.name.trim() || !isSupportedUrl(draft.baseUrl)) return false
  const editingHasReusableKey = canReuseSavedCredential(draft, profile)
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
function canReuseSavedCredential(
  draft: ApiProfileDraft,
  profile: ApiProfileView | null,
) {
  if (!profile?.hasCredential || profile.status === 'needs_rebind' || draft.kind !== profile.kind) {
    return false
  }
  return draft.kind === 'tikhub'
    || (profile.kind === 'ai' && draft.providerType === profile.providerType)
}
function requiresApiKey(draft: ApiProfileDraft, canKeepSavedKey: boolean) {
  return draft.kind === 'tikhub'
    ? !canKeepSavedKey
    : draft.providerType !== 'ollama' && !canKeepSavedKey
}
function keyPlaceholder(draft: ApiProfileDraft, canKeepSavedKey: boolean) {
  if (canKeepSavedKey) return 'form.keepCredentialPlaceholder'
  if (draft.kind === 'ai' && draft.providerType === 'ollama') return 'form.ollamaCredentialPlaceholder'
  return draft.kind === 'tikhub' ? 'form.tikhubCredentialPlaceholder' : 'form.aiCredentialPlaceholder'
}
function isSupportedUrl(value: string) {
  try {
    const url = new URL(value.trim())
    return url.protocol === 'https:' || url.protocol === 'http:'
  } catch {
    return false
  }
}
function profileDescriptor(profile: ApiProfileView, t: (key: string) => string) {
  if (profile.kind === 'tikhub') {
    return profile.testSummary?.maskedAccount ?? t('profileDescriptor.tikhub')
  }
  return `${t(AI_PROVIDER_PRESETS[profile.providerType].labelKey)} · ${t(API_FORMAT_LABEL_KEYS[profile.apiFormat])}`
}
function maskedKeyLabel(profile: ApiProfileView, t: (key: string) => string) {
  if (profile.maskedKey) return profile.maskedKey
  if (profile.kind === 'ai' && profile.providerType === 'ollama') return t('credentialStatus.notRequired')
  return profile.status === 'needs_rebind'
    ? t('credentialStatus.needsRebind')
    : t('credentialStatus.notBound')
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
    canReuseSavedCredential,
    canSaveProfile,
    createProfileDraft,
    initialApiProfilesDialogState,
  },
})

export default ApiProfilesDialogWithTestUtils
