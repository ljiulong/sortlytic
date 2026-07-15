import { KeyRound, MonitorCheck, ShieldCheck, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import type { TikhubConnectionTestResult, TikhubConnectorView } from './backend-api'

type TikhubSettingsPanelProps = {
  connector?: TikhubConnectorView | null
  isBusy: boolean
  result?: TikhubConnectionTestResult
  onSaveAndTest: (input: { token: string; baseUrl: string }) => Promise<unknown>
}

function TikhubSettingsPanel({ connector, isBusy, result, onSaveAndTest }: TikhubSettingsPanelProps) {
  const [isOpen, setIsOpen] = useState(false)
  const [token, setToken] = useState('')
  const [baseUrl, setBaseUrl] = useState('https://api.tikhub.io')
  const hasSavedToken = Boolean(connector?.secret_ref_id)
  const statusLabel = result?.success ? '已连通' : connector?.enabled ? '已保存' : '待配置'

  useEffect(() => {
    if (connector?.base_url) setBaseUrl(connector.base_url)
  }, [connector?.base_url])

  useEffect(() => {
    if (!isOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !isBusy) setIsOpen(false)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [isBusy, isOpen])

  const submit = async () => {
    try {
      await onSaveAndTest({ token, baseUrl })
      setToken('')
      setIsOpen(false)
    } catch {
      // 工作区顶部状态区会显示后端错误，保留输入内容以便修正后重试。
    }
  }

  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">TikHub 设置</p>
          <h2>免费额度可行性测试</h2>
        </div>
        <span className="status-pill" data-tone={result?.success ? 'success' : connector?.enabled ? 'info' : 'warning'}>
          {statusLabel}
        </span>
      </div>

      <div className="settings-summary">
        <div className="settings-summary-copy">
          <div className="connection-icon" data-tone={result?.success ? 'success' : 'info'}>
            <MonitorCheck size={17} aria-hidden="true" />
          </div>
          <div>
            <strong>{connector?.base_url ?? '尚未配置 TikHub API'}</strong>
            <span>{hasSavedToken ? 'Token 已保存到系统安全存储' : 'Token 尚未绑定'}</span>
          </div>
        </div>
        <button className="primary-button" type="button" onClick={() => setIsOpen(true)}>
          <KeyRound size={16} aria-hidden="true" />
          {hasSavedToken ? '编辑 TikHub 配置' : '配置 TikHub API'}
        </button>
      </div>

      <div className="export-grid">
        <InfoItem icon={MonitorCheck} label="账号" value={result?.masked_email ?? '等待 Token 测试'} />
        <InfoItem icon={ShieldCheck} label="免费额度" value={result?.free_credit == null ? '未知' : String(result.free_credit)} />
        <InfoItem icon={ShieldCheck} label="邮箱验证" value={result?.email_verified == null ? '未知' : result.email_verified ? '已验证' : '未验证'} />
      </div>

      {isOpen ? (
        <div
          className="settings-modal-backdrop"
          role="presentation"
          onMouseDown={() => {
            if (!isBusy) setIsOpen(false)
          }}
        >
          <div
            aria-labelledby="tikhub-settings-dialog-title"
            aria-modal="true"
            className="settings-modal"
            role="dialog"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="settings-modal-header">
              <div>
                <p className="eyebrow">TikHub API</p>
                <h2 id="tikhub-settings-dialog-title">配置并测试连接</h2>
              </div>
              <button
                aria-label="关闭 TikHub 配置弹窗"
                className="toolbar-icon-button"
                disabled={isBusy}
                type="button"
                onClick={() => setIsOpen(false)}
              >
                <X size={17} aria-hidden="true" />
              </button>
            </div>
            <div className="settings-modal-body">
              <label className="field">
                <span>API 域名</span>
                <select value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)}>
                  <option value="https://api.tikhub.io">国际域名 api.tikhub.io</option>
                  <option value="https://api.tikhub.dev">中国大陆域名 api.tikhub.dev</option>
                </select>
              </label>
              <label className="field">
                <span>API Token</span>
                <input
                  autoComplete="off"
                  autoFocus={!hasSavedToken}
                  placeholder={hasSavedToken ? '留空以复用已保存 Token' : '只保存到系统安全存储'}
                  type="password"
                  value={token}
                  onChange={(event) => setToken(event.target.value)}
                />
              </label>
              <div className="model-security-note">
                <ShieldCheck size={17} aria-hidden="true" />
                <p>Token 只写入 macOS 系统安全存储，应用数据库仅保存密钥引用。</p>
              </div>
            </div>
            <div className="settings-modal-footer">
              <button className="ghost-button" disabled={isBusy} type="button" onClick={() => setIsOpen(false)}>
                取消
              </button>
              <button
                className="primary-button"
                disabled={isBusy || (!hasSavedToken && token.trim().length < 8)}
                type="button"
                onClick={() => void submit()}
              >
                <KeyRound size={16} aria-hidden="true" />
                {isBusy ? '正在保存并测试' : '保存并测试'}
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  )
}

function InfoItem({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof MonitorCheck
  label: string
  value: string
}) {
  return (
    <div className="export-item">
      <div className="connection-icon" data-tone="info">
        <Icon size={15} aria-hidden="true" />
      </div>
      <div>
        <span>{label}</span>
        <strong>{value}</strong>
      </div>
    </div>
  )
}

export default TikhubSettingsPanel
