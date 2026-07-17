import { KeyRound, MonitorCheck, ShieldCheck, X } from 'lucide-react'
import { useEffect, useState } from 'react'
import type { TikhubConnectionTestResult, TikhubConnectorView } from './backend-api'
import './SettingsPanels.css'

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
    <section className="settings-provider" aria-labelledby="tikhub-provider-heading">
      <header className="settings-provider__header">
        <div className="settings-provider__identity">
          <span className="settings-provider__icon" data-tone={result?.success ? 'success' : 'info'}>
            <MonitorCheck size={17} aria-hidden="true" />
          </span>
          <div>
            <p className="eyebrow">TikHub API</p>
            <h3 id="tikhub-provider-heading">账户与可用额度</h3>
            <p>连接 TikTok、抖音和小红书的公开数据接口。</p>
          </div>
        </div>
        <span className="status-pill" data-tone={result?.success ? 'success' : connector?.enabled ? 'info' : 'warning'}>
          {statusLabel}
        </span>
      </header>

      <div className="settings-provider__current">
        <div>
          <span>当前端点</span>
          <strong>{connector?.base_url ?? '尚未配置 TikHub API'}</strong>
          <p>{hasSavedToken ? 'Token 已保存到系统安全存储' : 'Token 尚未绑定'}</p>
        </div>
        <button className="primary-button" type="button" onClick={() => setIsOpen(true)}>
          <KeyRound size={16} aria-hidden="true" />
          {hasSavedToken ? '管理配置' : '配置 API'}
        </button>
      </div>

      {result ? (
        <dl className="settings-provider__facts">
          <ProviderFact label="账号" value={result.masked_email ?? '未返回账号'} />
          <ProviderFact label="邮箱验证" value={result.email_verified == null ? '未知' : result.email_verified ? '已验证' : '未验证'} />
          <ProviderFact label="充值余额" value={formatCredit(result.balance)} numeric />
          <ProviderFact label="免费额度" value={formatCredit(result.free_credit)} numeric />
          <ProviderFact label="可用额度合计" value={formatCredit(result.available_credit)} numeric />
          <ProviderFact label="今日用量" value={formatDailyUsage(result.daily_usage_json)} numeric />
        </dl>
      ) : (
        <div className="settings-provider__empty">
          <strong>尚无账户与额度结果</strong>
          <p>保存并测试 Token 后，这里会显示真实账号、双额度和今日请求数。</p>
        </div>
      )}

      <div className="settings-provider__security">
        <ShieldCheck size={16} aria-hidden="true" />
        <p>Token 只写入 macOS 系统安全存储，应用数据库仅保存密钥引用。</p>
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
              <div className="settings-provider__dialog-security">
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

function formatCredit(value?: number | null) {
  return value == null || !Number.isFinite(value) ? '未知' : `$${value.toFixed(2)}`
}

function formatDailyUsage(value?: Record<string, unknown>) {
  if (!value || 'warning' in value) return '未知'
  const requestCount = ['total_requests', 'request_count', 'requests', 'used']
    .map((key) => value[key])
    .find((candidate): candidate is number => typeof candidate === 'number' && Number.isFinite(candidate))
  return requestCount == null ? '已获取明细' : `${requestCount.toLocaleString()} 次请求`
}

function ProviderFact({
  label,
  value,
  numeric = false,
}: {
  label: string
  value: string
  numeric?: boolean
}) {
  return (
    <div>
      <dt>{label}</dt>
      <dd data-numeric={numeric}>{value}</dd>
    </div>
  )
}

export default TikhubSettingsPanel
