import { KeyRound, MonitorCheck, ShieldCheck } from 'lucide-react'
import { useState } from 'react'
import type { TikhubConnectionTestResult } from './backend-api'

type TikhubSettingsPanelProps = {
  isBusy: boolean
  result?: TikhubConnectionTestResult
  onSaveAndTest: (input: { token: string; baseUrl: string }) => Promise<unknown>
}

function TikhubSettingsPanel({ isBusy, result, onSaveAndTest }: TikhubSettingsPanelProps) {
  const [token, setToken] = useState('')
  const [baseUrl, setBaseUrl] = useState('https://api.tikhub.io')

  const submit = async () => {
    await onSaveAndTest({ token, baseUrl })
    setToken('')
  }

  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">TikHub 设置</p>
          <h2>免费额度可行性测试</h2>
        </div>
        <span className="status-pill" data-tone={result?.success ? 'success' : 'warning'}>
          {result?.success ? '已连通' : '待配置'}
        </span>
      </div>
      <div className="export-grid">
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
            placeholder="只保存到系统安全存储"
            type="password"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </label>
        <button
          className="primary-button wide-button"
          disabled={isBusy || token.trim().length < 8}
          type="button"
          onClick={() => {
            void submit()
          }}
        >
          <KeyRound size={16} aria-hidden="true" />
          保存并测试
        </button>
        <div className="export-grid">
          <InfoItem
            icon={MonitorCheck}
            label="账号"
            value={result?.masked_email ?? '等待 Token 测试'}
          />
          <InfoItem
            icon={ShieldCheck}
            label="免费额度"
            value={result?.free_credit == null ? '未知' : String(result.free_credit)}
          />
          <InfoItem
            icon={ShieldCheck}
            label="邮箱验证"
            value={result?.email_verified == null ? '未知' : result.email_verified ? '已验证' : '未验证'}
          />
        </div>
      </div>
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
