import {
  ArrowUpRight,
  Download,
  RefreshCcw,
  RotateCcw,
  X,
} from 'lucide-react'
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
} from 'react'
import { useTranslation } from 'react-i18next'
import type { AppUpdateInfo } from './backend-api'
import ExternalLink from './ExternalLink'
import './i18n'
import type { AppUpdatePhase } from './use-app-updater'
import type { UpdatePreferences } from './update-preferences'
import './UpdateSettingsPanel.css'

const logoUrl = new URL('../src-tauri/icons/icon.png', import.meta.url).href
const githubUrl = 'https://github.com/ljiulong/sortlytic' as const
const automaticallyOpenedVersions = new Set<string>()

type UpdateSettingsPanelProps = {
  isTauriApp: boolean
  currentVersion: string | null
  update?: AppUpdateInfo | null
  phase: AppUpdatePhase
  error?: string
  preferences: UpdatePreferences
  setAutoCheck: (enabled: boolean) => void
  setAutoDownload: (enabled: boolean) => void
  checkForUpdate: () => Promise<AppUpdateInfo | null>
  prepareUpdate: () => Promise<void>
  relaunchToUpdate: () => Promise<void>
}

function UpdateSettingsPanel({
  isTauriApp,
  currentVersion,
  update,
  phase,
  error,
  preferences,
  setAutoCheck,
  setAutoDownload,
  checkForUpdate,
  prepareUpdate,
  relaunchToUpdate,
}: UpdateSettingsPanelProps) {
  const { t } = useTranslation('updates')
  const { t: tCommon } = useTranslation('common')
  const [dialogOpen, setDialogOpen] = useState(false)
  const [externalLinkError, setExternalLinkError] = useState<string | null>(null)
  const dialogRef = useRef<HTMLDivElement>(null)
  const returnFocusRef = useRef<HTMLElement | null>(null)
  const preparedRef = useRef(phase === 'ready')
  const isPreparing = phase === 'preparing'
  const isRelaunching = phase === 'relaunching'
  const isDialogBusy = isPreparing || isRelaunching
  const canCloseDialog = !isDialogBusy
  const statusText = getStatusText({ error, isTauriApp, phase, t, update })
  const statusTone = getStatusTone({ isTauriApp, phase })
  const checkDisabled = !isTauriApp
    || ['checking', 'preparing', 'ready', 'relaunching'].includes(phase)
  const checkLabel = phase === 'checking'
    ? t('actions.checking')
    : phase === 'idle'
      ? t('actions.check')
      : t('actions.checkAgain')
  const updateAction = getUpdateAction({
    hasUpdate: Boolean(update),
    phase,
    prepared: preparedRef.current,
    t,
  })
  const dialogUpdateAction = getDialogUpdateAction({
    hasUpdate: Boolean(update),
    phase,
    prepared: preparedRef.current,
    t,
  })

  const openDialog = useCallback((trigger?: HTMLElement | null) => {
    returnFocusRef.current = trigger
      ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null)
    setDialogOpen(true)
  }, [])

  const closeDialog = useCallback(() => {
    if (canCloseDialog) setDialogOpen(false)
  }, [canCloseDialog])

  useEffect(() => {
    if (phase === 'ready') preparedRef.current = true
    if (!update || phase === 'available') preparedRef.current = false
  }, [phase, update])

  useEffect(() => {
    if (
      !update
      || !['available', 'preparing', 'ready', 'error'].includes(phase)
      || automaticallyOpenedVersions.has(update.version)
    ) {
      return
    }
    automaticallyOpenedVersions.add(update.version)
    openDialog()
  }, [openDialog, phase, update])

  useEffect(() => {
    if (!dialogOpen) return
    const frame = window.requestAnimationFrame(() => dialogRef.current?.focus())
    const returnFocus = returnFocusRef.current
    return () => {
      window.cancelAnimationFrame(frame)
      returnFocus?.focus()
    }
  }, [dialogOpen])

  useEffect(() => {
    if (!dialogOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        if (canCloseDialog) setDialogOpen(false)
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
  }, [canCloseDialog, dialogOpen])

  const handleCheck = async (trigger: HTMLButtonElement) => {
    try {
      const nextUpdate = await checkForUpdate()
      if (!nextUpdate) return
      automaticallyOpenedVersions.add(nextUpdate.version)
      openDialog(trigger)
    } catch {
      // The updater hook exposes the safe error string and error phase.
    }
  }

  const runUpdateAction = async (action: UpdateAction) => {
    try {
      if (action.kind === 'prepare') {
        await prepareUpdate()
      } else if (action.kind === 'relaunch') {
        await relaunchToUpdate()
      }
    } catch {
      // The updater hook owns the visible error state and retry transition.
    }
  }

  const handleUpdateAction = (trigger: HTMLButtonElement) => {
    if (updateAction.kind === 'view') {
      openDialog(trigger)
      return
    }
    void runUpdateAction(updateAction)
  }

  const handleBackdropMouseDown = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) closeDialog()
  }

  return (
    <>
      <section className="update-settings" aria-labelledby="update-settings-heading">
        <header className="update-settings__header">
          <div className="update-settings__identity">
            <img alt="" aria-hidden="true" height="40" src={logoUrl} width="40" />
            <div>
              <p className="eyebrow">{t('about.eyebrow')}</p>
              <h3 id="update-settings-heading">{t('about.title')}</h3>
              <p className="update-settings__version">
                <span>{t('about.currentVersion')}</span>
                <strong>{currentVersion ? `v${currentVersion}` : t('about.developmentPreview')}</strong>
              </p>
            </div>
          </div>
          <ExternalLink
            ariaLabel={t('about.github')}
            className="update-settings__github"
            href={githubUrl}
            onClick={() => setExternalLinkError(null)}
            onOpenError={() => setExternalLinkError(tCommon('unknownError'))}
          >
            <GithubMark />
            <span>{t('about.github')}</span>
            <ArrowUpRight size={14} aria-hidden="true" />
          </ExternalLink>
        </header>

        {externalLinkError ? (
          <p className="update-settings__link-error" role="status">{externalLinkError}</p>
        ) : null}

        <div className="update-settings__preferences">
          <UpdatePreference
            checked={preferences.autoCheck}
            description={t('preferences.autoCheck.description')}
            id="auto-check"
            label={t('preferences.autoCheck.title')}
            onChange={setAutoCheck}
          />
          <UpdatePreference
            checked={preferences.autoDownload}
            description={preferences.autoCheck
              ? t('preferences.autoDownload.description')
              : t('preferences.autoDownload.unavailable')}
            disabled={!preferences.autoCheck}
            id="auto-download"
            label={t('preferences.autoDownload.title')}
            onChange={setAutoDownload}
          />
        </div>

        <footer className="update-settings__footer">
          <p className="update-settings__status" data-tone={statusTone} aria-live="polite">
            {statusText}
          </p>
          <div className="update-settings__actions">
            <button
              className="ghost-button"
              data-update-action="check"
              disabled={checkDisabled}
              type="button"
              onClick={(event) => void handleCheck(event.currentTarget)}
            >
              <RefreshCcw size={16} aria-hidden="true" />
              {checkLabel}
            </button>
            <button
              className="primary-button"
              data-update-action="update"
              disabled={updateAction.disabled}
              type="button"
              onClick={(event) => handleUpdateAction(event.currentTarget)}
            >
              {updateAction.kind === 'relaunch' ? (
                <RotateCcw size={16} aria-hidden="true" />
              ) : (
                <Download size={16} aria-hidden="true" />
              )}
              {updateAction.label}
            </button>
          </div>
        </footer>
      </section>

      {dialogOpen && update ? (
        <div className="update-dialog-backdrop" onMouseDown={handleBackdropMouseDown}>
          <div
            ref={dialogRef}
            aria-describedby="update-dialog-status"
            aria-labelledby="update-dialog-title"
            aria-modal="true"
            className="update-dialog"
            role="dialog"
            tabIndex={-1}
          >
            <header className="update-dialog__header">
              <div>
                <p className="eyebrow">{t('dialog.notesTitle')}</p>
                <h2 id="update-dialog-title">
                  {t('dialog.title', { version: update.version })}
                </h2>
              </div>
              <button
                aria-label={t('actions.close')}
                className="update-dialog__close"
                data-update-dialog-action="close"
                disabled={!canCloseDialog}
                type="button"
                onClick={closeDialog}
              >
                <X size={17} aria-hidden="true" />
              </button>
            </header>
            <div className="update-dialog__body">
              {update.date ? (
                <p className="update-dialog__date">
                  {t('dialog.releaseDate', { date: update.date })}
                </p>
              ) : null}
              <div className="update-dialog__notes" tabIndex={0}>
                {update.body?.trim() ? update.body : t('dialog.notesEmpty')}
              </div>
              <p
                className="update-dialog__status"
                id="update-dialog-status"
                role="status"
                aria-live="polite"
              >
                <strong>{t('dialog.statusTitle')}</strong>
                <span>{statusText}</span>
              </p>
            </div>
            <footer className="update-dialog__footer">
              <button
                className="ghost-button"
                disabled={!canCloseDialog}
                type="button"
                onClick={closeDialog}
              >
                {t('actions.close')}
              </button>
              <button
                className="primary-button"
                data-update-dialog-action="primary"
                disabled={dialogUpdateAction.disabled}
                type="button"
                onClick={() => void runUpdateAction(dialogUpdateAction)}
              >
                {dialogUpdateAction.kind === 'relaunch' ? (
                  <RotateCcw size={16} aria-hidden="true" />
                ) : (
                  <Download size={16} aria-hidden="true" />
                )}
                {dialogUpdateAction.label}
              </button>
            </footer>
          </div>
        </div>
      ) : null}
    </>
  )
}

type UpdatePreferenceProps = {
  checked: boolean
  description: string
  disabled?: boolean
  id: 'auto-check' | 'auto-download'
  label: string
  onChange: (enabled: boolean) => void
}

function UpdatePreference({
  checked,
  description,
  disabled = false,
  id,
  label,
  onChange,
}: UpdatePreferenceProps) {
  const descriptionId = `update-preference-${id}-description`
  return (
    <label className="update-settings__preference" data-disabled={disabled}>
      <input
        aria-describedby={descriptionId}
        checked={checked}
        data-update-preference={id}
        disabled={disabled}
        type="checkbox"
        onChange={(event) => onChange(event.currentTarget.checked)}
      />
      <span className="update-settings__switch" aria-hidden="true"><span /></span>
      <span className="update-settings__preference-copy">
        <strong>{label}</strong>
        <small id={descriptionId}>{description}</small>
      </span>
    </label>
  )
}

function GithubMark() {
  return (
    <svg aria-hidden="true" fill="currentColor" height="16" viewBox="0 0 24 24" width="16">
      <path d="M12 .7a11.3 11.3 0 0 0-3.57 22c.57.1.78-.25.78-.55v-2.17c-3.18.69-3.85-1.35-3.85-1.35-.52-1.32-1.27-1.67-1.27-1.67-1.04-.71.08-.7.08-.7 1.15.08 1.76 1.18 1.76 1.18 1.02 1.75 2.68 1.24 3.33.95.1-.74.4-1.24.73-1.53-2.54-.29-5.21-1.27-5.21-5.66 0-1.25.45-2.27 1.18-3.07-.12-.29-.51-1.45.11-3.02 0 0 .96-.31 3.11 1.17a10.76 10.76 0 0 1 5.67 0c2.15-1.48 3.11-1.17 3.11-1.17.62 1.57.23 2.73.11 3.02.73.8 1.18 1.82 1.18 3.07 0 4.4-2.68 5.37-5.23 5.65.41.35.78 1.05.78 2.12v3.15c0 .3.2.66.79.55A11.3 11.3 0 0 0 12 .7Z" />
    </svg>
  )
}

type Translation = ReturnType<typeof useTranslation<'updates'>>['t']

function getStatusText({
  error,
  isTauriApp,
  phase,
  t,
  update,
}: {
  error?: string
  isTauriApp: boolean
  phase: AppUpdatePhase
  t: Translation
  update?: AppUpdateInfo | null
}) {
  if (!isTauriApp) return t('phase.packagedOnly')
  if (phase === 'error') return error || t('phase.error')
  if (phase === 'available') return t('phase.available', { version: update?.version ?? '' })
  if (phase === 'preparing') return t('phase.preparing', { version: update?.version ?? '' })
  return t(`phase.${phase}`)
}

function getStatusTone({
  isTauriApp,
  phase,
}: {
  isTauriApp: boolean
  phase: AppUpdatePhase
}) {
  if (!isTauriApp || phase === 'idle') return 'warning'
  if (phase === 'error') return 'danger'
  if (phase === 'latest' || phase === 'ready') return 'success'
  return 'info'
}

type UpdateAction = {
  disabled: boolean
  kind: 'disabled' | 'view' | 'prepare' | 'relaunch'
  label: string
}

function getUpdateAction({
  hasUpdate,
  phase,
  prepared,
  t,
}: {
  hasUpdate: boolean
  phase: AppUpdatePhase
  prepared: boolean
  t: Translation
}): UpdateAction {
  if (phase === 'preparing') {
    return { disabled: true, kind: 'disabled', label: t('actions.preparing') }
  }
  if (phase === 'relaunching') {
    return { disabled: true, kind: 'disabled', label: t('actions.relaunching') }
  }
  if (phase === 'ready') {
    return { disabled: false, kind: 'relaunch', label: t('actions.relaunch') }
  }
  if (phase === 'error' && hasUpdate) {
    return {
      disabled: false,
      kind: prepared ? 'relaunch' : 'prepare',
      label: t('actions.retryUpdate'),
    }
  }
  if (phase === 'available' && hasUpdate) {
    return { disabled: false, kind: 'view', label: t('actions.viewUpdate') }
  }
  return { disabled: true, kind: 'disabled', label: t('actions.update') }
}

function getDialogUpdateAction({
  hasUpdate,
  phase,
  prepared,
  t,
}: {
  hasUpdate: boolean
  phase: AppUpdatePhase
  prepared: boolean
  t: Translation
}): UpdateAction {
  if (!hasUpdate) {
    return { disabled: true, kind: 'disabled', label: t('actions.update') }
  }
  if (phase === 'preparing') {
    return { disabled: true, kind: 'disabled', label: t('actions.preparing') }
  }
  if (phase === 'relaunching') {
    return { disabled: true, kind: 'disabled', label: t('actions.relaunching') }
  }
  if (phase === 'ready' || (phase === 'error' && prepared)) {
    return { disabled: false, kind: 'relaunch', label: t('actions.relaunch') }
  }
  if (phase === 'error') {
    return { disabled: false, kind: 'prepare', label: t('actions.retryUpdate') }
  }
  if (phase === 'available') {
    return { disabled: false, kind: 'prepare', label: t('actions.downloadAndInstall') }
  }
  return { disabled: true, kind: 'disabled', label: t('actions.update') }
}

function getFocusableElements(container: HTMLElement | null) {
  if (!container) return []
  return Array.from(container.querySelectorAll<HTMLElement>([
    'button:not([disabled])',
    'input:not([disabled])',
    '[href]',
    '[tabindex]:not([tabindex="-1"])',
  ].join(','))).filter((element) => !element.hasAttribute('hidden'))
}

export default UpdateSettingsPanel
