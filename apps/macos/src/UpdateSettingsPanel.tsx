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
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">自动更新</p>
          <h2>保持客户端为最新版本</h2>
        </div>
        <span className="status-pill" data-tone={statusTone}>{statusLabel}</span>
      </div>

      <div className="update-settings-body">
        <div className="update-summary">
          <div className="connection-icon" data-tone={update ? 'success' : updateError ? 'danger' : 'info'}>
            {update ? <CheckCircle2 size={17} aria-hidden="true" /> : <ShieldCheck size={17} aria-hidden="true" />}
          </div>
          <div>
            <strong>{summary}</strong>
            <p>{isTauriApp ? '更新包经过签名校验，安装完成后应用会自动重启。' : '浏览器预览不具备更新权限，请打开打包后的 macOS 应用。'}</p>
          </div>
        </div>

        {update?.body ? <p className="update-notes">版本说明：{update.body}</p> : null}

        <div className="update-settings-footer">
          <span className="muted-text">仅下载官方发布的 macOS 更新包</span>
          <div className="action-row">
            <button
              className="ghost-button"
              disabled={!isTauriApp || isCheckingForUpdate || isInstallingUpdate}
              type="button"
              onClick={() => {
                void checkForUpdate().catch(() => undefined)
              }}
            >
              <RefreshCcw size={16} aria-hidden="true" />
              {isCheckingForUpdate ? '正在检查' : '检查更新'}
            </button>
            {update ? (
              <button
                className="primary-button"
                disabled={isCheckingForUpdate || isInstallingUpdate}
                type="button"
                onClick={() => {
                  void installUpdate().catch(() => undefined)
                }}
              >
                <Download size={16} aria-hidden="true" />
                {isInstallingUpdate ? '正在安装' : '下载并重启'}
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </section>
  )
}

export default UpdateSettingsPanel
