import { CheckCircle2, Download, RefreshCcw, ShieldCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import type { AppUpdateInfo } from './backend-api'
import './i18n'
import './UpdateSettingsPanel.css'

type UpdateSettingsPanelProps = {
  isTauriApp: boolean
  update?: AppUpdateInfo | null
  hasCheckedForUpdate: boolean
  updateError?: string
  isCheckingForUpdate: boolean
  isInstallingUpdate: boolean
  checkForUpdate: () => Promise<AppUpdateInfo | null>
  installUpdate: () => Promise<void>
}

function UpdateSettingsPanel({
  isTauriApp,
  update,
  hasCheckedForUpdate,
  updateError,
  isCheckingForUpdate,
  isInstallingUpdate,
  checkForUpdate,
  installUpdate,
}: UpdateSettingsPanelProps) {
  const { t } = useTranslation('updates')
  const statusTone = updateError ? 'danger' : update ? 'success' : hasCheckedForUpdate ? 'info' : 'warning'
  const statusLabel = updateError
    ? t('status.checkFailed')
    : update
      ? t('status.available')
      : hasCheckedForUpdate
        ? t('status.latest')
        : isTauriApp
          ? t('status.notChecked')
          : t('status.packagedOnly')
  const summary = update
    ? t('summary.available', { version: update.version })
    : updateError
      ? t('summary.checkFailed')
      : hasCheckedForUpdate
        ? t('summary.latest')
        : t('summary.notChecked')

  return (
    <section className="update-settings" aria-labelledby="update-settings-heading">
      <header className="update-settings__header">
        <div className="update-settings__identity">
          <span className="update-settings__icon" data-tone={update ? 'success' : updateError ? 'danger' : 'info'}>
            {update ? <CheckCircle2 size={17} aria-hidden="true" /> : <ShieldCheck size={17} aria-hidden="true" />}
          </span>
          <div>
            <p className="eyebrow">{t('eyebrow')}</p>
            <h3 id="update-settings-heading">{t('title')}</h3>
            <p>{t('description')}</p>
          </div>
        </div>
        <span className="status-pill" data-tone={statusTone}>{statusLabel}</span>
      </header>

      <div className="update-settings__body">
        <strong>{summary}</strong>
        <p>
          {isTauriApp
            ? t('body.signedInstall')
            : t('body.browserPreview')}
        </p>
        {update?.body?.trim() ? (
          <section className="update-settings__notes" aria-labelledby="update-notes-heading">
            <h4 id="update-notes-heading">{t('notes.title')}</h4>
            <p>{update.body}</p>
          </section>
        ) : (
          <p className="update-settings__notes-empty">{t('notes.empty')}</p>
        )}
      </div>

      <footer className="update-settings__footer">
        <span>{t('footer')}</span>
        <div className="update-settings__actions">
          <button
            className="ghost-button"
            disabled={!isTauriApp || isCheckingForUpdate || isInstallingUpdate}
            type="button"
            onClick={() => void checkForUpdate().catch(() => undefined)}
          >
            <RefreshCcw size={16} aria-hidden="true" />
            {isCheckingForUpdate ? t('actions.checking') : t('actions.check')}
          </button>
          {update ? (
            <button
              className="primary-button"
              disabled={isCheckingForUpdate || isInstallingUpdate}
              type="button"
              onClick={() => void installUpdate().catch(() => undefined)}
            >
              <Download size={16} aria-hidden="true" />
              {isInstallingUpdate ? t('actions.installing') : t('actions.install')}
            </button>
          ) : null}
        </div>
      </footer>
    </section>
  )
}

export default UpdateSettingsPanel
