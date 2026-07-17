import { CheckCircle2, Download, RefreshCcw, ShieldCheck } from 'lucide-react'
import type { AppUpdateInfo } from './backend-api'
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
  const statusTone = updateError ? 'danger' : update ? 'success' : hasCheckedForUpdate ? 'info' : 'warning'
  const statusLabel = updateError
    ? '检查失败'
    : update
      ? '有新版本'
      : hasCheckedForUpdate
        ? '已是最新'
        : isTauriApp
          ? '尚未检查'
          : '打包应用可用'
  const summary = update
    ? `发现版本 ${update.version}，可以立即下载并重启应用`
    : updateError ?? (hasCheckedForUpdate ? '当前版本已经是最新版本' : '发布后可从 GitHub 安全获取签名更新')

  return (
    <section className="update-settings" aria-labelledby="update-settings-heading">
      <header className="update-settings__header">
        <div className="update-settings__identity">
          <span className="update-settings__icon" data-tone={update ? 'success' : updateError ? 'danger' : 'info'}>
            {update ? <CheckCircle2 size={17} aria-hidden="true" /> : <ShieldCheck size={17} aria-hidden="true" />}
          </span>
          <div>
            <p className="eyebrow">自动更新</p>
            <h3 id="update-settings-heading">客户端版本</h3>
            <p>只检查和下载官方发布的 macOS 更新包。</p>
          </div>
        </div>
        <span className="status-pill" data-tone={statusTone}>{statusLabel}</span>
      </header>

      <div className="update-settings__body">
        <strong>{summary}</strong>
        <p>
          {isTauriApp
            ? '更新包经过签名校验，安装完成后应用会自动重启。'
            : '浏览器预览不具备更新权限，请打开打包后的 macOS 应用。'}
        </p>
        {update?.body ? (
          <section className="update-settings__notes" aria-labelledby="update-notes-heading">
            <h4 id="update-notes-heading">版本说明</h4>
            <p>{update.body}</p>
          </section>
        ) : null}
      </div>

      <footer className="update-settings__footer">
        <span>官方发布源 · 签名校验 · 本地安装</span>
        <div className="update-settings__actions">
          <button
            className="ghost-button"
            disabled={!isTauriApp || isCheckingForUpdate || isInstallingUpdate}
            type="button"
            onClick={() => void checkForUpdate().catch(() => undefined)}
          >
            <RefreshCcw size={16} aria-hidden="true" />
            {isCheckingForUpdate ? '正在检查' : '检查更新'}
          </button>
          {update ? (
            <button
              className="primary-button"
              disabled={isCheckingForUpdate || isInstallingUpdate}
              type="button"
              onClick={() => void installUpdate().catch(() => undefined)}
            >
              <Download size={16} aria-hidden="true" />
              {isInstallingUpdate ? '正在安装' : '下载并重启'}
            </button>
          ) : null}
        </div>
      </footer>
    </section>
  )
}

export default UpdateSettingsPanel
