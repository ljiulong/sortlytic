// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { readFileSync } from 'node:fs'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ApiProfileRegistryView } from './api-profiles'
import SettingsPage from './SettingsPage'

const { settingsApiDialogReducer } = SettingsPage.testUtils

const useApiProfilesMock = vi.hoisted(() => vi.fn())

vi.mock('./use-api-profiles', () => ({
  useApiProfiles: useApiProfilesMock,
}))

vi.mock('./ApiProfilesDialog', () => ({
  default: ({ isOpen, kind }: { isOpen: boolean; kind: string }) => (
    isOpen ? <aside data-api-dialog-kind={kind} role="dialog">API 配置列表</aside> : null
  ),
}))

vi.mock('./UpdateSettingsPanel', () => ({
  default: () => <section>客户端版本</section>,
}))

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

beforeEach(() => {
  useApiProfilesMock.mockReset()
  useApiProfilesMock.mockReturnValue({
    registry,
    registryQuery: {
      error: null,
      isLoading: false,
    },
  })
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
    expect(markup).toContain('客户端版本')
    expect(markup).not.toContain('旧 TikHub 内联面板')
    expect(markup).not.toContain('旧 AI 内联面板')
    expect(markup).not.toContain('tikh••••5812')
    expect(markup).not.toContain('sk-p••••C7x9')
    expect(markup).not.toContain(secret)
  })

  it('两个按钮映射到各自弹窗，关闭后回到无弹窗状态', () => {
    expect(settingsApiDialogReducer(null, { type: 'open', kind: 'tikhub' }))
      .toBe('tikhub')
    expect(settingsApiDialogReducer('tikhub', { type: 'open', kind: 'ai' }))
      .toBe('ai')
    expect(settingsApiDialogReducer('ai', { type: 'close' })).toBeNull()
  })

  it('按钮在窄窗改为单列，并保留明确焦点样式与双主题令牌', () => {
    const css = readFileSync(new URL('./SettingsPage.css', import.meta.url), 'utf8')

    expect(css).toContain('.settings-page__api-actions')
    expect(css).toContain('.settings-page__api-button:focus-visible')
    expect(css).toContain('@media (max-width: 680px)')
    expect(css).toContain('grid-template-columns: 1fr;')
    expect(css).toContain('var(--surface-raised)')
    expect(css).toContain('var(--text-strong)')
  })
})
