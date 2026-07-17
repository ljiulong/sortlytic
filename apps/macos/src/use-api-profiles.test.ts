import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  activateApiProfile,
  deleteApiProfile,
  getApiProfileRegistry,
  saveApiProfile,
  testApiProfile,
  type ApiProfileRegistryView,
  type ApiProfileTestResult,
  type TikhubApiProfileView,
} from './api-profiles'
import {
  API_PROFILE_REGISTRY_QUERY_KEY,
  useApiProfiles,
} from './use-api-profiles'

type MutationOperation<T = unknown> = () => Promise<T>

type CapturedMutationOptions = {
  mutationFn: (operation: MutationOperation) => Promise<unknown>
  onSettled?: () => Promise<unknown> | unknown
}

type CapturedQueryOptions = {
  queryKey: readonly string[]
  queryFn: () => Promise<unknown>
}

const invokeMock = vi.hoisted(() => vi.fn())
const invalidateQueriesMock = vi.hoisted(() => vi.fn())
const queryOptionsMock = vi.hoisted(() => ({ current: undefined as CapturedQueryOptions | undefined }))
const mutationOptionsMock = vi.hoisted(() => ({ current: [] as CapturedMutationOptions[] }))
const mutationVariablesMock = vi.hoisted(() => ({ current: [] as MutationOperation[] }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

vi.mock('@tanstack/react-query', () => ({
  useQuery: (options: CapturedQueryOptions) => {
    queryOptionsMock.current = options
    return {
      data: undefined,
      error: null,
      isLoading: false,
      isSuccess: true,
    }
  },
  useQueryClient: () => ({
    invalidateQueries: invalidateQueriesMock,
  }),
  useMutation: (options: CapturedMutationOptions) => {
    mutationOptionsMock.current.push(options)
    return {
      isPending: false,
      mutateAsync: async (operation: MutationOperation) => {
        mutationVariablesMock.current.push(operation)
        try {
          return await options.mutationFn(operation)
        } finally {
          await options.onSettled?.()
        }
      },
    }
  },
}))

const tikhubProfile: TikhubApiProfileView = {
  kind: 'tikhub',
  id: 'tikhub-1',
  name: '主账号',
  baseUrl: 'https://api.tikhub.io',
  revision: 1,
  status: 'success',
  maskedKey: 'tikh...[REDACTED]...1234',
  hasCredential: true,
  isActive: true,
  lastTestedAt: '2026-07-17T00:00:00Z',
  testSummary: {
    maskedAccount: 'st***@example.com',
    balance: 10,
    freeCredit: 2,
    availableCredit: 12,
    todayUsage: 1,
  },
  createdAt: '2026-07-17T00:00:00Z',
  updatedAt: '2026-07-17T00:00:00Z',
}

const registry: ApiProfileRegistryView = {
  activeProfileIds: {
    tikhub: tikhubProfile.id,
    ai: null,
  },
  tikhubProfiles: [tikhubProfile],
  aiProfiles: [],
}

const testResult: ApiProfileTestResult = {
  success: true,
  message: 'TikHub API 配置测试成功',
  registry,
}

function renderApiProfilesHook() {
  let result: ReturnType<typeof useApiProfiles> | undefined

  function Probe() {
    result = useApiProfiles()
    return null
  }

  renderToStaticMarkup(createElement(Probe))
  if (!result) throw new Error('API 配置 Hook 未完成渲染')
  return result
}

beforeEach(() => {
  invokeMock.mockReset()
  invalidateQueriesMock.mockReset()
  invalidateQueriesMock.mockResolvedValue(undefined)
  queryOptionsMock.current = undefined
  mutationOptionsMock.current = []
  mutationVariablesMock.current = []
})

describe('API 配置注册表客户端', () => {
  it('使用固定命令契约查询、保存、测试、切换和删除配置', async () => {
    invokeMock
      .mockResolvedValueOnce(registry)
      .mockResolvedValueOnce(registry)
      .mockResolvedValueOnce(testResult)
      .mockResolvedValueOnce(registry)
      .mockResolvedValueOnce(registry)

    await getApiProfileRegistry()
    await saveApiProfile({
      kind: 'tikhub',
      name: '主账号',
      baseUrl: 'https://api.tikhub.io',
      apiKey: 'tikhub-secret-value',
    })
    await testApiProfile('tikhub', 'tikhub-1')
    await activateApiProfile('tikhub', 'tikhub-1')
    await deleteApiProfile('tikhub', 'tikhub-1')

    expect(invokeMock.mock.calls).toEqual([
      ['get_api_profile_registry', { rootPath: null }],
      ['save_api_profile', {
        input: {
          kind: 'tikhub',
          name: '主账号',
          baseUrl: 'https://api.tikhub.io',
          apiKey: 'tikhub-secret-value',
        },
        rootPath: null,
      }],
      ['test_api_profile', { kind: 'tikhub', profileId: 'tikhub-1', rootPath: null }],
      ['activate_api_profile', { kind: 'tikhub', profileId: 'tikhub-1', rootPath: null }],
      ['delete_api_profile', { kind: 'tikhub', profileId: 'tikhub-1', rootPath: null }],
    ])
  })

  it('丢弃后端意外返回的完整密钥字段，并从异常消息中脱敏', async () => {
    const secret = 'tikhub-secret-that-must-not-leak'
    invokeMock.mockResolvedValueOnce({
      ...registry,
      credentials: { credential: { secret } },
      tikhubProfiles: [{ ...tikhubProfile, apiKey: secret }],
    })

    const safeRegistry = await getApiProfileRegistry()
    expect(JSON.stringify(safeRegistry)).not.toContain(secret)

    invokeMock.mockRejectedValueOnce(new Error(`连接失败：${secret}`))
    await expect(saveApiProfile({
      kind: 'tikhub',
      name: '主账号',
      baseUrl: 'https://api.tikhub.io',
      apiKey: secret,
    })).rejects.not.toThrow(secret)
  })
})

describe('API 配置 React Query Hook', () => {
  it('保存后自动测试，并刷新首条配置的后端自动激活结果', async () => {
    const secret = 'tikhub-secret-that-must-not-enter-query-cache'
    invokeMock
      .mockResolvedValueOnce({
        ...registry,
        activeProfileIds: { ...registry.activeProfileIds, tikhub: null },
        tikhubProfiles: [{ ...tikhubProfile, status: 'untested', isActive: false }],
      })
      .mockResolvedValueOnce(testResult)

    const hook = renderApiProfilesHook()
    const result = await hook.saveAndTestProfile({
      kind: 'tikhub',
      name: '主账号',
      baseUrl: 'https://api.tikhub.io',
      apiKey: secret,
    })

    expect(result).toEqual(testResult)
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'save_api_profile',
      'test_api_profile',
    ])
    expect(invalidateQueriesMock).toHaveBeenCalledWith({
      queryKey: API_PROFILE_REGISTRY_QUERY_KEY,
    })
    expect(JSON.stringify(mutationVariablesMock.current)).not.toContain(secret)
  })

  it('测试、手动切换和删除后均使安全注册表查询失效', async () => {
    invokeMock
      .mockResolvedValueOnce(testResult)
      .mockResolvedValueOnce(registry)
      .mockResolvedValueOnce(registry)

    const hook = renderApiProfilesHook()
    await hook.retestProfile('tikhub', 'tikhub-1')
    await hook.activateProfile('tikhub', 'tikhub-1')
    await hook.deleteProfile('tikhub', 'tikhub-1')

    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual([
      'test_api_profile',
      'activate_api_profile',
      'delete_api_profile',
    ])
    expect(invalidateQueriesMock).toHaveBeenCalledTimes(3)
    expect(invalidateQueriesMock).toHaveBeenNthCalledWith(1, {
      queryKey: API_PROFILE_REGISTRY_QUERY_KEY,
    })
  })

  it('查询使用独立键，失败保存也刷新列表且不在缓存变量或错误中暴露密钥', async () => {
    const secret = 'failed-secret-that-must-not-leak'
    invokeMock.mockRejectedValueOnce(new Error(`保存 ${secret} 失败`))

    const hook = renderApiProfilesHook()
    expect(queryOptionsMock.current?.queryKey).toEqual(API_PROFILE_REGISTRY_QUERY_KEY)
    await expect(hook.saveAndTestProfile({
      kind: 'tikhub',
      name: '失败账号',
      baseUrl: 'https://api.tikhub.dev',
      apiKey: secret,
    })).rejects.not.toThrow(secret)

    expect(invalidateQueriesMock).toHaveBeenCalledWith({
      queryKey: API_PROFILE_REGISTRY_QUERY_KEY,
    })
    expect(JSON.stringify(mutationVariablesMock.current)).not.toContain(secret)
  })
})
