// @vitest-environment happy-dom

// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { readFileSync } from 'node:fs'
// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { resolve } from 'node:path'
import { act, createElement, type ComponentProps } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  AiApiProfileView,
  ApiProfileRegistryView,
  TikhubApiProfileView,
} from './api-profiles'
import ApiProfilesDialog, { ApiProfileFormFields } from './ApiProfilesDialog'
import { i18n } from './i18n'

const {
  apiProfilesDialogReducer,
  buildSaveProfileInput,
  canReuseSavedCredential,
  canSaveProfile,
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

type MountedDialog = {
  container: HTMLDivElement
  rerender: (props: ComponentProps<typeof ApiProfilesDialog>) => void
  unmount: () => void
}

type MountedRoot = {
  container: HTMLDivElement
  root: Root
}

const mountedRoots = new Set<MountedRoot>()

function mountDialog(
  props: ComponentProps<typeof ApiProfilesDialog>,
): MountedDialog {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mountedRoot = { container, root }
  document.body.append(container)
  mountedRoots.add(mountedRoot)
  act(() => root.render(createElement(ApiProfilesDialog, props)))

  return {
    container,
    rerender(nextProps) {
      act(() => root.render(createElement(ApiProfilesDialog, nextProps)))
    },
    unmount() {
      if (!mountedRoots.delete(mountedRoot)) return
      act(() => root.unmount())
      container.remove()
    },
  }
}

async function flushDialogFocus() {
  await act(async () => {
    await new Promise<void>((resolveFrame) => {
      window.requestAnimationFrame(() => resolveFrame())
    })
  })
}

function dispatchDocumentKey(key: string, shiftKey = false) {
  const event = new KeyboardEvent('keydown', {
    bubbles: true,
    cancelable: true,
    key,
    shiftKey,
  })
  act(() => document.dispatchEvent(event))
  return event
}

function dispatchBackdropMouseDown(container: HTMLElement) {
  const backdrop = container.querySelector<HTMLElement>(
    '.api-profile-dialog__backdrop',
  )
  expect(backdrop).not.toBeNull()
  act(() => {
    backdrop?.dispatchEvent(new MouseEvent('mousedown', {
      bubbles: true,
      cancelable: true,
    }))
  })
}

beforeEach(async () => {
  await i18n.changeLanguage('zh-CN')
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  useApiProfilesMock.mockReset()
  useApiProfilesMock.mockReturnValue(hookResult())
  saveAndTestProfileMock.mockReset()
  retestProfileMock.mockReset()
  activateProfileMock.mockReset()
  deleteProfileMock.mockReset()
  refreshProfilesMock.mockReset()
})

afterEach(() => {
  for (const mountedRoot of mountedRoots) {
    act(() => mountedRoot.root.unmount())
    mountedRoot.container.remove()
  }
  mountedRoots.clear()
  document.body.replaceChildren()
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

  it('展示 TikHub 真实测试返回的额度构成、今日用量和检测时间', () => {
    const markup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    }))

    expect(markup).toContain('充值余额')
    expect(markup).toContain('$18.60')
    expect(markup).toContain('免费额度')
    expect(markup).toContain('$2.40')
    expect(markup).toContain('可用总额')
    expect(markup).toContain('$21.00')
    expect(markup).toContain('今日用量')
    expect(markup).toContain('<dd>37</dd>')
    expect(markup).toContain('最近检测')
    expect(markup).toContain('dateTime="2026-07-17T08:30:00Z"')
  })

  it('TikHub 测试未返回今日用量时不渲染该项', () => {
    useApiProfilesMock.mockReturnValue(hookResult({
      registry: {
        ...registry,
        tikhubProfiles: [{
          ...tikhubProfiles[0],
          testSummary: {
            ...tikhubProfiles[0].testSummary!,
            todayUsage: null,
          },
        }],
      },
    }))

    const markup = renderToStaticMarkup(createElement(ApiProfilesDialog, {
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    }))

    expect(markup).not.toContain('今日用量')
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
    expect(markup).toContain('完整性校验通过')
    expect(markup).toContain('重新校验')
    expect(markup).not.toContain('配置测试成功')
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

describe('ApiProfilesDialog 真实 DOM 交互', () => {
  it('Escape 与点击遮罩均会请求关闭弹窗', () => {
    const onClose = vi.fn()
    const mounted = mountDialog({
      isOpen: true,
      kind: 'tikhub',
      onClose,
    })

    const escapeEvent = dispatchDocumentKey('Escape')
    expect(escapeEvent.defaultPrevented).toBe(true)
    expect(onClose).toHaveBeenCalledTimes(1)

    dispatchBackdropMouseDown(mounted.container)
    expect(onClose).toHaveBeenCalledTimes(2)
  })

  it('忙碌期间 Escape 与遮罩都不能关闭弹窗', () => {
    useApiProfilesMock.mockReturnValue(hookResult({ isPending: true }))
    const onClose = vi.fn()
    const mounted = mountDialog({
      isOpen: true,
      kind: 'tikhub',
      onClose,
    })

    const escapeEvent = dispatchDocumentKey('Escape')
    dispatchBackdropMouseDown(mounted.container)

    expect(escapeEvent.defaultPrevented).toBe(true)
    expect(onClose).not.toHaveBeenCalled()
    expect(mounted.container.querySelector('[role="dialog"]')
      ?.getAttribute('aria-busy')).toBe('true')
  })

  it('Tab 与 Shift+Tab 在弹窗首尾可聚焦元素间循环', () => {
    const mounted = mountDialog({
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    })
    const dialog = mounted.container.querySelector<HTMLElement>('[role="dialog"]')
    const focusable = Array.from(dialog?.querySelectorAll<HTMLElement>([
      'button:not([disabled])',
      'input:not([disabled])',
      'select:not([disabled])',
      'textarea:not([disabled])',
      '[href]',
      '[tabindex]:not([tabindex="-1"])',
    ].join(',')) ?? [])
    const first = focusable[0]
    const last = focusable[focusable.length - 1]
    expect(first).toBeDefined()
    expect(last).toBeDefined()

    first?.focus()
    const reverseTabEvent = dispatchDocumentKey('Tab', true)
    expect(reverseTabEvent.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(last)

    const forwardTabEvent = dispatchDocumentKey('Tab')
    expect(forwardTabEvent.defaultPrevented).toBe(true)
    expect(document.activeElement).toBe(first)
  })

  it('关闭或卸载后都会把焦点还给原触发按钮', async () => {
    const trigger = document.createElement('button')
    trigger.textContent = '配置 TikHub API'
    document.body.append(trigger)
    trigger.focus()
    const props = {
      isOpen: true,
      kind: 'tikhub' as const,
      onClose: vi.fn(),
    }
    const mounted = mountDialog(props)
    await flushDialogFocus()
    expect(document.activeElement).toBe(
      mounted.container.querySelector('[role="dialog"]'),
    )

    mounted.rerender({ ...props, isOpen: false })
    expect(document.activeElement).toBe(trigger)

    mounted.rerender(props)
    await flushDialogFocus()
    mounted.unmount()
    expect(document.activeElement).toBe(trigger)
  })

  it('即使安全视图意外携带完整密钥字段，完整值也不进入 DOM', () => {
    const fullSecret = 'tikhub-full-secret-must-never-enter-dom'
    const registryWithUnexpectedSecret = {
      ...registry,
      tikhubProfiles: registry.tikhubProfiles.map((profile) => ({
        ...profile,
        apiKey: profile.id === 'tikhub-main' ? fullSecret : undefined,
      })),
    }
    useApiProfilesMock.mockReturnValue(hookResult({
      registry: registryWithUnexpectedSecret,
    }))

    const mounted = mountDialog({
      isOpen: true,
      kind: 'tikhub',
      onClose: vi.fn(),
    })

    expect(mounted.container.textContent).not.toContain(fullSecret)
    expect(mounted.container.innerHTML).not.toContain(fullSecret)
    expect(Array.from(mounted.container.querySelectorAll('input'))
      .some((input) => input.value.includes(fullSecret))).toBe(false)
    expect(mounted.container.textContent).toContain('tikh••••••5812')
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

  it('切换 AI 供应商后不能复用旧供应商密钥', () => {
    const openAiDraft = createProfileDraft('ai', aiProfiles[0])
    if (openAiDraft.kind !== 'ai') throw new Error('Expected an AI profile draft')
    const anthropicDraft = {
      ...openAiDraft,
      providerType: 'anthropic' as const,
      apiFormat: 'anthropic_messages' as const,
      baseUrl: 'https://api.anthropic.com',
      apiKey: '',
    }

    expect(canReuseSavedCredential(openAiDraft, aiProfiles[0])).toBe(true)
    expect(canReuseSavedCredential(anthropicDraft, aiProfiles[0])).toBe(false)
    expect(canSaveProfile(anthropicDraft, aiProfiles[0])).toBe(false)
    expect(canSaveProfile({ ...anthropicDraft, apiKey: 'new-anthropic-key' }, aiProfiles[0]))
      .toBe(true)
    expect(canReuseSavedCredential({
      ...anthropicDraft,
      providerType: 'ollama',
      apiFormat: 'ollama',
      baseUrl: 'http://127.0.0.1:11434',
    }, aiProfiles[0])).toBe(false)
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
    const dialogCss = readFileSync(resolve('src/ApiProfilesDialog.css'), 'utf8')
    expect(dialogCss).toMatch(/min-height:\s*40px/u)
    expect(dialogCss).toContain('@media (prefers-reduced-motion: reduce)')
    expect(dialogCss).toContain('@media (max-width: 560px)')
    expect(dialogCss).not.toMatch(/transition\s*:\s*all/iu)
    expect(dialogCss).not.toContain('backdrop-filter')
  })
})
