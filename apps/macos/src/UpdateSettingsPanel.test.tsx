import { createElement, type ComponentProps } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import UpdateSettingsPanel from './UpdateSettingsPanel'

const baseProps: ComponentProps<typeof UpdateSettingsPanel> = {
  isTauriApp: true,
  hasCheckedForUpdate: false,
  isCheckingForUpdate: false,
  isInstallingUpdate: false,
  checkForUpdate: vi.fn(async () => null),
  installUpdate: vi.fn(async () => undefined),
}

function renderPanel(overrides: Partial<ComponentProps<typeof UpdateSettingsPanel>> = {}) {
  return renderToStaticMarkup(createElement(UpdateSettingsPanel, { ...baseProps, ...overrides }))
}

describe('UpdateSettingsPanel', () => {
  it('使用单层内容区和操作底栏，不再嵌套通用卡片', () => {
    const markup = renderPanel()

    expect(markup).toContain('update-settings__header')
    expect(markup).toContain('update-settings__body')
    expect(markup).toContain('update-settings__footer')
    expect(markup).not.toContain('glass-panel')
    expect(markup).not.toContain('update-summary')
  })

  it('发现新版本时显示版本说明和下载安装动作', () => {
    const markup = renderPanel({
      hasCheckedForUpdate: true,
      update: {
        version: '0.3.0',
        body: '新增真实任务导出能力',
        date: '2026-07-17',
      },
    })

    expect(markup).toContain('发现版本 0.3.0')
    expect(markup).toContain('新增真实任务导出能力')
    expect(markup).toContain('检查更新')
    expect(markup).toContain('下载并重启')
  })

  it('确认最新后不显示无效的下载动作', () => {
    const markup = renderPanel({ hasCheckedForUpdate: true })

    expect(markup).toContain('当前版本已经是最新版本')
    expect(markup).not.toContain('下载并重启')
  })
})
