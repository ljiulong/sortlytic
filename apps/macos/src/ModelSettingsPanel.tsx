import { Bot, KeyRound, ShieldCheck, X } from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import type { ModelProviderView, ProviderTestResult } from './backend-api'
import type { ModelSettingsInput } from './use-workbench-backend'
import './SettingsPanels.css'

type ApiFormat = ModelSettingsInput['apiFormat']

type ModelSettingsPanelProps = {
  providers: ModelProviderView[]
  isPending: boolean
  result?: ProviderTestResult
  activateModelProvider: (providerId: string) => Promise<unknown>
  saveAndValidateModelProvider: (input: ModelSettingsInput) => Promise<unknown>
}

const providerPresets: Array<{
  providerId: string
  displayName: string
  apiFormat: ApiFormat
  baseUrl: string
}> = [
  { providerId: 'openai', displayName: 'OpenAI', apiFormat: 'openai_compatible', baseUrl: 'https://api.openai.com/v1' },
  { providerId: 'anthropic', displayName: 'Anthropic', apiFormat: 'anthropic_messages', baseUrl: 'https://api.anthropic.com' },
  { providerId: 'gemini', displayName: 'Google Gemini', apiFormat: 'gemini', baseUrl: 'https://generativelanguage.googleapis.com' },
  { providerId: 'custom-openai', displayName: '自定义 OpenAI 兼容服务', apiFormat: 'openai_compatible', baseUrl: '' },
]

const apiFormatLabels: Record<ApiFormat, string> = {
  openai_compatible: 'OpenAI 兼容格式',
  anthropic_messages: 'Anthropic Messages',
  gemini: 'Gemini 原生格式',
  ollama: 'Ollama 本地格式',
}

function ModelSettingsPanel({
  providers,
  isPending,
  result,
  activateModelProvider,
  saveAndValidateModelProvider,
}: ModelSettingsPanelProps) {
  const initialProvider = providers[0]
  const initialPreset = providerPresets.find((preset) => preset.providerId === initialProvider?.provider_id) ?? providerPresets[0]
  const [isOpen, setIsOpen] = useState(false)
  const [form, setForm] = useState<Omit<ModelSettingsInput, 'apiKey'>>({
    providerId: initialProvider?.provider_id ?? initialPreset.providerId,
    displayName: initialProvider?.display_name ?? initialPreset.displayName,
    apiFormat: toApiFormat(initialProvider?.api_format) ?? initialPreset.apiFormat,
    baseUrl: initialProvider?.base_url ?? initialPreset.baseUrl,
    defaultModelId: initialProvider?.default_model_id ?? '',
  })
  const [apiKey, setApiKey] = useState('')
  const didSyncProviders = useRef(false)
  const configuredProvider = providers.find((provider) => provider.provider_id === form.providerId)
  const activeProvider = providers.find((provider) => provider.enabled)
  const isActiveProvider = activeProvider?.provider_id === form.providerId
  const isValidated = result?.success && result.provider_id === form.providerId
  const providerOptions = Array.from(
    new Map([
      ...providerPresets.map((preset) => [preset.providerId, preset.displayName] as const),
      ...providers.map((provider) => [provider.provider_id, provider.display_name] as const),
    ]).entries(),
  )
  const needsBaseUrl = form.apiFormat === 'openai_compatible'
  const canSubmit =
    (Boolean(configuredProvider) || apiKey.trim().length >= 8) &&
    form.displayName.trim().length > 0 &&
    form.defaultModelId.trim().length > 0 &&
    (!needsBaseUrl || form.baseUrl.trim().length > 0)

  useEffect(() => {
    if (!isOpen) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !isPending) setIsOpen(false)
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [isOpen, isPending])

  const selectProvider = useCallback((providerId: string) => {
    const existing = providers.find((provider) => provider.provider_id === providerId)
    const preset = providerPresets.find((item) => item.providerId === providerId)
    setForm({
      providerId,
      displayName: existing?.display_name ?? preset?.displayName ?? providerId,
      apiFormat: toApiFormat(existing?.api_format) ?? preset?.apiFormat ?? 'openai_compatible',
      baseUrl: existing?.base_url ?? preset?.baseUrl ?? '',
      defaultModelId: existing?.default_model_id ?? '',
    })
    setApiKey('')
  }, [providers])

  useEffect(() => {
    if (didSyncProviders.current || providers.length === 0) return
    didSyncProviders.current = true
    const savedProvider = providers.find((provider) => provider.enabled) ?? providers[0]
    if (savedProvider) selectProvider(savedProvider.provider_id)
  }, [providers, selectProvider])

  const openModal = () => {
    const providerToEdit = activeProvider ?? providers[0]
    if (providerToEdit) selectProvider(providerToEdit.provider_id)
    setIsOpen(true)
  }

  const submit = async () => {
    try {
      await saveAndValidateModelProvider({
        ...form,
        displayName: form.displayName.trim(),
        baseUrl: form.baseUrl.trim(),
        defaultModelId: form.defaultModelId.trim(),
        apiKey,
      })
      setApiKey('')
      setIsOpen(false)
    } catch {
      // 工作区顶部状态区会显示后端错误，密钥仍保留以便修正后重试。
    }
  }

  const activate = async () => {
    try {
      await activateModelProvider(form.providerId)
      setIsOpen(false)
    } catch {
      // 工作区顶部状态区会显示后端错误。
    }
  }

  return (
    <section className="settings-provider" aria-labelledby="model-provider-heading">
      <header className="settings-provider__header">
        <div className="settings-provider__identity">
          <span className="settings-provider__icon" data-tone={isActiveProvider ? 'success' : 'info'}>
            <Bot size={17} aria-hidden="true" />
          </span>
          <div>
            <p className="eyebrow">模型 API</p>
            <h3 id="model-provider-heading">供应商与结构化输出</h3>
            <p>管理模型地址、默认模型和安全密钥。</p>
          </div>
        </div>
        <span className="status-pill" data-tone={isActiveProvider || isValidated ? 'success' : configuredProvider ? 'info' : 'warning'}>
          {isActiveProvider ? '当前使用' : isValidated ? '配置已校验' : configuredProvider ? '已保存' : '待配置'}
        </span>
      </header>

      <div className="settings-provider__current">
        <div>
          <span>当前供应商</span>
          <strong>{activeProvider?.display_name ?? '尚未配置模型 API'}</strong>
          <p>{activeProvider?.default_model_id ?? '保存后可在弹窗中切换供应商'}</p>
        </div>
        <button className="primary-button" type="button" onClick={openModal}>
          <KeyRound size={16} aria-hidden="true" />
          {providers.length > 0 ? '管理配置' : '配置 API'}
        </button>
      </div>

      {activeProvider ? (
        <dl className="settings-provider__facts settings-provider__facts--model">
          <div>
            <dt>供应商</dt>
            <dd>{activeProvider.display_name}</dd>
          </div>
          <div>
            <dt>默认模型</dt>
            <dd>{activeProvider.default_model_id || '尚未设置'}</dd>
          </div>
          <div>
            <dt>API 格式</dt>
            <dd>{apiFormatLabels[toApiFormat(activeProvider.api_format) ?? 'openai_compatible']}</dd>
          </div>
        </dl>
      ) : (
        <div className="settings-provider__empty">
          <strong>尚无可用模型配置</strong>
          <p>保存并校验供应商、模型 ID 和 API Key 后，可以将它设为当前模型。</p>
        </div>
      )}

      <div className="settings-provider__security">
        <ShieldCheck size={17} aria-hidden="true" />
        <p>API Key 只写入 macOS 系统安全存储，应用数据库仅保存密钥引用。</p>
      </div>

      {isOpen ? (
        <div
          className="settings-modal-backdrop"
          role="presentation"
          onMouseDown={() => {
            if (!isPending) setIsOpen(false)
          }}
        >
          <div
            aria-labelledby="model-settings-dialog-title"
            aria-modal="true"
            className="settings-modal settings-modal-wide"
            role="dialog"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="settings-modal-header">
              <div>
                <p className="eyebrow">模型 API</p>
                <h2 id="model-settings-dialog-title">保存配置并切换供应商</h2>
              </div>
              <button
                aria-label="关闭模型 API 配置弹窗"
                className="toolbar-icon-button"
                disabled={isPending}
                type="button"
                onClick={() => setIsOpen(false)}
              >
                <X size={17} aria-hidden="true" />
              </button>
            </div>
            <div className="settings-modal-body">
              <div className="model-settings-form">
                <label className="field">
                  <span>模型供应商</span>
                  <select value={form.providerId} onChange={(event) => selectProvider(event.target.value)}>
                    {providerOptions.map(([providerId, displayName]) => (
                      <option key={providerId} value={providerId}>{displayName}</option>
                    ))}
                  </select>
                </label>
                <label className="field">
                  <span>供应商名称</span>
                  <input value={form.displayName} onChange={(event) => setForm((current) => ({ ...current, displayName: event.target.value }))} />
                </label>
                <label className="field">
                  <span>API 格式</span>
                  <select value={form.apiFormat} onChange={(event) => setForm((current) => ({ ...current, apiFormat: event.target.value as ApiFormat }))}>
                    {Object.entries(apiFormatLabels).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
                  </select>
                </label>
                <label className="field">
                  <span>Base URL</span>
                  <input placeholder={needsBaseUrl ? 'OpenAI 兼容格式必填' : '使用供应商默认地址'} type="url" value={form.baseUrl} onChange={(event) => setForm((current) => ({ ...current, baseUrl: event.target.value }))} />
                </label>
                <label className="field">
                  <span>默认模型 ID</span>
                  <input placeholder="例如 gpt-4.1-mini" value={form.defaultModelId} onChange={(event) => setForm((current) => ({ ...current, defaultModelId: event.target.value }))} />
                </label>
                <label className="field">
                  <span>API Key</span>
                  <input autoComplete="new-password" autoFocus={Boolean(configuredProvider)} placeholder={configuredProvider ? '留空以复用已保存密钥' : '只保存到系统安全存储'} type="password" value={apiKey} onChange={(event) => setApiKey(event.target.value)} />
                </label>
              </div>
              <div className="settings-provider__dialog-security">
                <ShieldCheck size={17} aria-hidden="true" />
                <p>API Key 只写入 macOS 系统安全存储，当前操作校验配置完整性，不会发起真实模型请求。</p>
              </div>
            </div>
            <div className="settings-modal-footer">
              <button className="ghost-button" disabled={isPending} type="button" onClick={() => void activate()}>
                {isActiveProvider ? '当前使用' : configuredProvider?.default_model_id ? '切换到此配置' : '先保存配置'}
              </button>
              <div className="settings-modal-footer-actions">
                <button className="ghost-button" disabled={isPending} type="button" onClick={() => setIsOpen(false)}>取消</button>
                <button className="primary-button" disabled={isPending || !canSubmit} type="button" onClick={() => void submit()}>
                  <KeyRound size={16} aria-hidden="true" />
                  {isPending ? '正在保存' : '保存并校验'}
                </button>
              </div>
            </div>
          </div>
        </div>
      ) : null}
    </section>
  )
}

function toApiFormat(value?: string): ApiFormat | undefined {
  if (value === 'openai_compatible' || value === 'anthropic_messages' || value === 'gemini' || value === 'ollama') return value
  return undefined
}

export default ModelSettingsPanel
