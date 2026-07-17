// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { readFileSync } from 'node:fs'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  AiApiProfileView,
  ApiProfileRegistryView,
  TikhubApiProfileView,
} from './api-profiles'
import ApiProfilesDialog, { ApiProfileFormFields } from './ApiProfilesDialog'

const {
  apiProfilesDialogReducer,
  buildSaveProfileInput,
  createProfileDraft,
  initialApiProfilesDialogState,
} = ApiProfilesDialog.testUtils

const useApiProfilesMock = vi.hoisted(() => vi.fn())
const saveAndTestProfileMock = vi.hoisted(() => vi.fn())
const retestProfileMock = vi.hoisted(() => vi.fn())
const activateProfileMock = vi.hoisted(() => vi.fn())
const deleteProfileMock = vi.hoisted(() => vi.fn())
const refreshProfilesMock = vi.hoisted(() => vi.fn())

vi.mock('./use-api-profiles', () => ({
  useApiProfiles: useApiProfilesMock,
}))

const tikhubProfiles: TikhubApiProfileView[] = [
  {
    kind: 'tikhub',
    id: 'tikhub-main',
    name: '主数据账号',
    baseUrl: 'https://api.tikhub.io',
    revision: 3,
    status: 'success',
    maskedKey: 'tikh••••••5812',
    hasCredential: true,
    isActive: true,
    lastTestedAt: '2026-07-17T08:30:00Z',
    testSummary: {
      maskedAccount: 'st***@example.com',
      balance: 18.6,
      freeCredit: 2.4,
      availableCredit: 21,
      todayUsage: 37,
    },
    createdAt: '2026-07-16T08:30:00Z',
    updatedAt: '2026-07-17T08:30:00Z',
  },
  {
    kind: 'tikhub',
    id: 'tikhub-backup',
    name: '备用账号',
    baseUrl: 'https://api.tikhub.dev',
    revision: 1,
    status: 'failed',
    maskedKey: 'tikh••••••0941',
    hasCredential: true,
    isActive: false,
    lastTestedAt: '2026-07-17T08:10:00Z',
    testSummary: null,
    createdAt: '2026-07-17T08:00:00Z',
    updatedAt: '2026-07-17T08:10:00Z',
  },
]

const aiProfiles: AiApiProfileView[] = [
  {
    kind: 'ai',
    id: 'ai-openai-main',
    name: '内容整理模型',
    providerType: 'openai',
    apiFormat: 'openai_compatible',
    baseUrl: 'https://api.openai.com/v1',
    defaultModelId: 'gpt-4.1-mini',
    revision: 2,
    status: 'success',
    maskedKey: 'sk-p••••••C7x9',
    hasCredential: true,
    isActive: true,
    lastTestedAt: '2026-07-17T08:35:00Z',
    createdAt: '2026-07-16T08:35:00Z',
    updatedAt: '2026-07-17T08:35:00Z',
  },
]

const registry: ApiProfileRegistryView = {
  activeProfileIds: {
    tikhub: 'tikhub-main',
    ai: 'ai-openai-main',
  },
  tikhubProfiles,
  aiProfiles,
}

function hookResult(overrides: Record<string, unknown> = {}) {
  return {
    registry,
    registryQuery: {
      error: null,
      isLoading: false,
    },
    saveAndTestProfile: saveAndTestProfileMock,
    retestProfile: retestProfileMock,
    activateProfile: activateProfileMock,
    deleteProfile: deleteProfileMock,
    refreshProfiles: refreshProfilesMock,
    isSaving: false,
    isTesting: false,
    isActivating: false,
    isDeleting: false,
    isPending: false,
    ...overrides,
  }
}

beforeEach(() => {
  useApiProfilesMock.mockReset()
  useApiProfilesMock.mockReturnValue(hookResult())
  saveAndTestProfileMock.mockReset()
  retestProfileMock.mockReset()
  activateProfileMock.mockReset()
  deleteProfileMock.mockReset()
  refreshProfilesMock.mockReset()
})

describe('ApiProfilesDialog 列表优先界面', () => {
  it('打开 TikHub 弹窗时先展示保存列表、当前项、安全密钥视图和状态', () => {
    const secret = 'tikhub-secret-must-never-render'
    const markup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    }))

    expect(markup.match(/role="dialog"/gu)).toHaveLength(1)
    expect(markup).toContain('管理 TikHub API 配置')
    expect(markup).toContain('主数据账号')
    expect(markup).toContain('备用账号')
    expect(markup).toContain('https://api.tikhub.io')
    expect(markup).toContain('tikh••••••5812')
    expect(markup).toContain('当前')
    expect(markup).toContain('已验证')
    expect(markup).toContain('测试失败')
    expect(markup).not.toContain(secret)
    expect(markup).not.toContain('<form')
  })

  it('空注册表仍先展示空列表与新增按钮，不直接进入表单', () => {
    useApiProfilesMock.mockReturnValue(hookResult({
      registry: {
        activeProfileIds: { tikhub: null, ai: null },
        tikhubProfiles: [],
        aiProfiles: [],
      },
    }))

    const markup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'ai',
      onClose: vi.fn(),
    }))

    expect(markup).toContain('尚未保存 AI API 配置')
    expect(markup).toContain('新增 AI 配置')
    expect(markup).not.toContain('<form')
    expect(markup).not.toContain('默认模型 ID</span><input')
  })

  it('AI 列表显示供应商、模型、端点和脱敏密钥', () => {
    const markup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'ai',
      onClose: vi.fn(),
    }))

    expect(markup).toContain('内容整理模型')
    expect(markup).toContain('OpenAI')
    expect(markup).toContain('gpt-4.1-mini')
    expect(markup).toContain('https://api.openai.com/v1')
    expect(markup).toContain('sk-p••••••C7x9')
  })

  it('加载中与错误态不会错误显示最终空状态', () => {
    useApiProfilesMock.mockReturnValue(hookResult({
      registry: undefined,
      registryQuery: { error: null, isLoading: true },
    }))
    const loadingMarkup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    }))
    expect(loadingMarkup).toContain('正在读取保存的配置')
    expect(loadingMarkup).not.toContain('尚未保存 TikHub API 配置')

    useApiProfilesMock.mockReturnValue(hookResult({
      registry: undefined,
      registryQuery: { error: new Error('无法读取配置'), isLoading: false },
    }))
    const errorMarkup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    }))
    expect(errorMarkup).toContain('无法读取 API 配置')
    expect(errorMarkup).toContain('重新读取')
    expect(errorMarkup).not.toContain('尚未保存 TikHub API 配置')
  })
})

describe('ApiProfilesDialog 表单与状态机', () => {
  it('TikHub 与 AI 表单都包含完整字段，密钥始终是本地密码输入框', () => {
    const tikhubMarkup = renderToStaticMarkup(createElement(ApiProfileFormFields, {
      draft: createProfileDraft('tikhub'),
      disabled: false,
      isEditing: false,
      onChange: vi.fn(),
    }))
    expect(tikhubMarkup).toContain('配置名称')
    expect(tikhubMarkup).toContain('API 端点')
    expect(tikhubMarkup).toContain('API Token')
    expect(tikhubMarkup).toContain('type="password"')

    const aiMarkup = renderToStaticMarkup(createElement(ApiProfileFormFields, {
      draft: createProfileDraft('ai'),
      disabled: false,
      isEditing: false,
      onChange: vi.fn(),
    }))
    expect(aiMarkup).toContain('供应商类型')
    expect(aiMarkup).toContain('API 格式')
    expect(aiMarkup).toContain('Base URL')
    expect(aiMarkup).toContain('默认模型 ID')
    expect(aiMarkup).toContain('API Key')
    expect(aiMarkup).toContain('type="password"')
  })

  it('需重新绑定的编辑态明确要求重新输入密钥，不承诺留空保留', () => {
    const rebindMarkup = renderToStaticMarkup(createElement(ApiProfileFormFields, {
      canKeepSavedKey: false,
      draft: createProfileDraft('tikhub', {
        ...tikhubProfiles[0],
        status: 'needs_rebind',
        hasCredential: false,
      }),
      disabled: false,
      isEditing: true,
      onChange: vi.fn(),
    }))

    expect(rebindMarkup).toContain('重新输入 TikHub Token')
    expect(rebindMarkup).toContain('required=""')
    expect(rebindMarkup).not.toContain('留空会保留原密钥')
  })

  it('编辑时密钥留空会从保存输入中省略，填写后才传递新密钥', () => {
    const blankKeyDraft = createProfileDraft('ai', aiProfiles[0])
    const preservedInput = buildSaveProfileInput(blankKeyDraft, aiProfiles[0])
    expect(preservedInput).not.toHaveProperty('apiKey')

    const changedInput = buildSaveProfileInput(
      { ...blankKeyDraft, apiKey: 'replacement-secret-value' },
      aiProfiles[0],
    )
    expect(changedInput).toMatchObject({
      id: 'ai-openai-main',
      apiKey: 'replacement-secret-value',
    })
  })

  it('新增、编辑、返回与删除确认都在同一个弹窗视图状态中切换', () => {
    const adding = apiProfilesDialogReducer(initialApiProfilesDialogState, {
      type: 'add',
    })
    expect(adding).toEqual({
      view: 'form',
      editingProfileId: null,
      confirmingDeleteId: null,
    })

    const editing = apiProfilesDialogReducer(adding, {
      type: 'edit',
      profileId: 'tikhub-main',
    })
    expect(editing).toMatchObject({
      view: 'form',
      editingProfileId: 'tikhub-main',
    })

    const returned = apiProfilesDialogReducer(editing, { type: 'showList' })
    const confirming = apiProfilesDialogReducer(returned, {
      type: 'requestDelete',
      profileId: 'tikhub-main',
    })
    expect(confirming).toMatchObject({
      view: 'list',
      confirmingDeleteId: 'tikhub-main',
    })
    expect(apiProfilesDialogReducer(confirming, { type: 'cancelDelete' }))
      .toMatchObject({ confirmingDeleteId: null })
  })

  it('忙碌期间关闭与所有配置动作均禁用', () => {
    useApiProfilesMock.mockReturnValue(hookResult({ isPending: true }))
    const markup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    }))

    expect(markup).toContain('aria-busy="true"')
    expect(markup).toContain('aria-label="关闭 TikHub API 配置弹窗"')
    expect(markup).toMatch(/aria-label="关闭 TikHub API 配置弹窗"[^>]*disabled/gu)
    expect((markup.match(/disabled=""/gu) ?? []).length).toBeGreaterThan(5)
  })
})

describe('ApiProfilesDialog 样式安全线', () => {
  it('提供 40px 点击区、减少动态效果和响应式布局，不使用被禁用效果', () => {
    const dialogCss = readFileSync(new URL('./ApiProfilesDialog.css', import.meta.url), 'utf8')
    expect(dialogCss).toMatch(/min-height:\s*40px/u)
    expect(dialogCss).toContain('@media (prefers-reduced-motion: reduce)')
    expect(dialogCss).toContain('@media (max-width: 560px)')
    expect(dialogCss).not.toMatch(/transition\s*:\s*all/iu)
    expect(dialogCss).not.toContain('backdrop-filter')
  })
})
