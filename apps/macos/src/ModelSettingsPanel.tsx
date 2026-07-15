import { Bot, KeyRound, ShieldCheck } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import type { ModelProviderView, ProviderTestResult } from './backend-api'
import type { ModelSettingsInput } from './use-workbench-backend'

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
  {
    providerId: 'openai',
    displayName: 'OpenAI',
    apiFormat: 'openai_compatible',
    baseUrl: 'https://api.openai.com/v1',
  },
  {
    providerId: 'anthropic',
    displayName: 'Anthropic',
    apiFormat: 'anthropic_messages',
    baseUrl: 'https://api.anthropic.com',
  },
  {
    providerId: 'gemini',
    displayName: 'Google Gemini',
    apiFormat: 'gemini',
    baseUrl: 'https://generativelanguage.googleapis.com',
  },
  {
    providerId: 'custom-openai',
    displayName: '自定义 OpenAI 兼容服务',
    apiFormat: 'openai_compatible',
    baseUrl: '',
  },
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
  const initialPreset = providerPresets.find(
    (preset) => preset.providerId === initialProvider?.provider_id,
  ) ?? providerPresets[0]
  const [form, setForm] = useState<Omit<ModelSettingsInput, 'apiKey'>>({
    providerId: initialProvider?.provider_id ?? initialPreset.providerId,
    displayName: initialProvider?.display_name ?? initialPreset.displayName,
    apiFormat: toApiFormat(initialProvider?.api_format) ?? initialPreset.apiFormat,
    baseUrl: initialProvider?.base_url ?? initialPreset.baseUrl,
    defaultModelId: initialProvider?.default_model_id ?? '',
  })
  const [apiKey, setApiKey] = useState('')
  const didSyncProviders = useRef(false)
  const configuredProvider = providers.find(
    (provider) => provider.provider_id === form.providerId,
  )
  const activeProvider = providers.find((provider) => provider.enabled)
  const isActiveProvider = activeProvider?.provider_id === form.providerId
  const isValidated = result?.success && result.provider_id === form.providerId
  const providerOptions = Array.from(
    new Map(
      [
        ...providerPresets.map((preset) => [preset.providerId, preset.displayName] as const),
        ...providers.map((provider) => [provider.provider_id, provider.display_name] as const),
      ],
    ).entries(),
  )
  const needsBaseUrl = form.apiFormat === 'openai_compatible'
  const canSubmit =
    (Boolean(configuredProvider) || apiKey.trim().length >= 8) &&
    form.displayName.trim().length > 0 &&
    form.defaultModelId.trim().length > 0 &&
    (!needsBaseUrl || form.baseUrl.trim().length > 0)

  const selectProvider = (providerId: string) => {
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
  }

  useEffect(() => {
    if (didSyncProviders.current || providers.length === 0) return
    didSyncProviders.current = true
    const savedProvider = providers.find((provider) => provider.enabled) ?? providers[0]
    if (!savedProvider) return
    setForm({
      providerId: savedProvider.provider_id,
      displayName: savedProvider.display_name,
      apiFormat: toApiFormat(savedProvider.api_format) ?? 'openai_compatible',
      baseUrl: savedProvider.base_url ?? '',
      defaultModelId: savedProvider.default_model_id ?? '',
    })
  }, [providers])

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
    } catch {
      // 后端错误会显示在工作区顶部状态区，密钥仍保留以便用户修正后重试。
    }
  }

  return (
    <section className="glass-panel compact-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">模型 API 设置</p>
          <h2>供应商、模型与安全密钥</h2>
        </div>
        <span
          className="status-pill"
          data-tone={isActiveProvider || isValidated ? 'success' : configuredProvider ? 'info' : 'warning'}
        >
          {isActiveProvider ? '当前使用' : isValidated ? '配置已校验' : configuredProvider ? '已保存' : '待配置'}
        </span>
      </div>

      <div className="model-settings-form">
        <label className="field">
          <span>模型供应商</span>
          <select value={form.providerId} onChange={(event) => selectProvider(event.target.value)}>
            {providerOptions.map(([providerId, displayName]) => (
              <option key={providerId} value={providerId}>
                {displayName}
              </option>
            ))}
          </select>
        </label>

        <label className="field">
          <span>供应商名称</span>
          <input
            value={form.displayName}
            onChange={(event) => setForm((current) => ({
              ...current,
              displayName: event.target.value,
            }))}
          />
        </label>

        <label className="field">
          <span>API 格式</span>
          <select
            value={form.apiFormat}
            onChange={(event) => setForm((current) => ({
              ...current,
              apiFormat: event.target.value as ApiFormat,
            }))}
          >
            {Object.entries(apiFormatLabels).map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>

        <label className="field">
          <span>Base URL</span>
          <input
            placeholder={needsBaseUrl ? 'OpenAI 兼容格式必填' : '使用供应商默认地址'}
            type="url"
            value={form.baseUrl}
            onChange={(event) => setForm((current) => ({
              ...current,
              baseUrl: event.target.value,
            }))}
          />
        </label>

        <label className="field">
          <span>默认模型 ID</span>
          <input
            placeholder="例如 gpt-4.1-mini"
            value={form.defaultModelId}
            onChange={(event) => setForm((current) => ({
              ...current,
              defaultModelId: event.target.value,
            }))}
          />
        </label>

        <label className="field">
          <span>API Key</span>
          <input
            autoComplete="new-password"
            placeholder={configuredProvider ? '留空以复用已保存密钥' : '只保存到系统安全存储'}
            type="password"
            value={apiKey}
            onChange={(event) => setApiKey(event.target.value)}
          />
        </label>
      </div>

      <div className="model-security-note">
        <ShieldCheck size={17} aria-hidden="true" />
        <p>
          API Key 只写入 macOS 系统安全存储，应用数据库仅保存密钥引用。当前操作校验配置完整性，不会发起真实模型请求。
        </p>
      </div>

      <div className="model-settings-footer">
        <div className="model-config-summary">
          <Bot size={17} aria-hidden="true" />
          <span>
            {configuredProvider
              ? `${configuredProvider.display_name} · ${configuredProvider.default_model_id ?? '未设置默认模型'}`
              : '尚未保存该供应商配置'}
          </span>
        </div>
        <button
          className="ghost-button"
          disabled={isPending || isActiveProvider || !configuredProvider?.default_model_id}
          type="button"
          onClick={() => {
            void activateModelProvider(form.providerId).catch(() => undefined)
          }}
        >
          {isActiveProvider
            ? '当前使用'
            : configuredProvider?.default_model_id
              ? '切换到此配置'
              : '先配置默认模型'}
        </button>
        <button
          className="primary-button"
          disabled={isPending || !canSubmit}
          type="button"
          onClick={() => {
            void submit()
          }}
        >
          <KeyRound size={16} aria-hidden="true" />
          {isPending ? '正在保存' : '保存并校验'}
        </button>
      </div>
    </section>
  )
}

function toApiFormat(value?: string): ApiFormat | undefined {
  if (
    value === 'openai_compatible' ||
    value === 'anthropic_messages' ||
    value === 'gemini' ||
    value === 'ollama'
  ) {
    return value
  }

  return undefined
}

export default ModelSettingsPanel
