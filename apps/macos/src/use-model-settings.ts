import { useState } from 'react'
import {
  backendErrorMessage,
  createModelProvider,
  listModelProviders,
  saveSecret,
  setActiveModelProvider,
  setDefaultModel,
  testModelProvider,
  testSecretConnection,
  type ProviderTestResult,
  updateModelProvider,
  updateSecret,
  upsertModelProfile,
} from './backend-api'

export type ModelSettingsInput = {
  providerId: string
  displayName: string
  apiFormat: 'openai_compatible' | 'anthropic_messages' | 'gemini' | 'ollama'
  baseUrl: string
  defaultModelId: string
  apiKey: string
}

type ModelSettingsOptions = {
  refreshWorkbench: () => Promise<unknown>
  setActionMessage: (message: string) => void
}

export function useModelSettings({
  refreshWorkbench,
  setActionMessage,
}: ModelSettingsOptions) {
  const [modelValidationResult, setModelValidationResult] = useState<ProviderTestResult>()
  const [isModelSettingsPending, setIsModelSettingsPending] = useState(false)
  const [isModelActivationPending, setIsModelActivationPending] = useState(false)

  const saveAndValidateModelProvider = async (input: ModelSettingsInput) => {
    assertTauriRuntime()
    setIsModelSettingsPending(true)
    setModelValidationResult(undefined)

    try {
      const providers = await listModelProviders()
      const existingProvider = providers.find(
        (provider) => provider.provider_id === input.providerId,
      )
      const apiKey = input.apiKey.trim()
      const secretRefId = existingProvider?.secret_ref_id
      if (!secretRefId && apiKey.length < 8) {
        throw new Error('请先输入至少 8 位模型 API Key')
      }
      if (secretRefId && apiKey) {
        await updateSecret(secretRefId, apiKey)
      }
      const savedSecretRefId = secretRefId ?? (await saveSecret({
        provider_type: 'model_provider',
        provider_id: input.providerId,
        secret: apiKey,
        alias: `${input.displayName} API Key`,
      })).id
      await testSecretConnection(savedSecretRefId)

      const providerInput = {
        provider_id: input.providerId,
        display_name: input.displayName,
        enabled: true,
        auth_type: 'api_key' as const,
        secret_ref_id: savedSecretRefId,
        base_url: input.baseUrl.trim() || null,
        api_format: input.apiFormat,
        region: null,
        cost_policy_json: null,
        rate_limit_policy_json: null,
        health_check_json: null,
      }

      if (existingProvider) {
        await updateModelProvider(input.providerId, providerInput)
      } else {
        await createModelProvider(providerInput)
      }

      await upsertModelProfile({
        provider_id: input.providerId,
        model_id: input.defaultModelId,
        display_name: input.defaultModelId,
        capabilities_json: null,
        context_window: null,
        supports_structured_output: false,
        supports_streaming: false,
        supports_tools: false,
        supports_vision: false,
        enabled: true,
      })
      await setDefaultModel(input.providerId, input.defaultModelId)
      await setActiveModelProvider(input.providerId)

      const result = await testModelProvider(input.providerId, input.defaultModelId)
      setModelValidationResult(result)
      setActionMessage(result.message)
      await refreshWorkbench()
      return result
    } catch (error) {
      setActionMessage(backendErrorMessage(error))
      throw error
    } finally {
      setIsModelSettingsPending(false)
    }
  }

  const activateModelProvider = async (providerId: string) => {
    assertTauriRuntime()
    setIsModelActivationPending(true)
    try {
      await setActiveModelProvider(providerId)
      setActionMessage('模型 API 配置已切换')
      await refreshWorkbench()
    } catch (error) {
      setActionMessage(backendErrorMessage(error))
      throw error
    } finally {
      setIsModelActivationPending(false)
    }
  }

  return {
    saveAndValidateModelProvider,
    modelValidationResult,
    isModelSettingsPending,
    isModelActivationPending,
    activateModelProvider,
  }
}

function assertTauriRuntime() {
  if (
    typeof window === 'undefined'
    || !(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  ) {
    throw new Error('请在打包后的 macOS 应用内使用后端能力')
  }
}
