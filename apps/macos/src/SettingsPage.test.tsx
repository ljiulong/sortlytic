// @vitest-environment happy-dom

// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { readFileSync } from 'node:fs'
// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { resolve } from 'node:path'
import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ApiProfileRegistryView } from './api-profiles'
import { i18n } from './i18n'
import SettingsPage from './SettingsPage'

const { settingsApiDialogReducer } = SettingsPage.testUtils

const useApiProfilesMock = vi.hoisted(() => vi.fn())
const promptApiMocks = vi.hoisted(() => ({
  activatePromptVersion: vi.fn(),
  createPromptVersion: vi.fn(),
  listPromptTemplates: vi.fn(),
  listPromptVersions: vi.fn(),
}))

vi.mock('./use-api-profiles', () => ({
  useApiProfiles: useApiProfilesMock,
}))

vi.mock('./ApiProfilesDialog', () => ({
  default: ({ isOpen, kind }: { isOpen: boolean; kind: string }) => (
    isOpen ? <aside data-api-dialog-kind={kind} role="dialog">API 配置列表</aside> : null
  ),
}))

vi.mock('./UpdateSettingsPanel', () => ({
  default: () => <section data-testid="about-card">关于 Sortlytic</section>,
}))

vi.mock('./backend-api', () => promptApiMocks)

const secret = 'full-secret-must-never-render'
const registry: ApiProfileRegistryView = {
  activeProfileIds: {
    tikhub: 'tikhub-main',
    ai: 'ai-main',
  },
  tikhubProfiles: [{
    kind: 'tikhub',
    id: 'tikhub-main',
    name: '主数据账号',
    baseUrl: 'https://api.tikhub.io',
    revision: 2,
    status: 'success',
    maskedKey: 'tikh••••5812',
    hasCredential: true,
    isActive: true,
    lastTestedAt: '2026-07-17T08:30:00Z',
    testSummary: null,
    createdAt: '2026-07-16T08:30:00Z',
    updatedAt: '2026-07-17T08:30:00Z',
    apiKey: secret,
  } as never],
  aiProfiles: [{
    kind: 'ai',
    id: 'ai-main',
    name: '内容整理模型',
    providerType: 'openai',
    apiFormat: 'openai_compatible',
    baseUrl: 'https://api.openai.com/v1',
    defaultModelId: 'gpt-4.1-mini',
    revision: 4,
    status: 'success',
    maskedKey: 'sk-p••••C7x9',
    hasCredential: true,
    isActive: true,
    lastTestedAt: '2026-07-17T08:35:00Z',
    createdAt: '2026-07-16T08:35:00Z',
    updatedAt: '2026-07-17T08:35:00Z',
    apiKey: secret,
  } as never],
}

const backend = {
  data: {
    workspace: {
      health: '运行正常',
      lastBackup: '2026-07-17 08:00',
      storage: '/Users/test/Library/Application Support/com.steven.sortlytic/default-workspace',
    },
    runtimeMode: 'backend',
    modelProviders: [],
    tikhubConnector: null,
  },
}

const promptTemplate = {
  id: 'prompt-template-collection',
  template_key: 'collection_plan_from_text',
  name: '自然语言采集计划生成',
  task_type: 'collection_plan',
  description: '把自然语言需求转为结构化采集计划',
  output_schema_id: 'collection_plan_v3',
  is_builtin: true,
  created_at: '2026-07-17T08:00:00Z',
  updated_at: '2026-07-17T08:00:00Z',
}

const activePromptVersion = {
  id: 'prompt-version-3',
  template_id: promptTemplate.id,
  version: 3,
  content: '只输出符合 collection_plan_v3 的 JSON，不得猜测缺失字段。',
  change_note: '约束结构化采集计划',
  status: 'active',
  created_at: '2026-07-17T08:00:00Z',
  activated_at: '2026-07-17T08:05:00Z',
  rollback_from_version: null,
  content_hash: 'safe-content-hash',
}

type MountedSettings = {
  container: HTMLDivElement
  root: Root
}

const mountedSettings = new Set<MountedSettings>()

function mountSettingsPage() {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  document.body.append(container)
  mountedSettings.add(mounted)
  act(() => root.render(createElement(SettingsPage, { backend: backend as never })))
  return mounted
}

async function flushPromptSettings() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

function changeControlValue(
  control: HTMLInputElement | HTMLTextAreaElement,
  value: string,
) {
  const prototype = control instanceof HTMLTextAreaElement
    ? HTMLTextAreaElement.prototype
    : HTMLInputElement.prototype
  const nativeSetter = Object.getOwnPropertyDescriptor(prototype, 'value')?.set
  act(() => {
    nativeSetter?.call(control, value)
    control.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

beforeEach(async () => {
  await i18n.changeLanguage('zh-CN')
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  useApiProfilesMock.mockReset()
  useApiProfilesMock.mockReturnValue({
    registry,
    registryQuery: {
      error: null,
      isLoading: false,
    },
  })
  promptApiMocks.listPromptTemplates.mockReset()
  promptApiMocks.listPromptTemplates.mockResolvedValue([promptTemplate])
  promptApiMocks.listPromptVersions.mockReset()
  promptApiMocks.listPromptVersions.mockResolvedValue([activePromptVersion])
  promptApiMocks.createPromptVersion.mockReset()
  promptApiMocks.activatePromptVersion.mockReset()
})

afterEach(() => {
  for (const mounted of mountedSettings) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedSettings.clear()
  document.body.replaceChildren()
})

describe('SettingsPage API 配置入口', () => {
  it('只显示两个 API 配置按钮，并从安全注册表显示当前配置状态', () => {
    const markup = renderToStaticMarkup(createElement(SettingsPage, {
      backend: backend as never,
    }))
    const apiActions = markup.match(
      /<div class="settings-page__api-actions">([\s\S]*?)<\/div>/u,
    )?.[1] ?? ''

    expect(apiActions.match(/<button/gu)).toHaveLength(2)
    expect(apiActions).toContain('配置 TikHub API')
    expect(apiActions).toContain('配置 AI API')
    expect(markup).toContain('主数据账号 当前配置')
    expect(markup).toContain('内容整理模型 当前配置')
    expect(markup).toContain('应用身份与数据位置')
    expect(markup).toContain('关于 Sortlytic')
    expect(markup).not.toContain('旧 TikHub 内联面板')
    expect(markup).not.toContain('旧 AI 内联面板')
    expect(markup).not.toContain('tikh••••5812')
    expect(markup).not.toContain('sk-p••••C7x9')
    expect(markup).not.toContain(secret)
  })

  it('把关于卡片从本地环境分组移出，并放在设置页最底部', () => {
    const container = document.createElement('div')
    container.innerHTML = renderToStaticMarkup(createElement(SettingsPage, {
      backend: backend as never,
    }))
    const page = container.querySelector('.settings-page')
    const groups = Array.from(page?.querySelectorAll('.settings-page__group') ?? [])
    const localGroup = groups[groups.length - 1]
    const aboutCard = page?.querySelector('[data-testid="about-card"]')

    expect(localGroup?.contains(aboutCard ?? null)).toBe(false)
    expect(page?.lastElementChild).toBe(aboutCard)
  })

  it('两个按钮映射到各自弹窗，关闭后回到无弹窗状态', () => {
    expect(settingsApiDialogReducer(null, { type: 'open', kind: 'tikhub' }))
      .toBe('tikhub')
    expect(settingsApiDialogReducer('tikhub', { type: 'open', kind: 'ai' }))
      .toBe('ai')
    expect(settingsApiDialogReducer('ai', { type: 'close' })).toBeNull()
  })

  it('按钮在窄窗改为单列，并保留明确焦点样式与双主题令牌', () => {
    const css = readFileSync(resolve('src/SettingsPage.css'), 'utf8')

    expect(css).toContain('.settings-page__api-actions')
    expect(css).toContain('.settings-page__api-button:focus-visible')
    expect(css).toContain('@media (max-width: 680px)')
    expect(css).toContain('grid-template-columns: 1fr;')
    expect(css).toContain('var(--surface-raised)')
    expect(css).toContain('var(--text-strong)')
  })

  it('语言选择器打开时允许选项弹层超出设置卡片', () => {
    const css = readFileSync(resolve('src/SettingsPage.css'), 'utf8')

    expect(css).toContain('.workspace-settings:has(.app-select[data-open=\'true\'])')
    expect(css).toContain('overflow: visible')
  })
})

describe('SettingsPage 语言设置卡片', () => {
  it('把当前语言作为事实标题展示，不再伪装成状态徽章或重复标题', () => {
    const markup = renderToStaticMarkup(createElement(SettingsPage, {
      backend: backend as never,
    }))
    const languageCard = markup.match(
      /<section class="workspace-settings language-settings"[\s\S]*?<\/section>/u,
    )?.[0] ?? ''

    expect(languageCard).toContain('<p class="eyebrow">界面语言</p>')
    expect(languageCard).toContain(
      '<h3 id="language-settings-heading">简体中文</h3>',
    )
    expect(languageCard).toMatch(
      /<span class="language-settings__field-label"[^>]*>选择应用语言<\/span>/u,
    )
    expect(languageCard).not.toContain('status-pill')
    expect(languageCard).not.toContain('>zh-CN<')
  })

  it('把持久化说明和选择器收进同一内容区，并建立可访问描述关系', () => {
    const markup = renderToStaticMarkup(createElement(SettingsPage, {
      backend: backend as never,
    }))

    expect(markup).toContain('<div class="language-settings__body"')
    expect(markup).toContain('id="app-language-description"')
    expect(markup).toContain('aria-describedby="app-language-description"')
    expect(markup).toContain('aria-live="polite"')
  })

  it('为桌面与窄窗提供独立的内容间距，不让选择器贴住卡片边缘', () => {
    const css = readFileSync(resolve('src/SettingsPage.css'), 'utf8')

    expect(css).toMatch(/\.language-settings__body\s*\{[^}]*display:\s*grid;/su)
    expect(css).toMatch(/\.language-settings__body\s*\{[^}]*gap:\s*10px;/su)
    expect(css).toMatch(
      /\.language-settings__body\s*\{[^}]*padding:\s*16px 18px 18px;/su,
    )
    expect(css).toMatch(
      /@media \(max-width: 680px\)[\s\S]*?\.language-settings__body\s*\{[^}]*padding:\s*14px 16px 16px;/su,
    )
  })

  it('语言已切换但设备持久化失败时显示仅本次会话生效', async () => {
    const storageDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: {
        getItem: () => null,
        setItem: () => {
        throw new Error('storage unavailable')
        },
      },
    })
    const mounted = mountSettingsPage()
    await flushPromptSettings()
    const trigger = mounted.container.querySelector<HTMLButtonElement>('#app-language')
    expect(trigger).not.toBeNull()

    act(() => trigger?.click())
    const englishOption = mounted.container.querySelector<HTMLButtonElement>(
      '#app-language-option-en-US',
    )
    expect(englishOption).not.toBeNull()
    await act(async () => {
      englishOption?.click()
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(mounted.container.textContent).toContain(
      'Language changed but could not be saved; it may revert after restart.',
    )
    if (storageDescriptor) {
      Object.defineProperty(globalThis, 'localStorage', storageDescriptor)
    } else {
      Reflect.deleteProperty(globalThis, 'localStorage')
    }
  })
})

describe('SettingsPage AI 提示词卡片', () => {
  it('读取并展示当前自然语言采集提示词、状态和真实受控调用链路', async () => {
    const mounted = mountSettingsPage()
    await flushPromptSettings()

    expect(promptApiMocks.listPromptTemplates).toHaveBeenCalledTimes(1)
    expect(promptApiMocks.listPromptVersions)
      .toHaveBeenCalledWith(promptTemplate.id)
    expect(mounted.container.textContent).toContain('AI 提示词')
    expect(mounted.container.textContent).toContain('当前启用 v3')
    expect(mounted.container.textContent).toContain(
      '提示词 → AI 结构化计划 → Schema / 能力校验 → 用户确认 → TikHub 真实 API',
    )
    expect(mounted.container.textContent).toContain(
      '提示词不保存 API Key，也不能绕过预算校验和用户确认',
    )

    const editor = mounted.container.querySelector<HTMLTextAreaElement>(
      '[data-prompt-content]',
    )
    expect(editor?.value).toBe(activePromptVersion.content)
  })

  it('把修改保存为新版本，并在用户明确操作后激活该版本', async () => {
    const draftVersion = {
      ...activePromptVersion,
      id: 'prompt-version-4',
      version: 4,
      content: '只输出严格 JSON，并保留可验证证据。',
      change_note: '补充证据约束',
      status: 'draft',
      activated_at: null,
    }
    promptApiMocks.createPromptVersion.mockResolvedValue(draftVersion)
    promptApiMocks.activatePromptVersion.mockResolvedValue({
      ...draftVersion,
      status: 'active',
      activated_at: '2026-07-17T09:00:00Z',
    })

    const mounted = mountSettingsPage()
    await flushPromptSettings()
    const editor = mounted.container.querySelector<HTMLTextAreaElement>(
      '[data-prompt-content]',
    )
    const note = mounted.container.querySelector<HTMLInputElement>(
      '[data-prompt-change-note]',
    )
    expect(editor).not.toBeNull()
    expect(note).not.toBeNull()

    if (editor) changeControlValue(editor, draftVersion.content)
    if (note) changeControlValue(note, draftVersion.change_note)
    const saveButton = Array.from(mounted.container.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('保存为新版本'))
    expect(saveButton).toBeDefined()
    await act(async () => saveButton?.click())

    expect(promptApiMocks.createPromptVersion).toHaveBeenCalledWith({
      template_id: promptTemplate.id,
      content: draftVersion.content,
      change_note: draftVersion.change_note,
    })
    expect(mounted.container.textContent).toContain('草稿 v4')

    const activateButton = Array.from(mounted.container.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('激活当前草稿'))
    expect(activateButton).toBeDefined()
    await act(async () => activateButton?.click())

    expect(promptApiMocks.activatePromptVersion)
      .toHaveBeenCalledWith(draftVersion.id)
    expect(mounted.container.textContent).toContain('当前启用 v4')
  })
})
