// @vitest-environment happy-dom

import { act, createElement, type ComponentProps } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { i18n } from './i18n'
import UpdateSettingsPanel from './UpdateSettingsPanel'

const baseProps: ComponentProps<typeof UpdateSettingsPanel> = {
  isTauriApp: true,
  currentVersion: '0.3.0',
  phase: 'idle',
  preferences: { autoCheck: true, autoDownload: false },
  setAutoCheck: vi.fn(),
  setAutoDownload: vi.fn(),
  checkForUpdate: vi.fn(async () => null),
  prepareUpdate: vi.fn(async () => undefined),
  relaunchToUpdate: vi.fn(async () => undefined),
}

type MountedPanel = {
  container: HTMLDivElement
  root: Root
}

const mountedPanels = new Set<MountedPanel>()

function renderPanel(overrides: Partial<ComponentProps<typeof UpdateSettingsPanel>> = {}) {
  return renderToStaticMarkup(createElement(UpdateSettingsPanel, {
    ...baseProps,
    ...overrides,
  }))
}

function mountPanel(overrides: Partial<ComponentProps<typeof UpdateSettingsPanel>> = {}) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  mountedPanels.add(mounted)
  document.body.append(container)
  act(() => root.render(createElement(UpdateSettingsPanel, {
    ...baseProps,
    ...overrides,
  })))
  return mounted
}

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  await i18n.changeLanguage('zh-CN')
})

afterEach(() => {
  for (const mounted of mountedPanels) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedPanels.clear()
  vi.clearAllMocks()
})

describe('UpdateSettingsPanel', () => {
  it('紧凑展示真实 Logo、名称、当前版本和 GitHub，不展示作者或产品长简介', () => {
    const markup = renderPanel()

    expect(markup).toContain('关于 Sortlytic')
    expect(markup).toContain('当前版本')
    expect(markup).toContain('v0.3.0')
    expect(markup).toContain('icon.png')
    expect(markup).toContain('href="https://github.com/ljiulong/sortlytic"')
    expect(markup).toContain('data-external-link="true"')
    expect(markup).not.toContain('作者')
    expect(markup).not.toContain('只检查和下载官方发布')
  })

  it('浏览器预览显示开发预览，并禁用真实更新操作', () => {
    const markup = renderPanel({ currentVersion: null, isTauriApp: false })

    expect(markup).toContain('开发预览')
    expect(markup).toContain('开发预览不执行真实更新')
    expect(markup).toContain('data-update-action="check" disabled=""')
    expect(markup).toContain('data-update-action="update" disabled=""')
  })

  it('自动检查关闭时禁用自动下载，并保留手动检查按钮', () => {
    const { container } = mountPanel({
      preferences: { autoCheck: false, autoDownload: false },
    })
    const autoCheck = container.querySelector<HTMLInputElement>(
      '[data-update-preference="auto-check"]',
    )
    const autoDownload = container.querySelector<HTMLInputElement>(
      '[data-update-preference="auto-download"]',
    )
    const checkButton = container.querySelector<HTMLButtonElement>(
      '[data-update-action="check"]',
    )

    expect(autoCheck?.checked).toBe(false)
    expect(autoDownload?.disabled).toBe(true)
    expect(checkButton?.disabled).toBe(false)
  })

  it('尚未发现更新时禁用更新按钮', () => {
    const markup = renderPanel()

    expect(markup).toContain('尚未检查')
    expect(markup).toContain('data-update-action="update" disabled=""')
  })

  it('检查失败后提供重试检查，并保持无更新时的更新按钮禁用', () => {
    const markup = renderPanel({ error: '网络连接失败', phase: 'error' })

    expect(markup).toContain('网络连接失败')
    expect(markup).toContain('重试检查')
    expect(markup).toContain('data-update-action="update" disabled=""')
  })

  it('长更新说明只在自动打开的可访问弹窗正文中出现', async () => {
    const longNotes = `首段说明\n${'https://example.com/very-long-path/'.repeat(350)}`
    expect(renderPanel({
      phase: 'available',
      update: { version: '9.9.1', body: longNotes, date: '2026-07-18' },
    })).not.toContain(longNotes)

    const { container } = mountPanel({
      phase: 'available',
      update: { version: '9.9.1', body: longNotes, date: '2026-07-18' },
    })

    await vi.waitFor(() => {
      const dialog = container.querySelector('[role="dialog"]')
      expect(dialog?.getAttribute('aria-modal')).toBe('true')
      expect(dialog?.textContent).toContain('更新至 Sortlytic v9.9.1')
      expect(dialog?.textContent).toContain(longNotes)
      expect(dialog?.querySelector('.update-dialog__notes')).not.toBeNull()
    })
  })

  it('空更新说明显示明确空状态', async () => {
    const { container } = mountPanel({
      phase: 'available',
      update: { version: '9.9.2', body: '   ' },
    })

    await vi.waitFor(() => {
      expect(container.querySelector('[role="dialog"]')?.textContent)
        .toContain('本次更新没有提供说明。')
    })
  })

  it('发现新版本时弹窗主按钮会下载并安装，不会停留在仅查看状态', async () => {
    const prepareUpdate = vi.fn(async () => undefined)
    const { container } = mountPanel({
      phase: 'available',
      prepareUpdate,
      update: { version: '9.9.5', body: '准备更新测试' },
    })

    await vi.waitFor(() => expect(container.querySelector('[role="dialog"]')).not.toBeNull())
    const primaryAction = container.querySelector<HTMLButtonElement>(
      '[data-update-dialog-action="primary"]',
    )
    expect(primaryAction?.disabled).toBe(false)
    await act(async () => primaryAction?.click())

    expect(prepareUpdate).toHaveBeenCalledTimes(1)
  })

  it('弹窗支持关闭、再次打开、Esc 关闭和焦点恢复', async () => {
    const { container } = mountPanel({
      phase: 'available',
      update: { version: '9.9.3', body: '修复更新流程' },
    })

    await vi.waitFor(() => expect(container.querySelector('[role="dialog"]')).not.toBeNull())
    const closeButton = container.querySelector<HTMLButtonElement>(
      '[data-update-dialog-action="close"]',
    )
    act(() => closeButton?.click())
    expect(container.querySelector('[role="dialog"]')).toBeNull()

    const viewButton = container.querySelector<HTMLButtonElement>(
      '[data-update-action="update"]',
    )
    viewButton?.focus()
    act(() => viewButton?.click())
    await vi.waitFor(() => expect(container.querySelector('[role="dialog"]')).not.toBeNull())
    act(() => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })))

    expect(container.querySelector('[role="dialog"]')).toBeNull()
    expect(document.activeElement).toBe(viewButton)
  })

  it('正在准备更新时禁用关闭并忽略 Esc', async () => {
    const { container } = mountPanel({
      phase: 'preparing',
      update: { version: '9.9.6', body: '正在准备' },
    })

    await vi.waitFor(() => expect(container.querySelector('[role="dialog"]')).not.toBeNull())
    const closeButton = container.querySelector<HTMLButtonElement>(
      '[data-update-dialog-action="close"]',
    )
    expect(closeButton?.disabled).toBe(true)
    act(() => document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' })))
    expect(container.querySelector('[role="dialog"]')).not.toBeNull()
  })

  it('同一版本在同一会话中只自动打开一次', async () => {
    const update = { version: '9.9.7', body: '只自动展示一次' }
    const first = mountPanel({ phase: 'available', update })
    await vi.waitFor(() => expect(first.container.querySelector('[role="dialog"]')).not.toBeNull())
    act(() => first.root.unmount())
    first.container.remove()
    mountedPanels.delete(first)

    const second = mountPanel({ phase: 'available', update })
    await Promise.resolve()
    expect(second.container.querySelector('[role="dialog"]')).toBeNull()
  })

  it('准备完成后只提供由用户触发的重启更新操作', () => {
    const markup = renderPanel({
      phase: 'ready',
      update: { version: '9.9.4', body: '更新已准备' },
    })

    expect(markup).toContain('更新已准备好')
    expect(markup).toContain('重启并更新')
    expect(markup).not.toContain('下载并重启')
  })
})
