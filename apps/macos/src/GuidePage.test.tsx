// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import GuidePage from './GuidePage'
import { i18n } from './i18n'

const openUrlMock = vi.fn()

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: openUrlMock }))

type MountedGuide = {
  container: HTMLDivElement
  root: Root
}

const mountedGuides = new Set<MountedGuide>()

function mountGuide() {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  mountedGuides.add(mounted)
  document.body.append(container)
  act(() => root.render(createElement(GuidePage, { onOpenSettings: vi.fn() })))
  return mounted
}

function renderGuide() {
  return renderToStaticMarkup(createElement(GuidePage, { onOpenSettings: vi.fn() }))
}

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  openUrlMock.mockReset()
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
  await i18n.changeLanguage('zh-CN')
})

afterEach(() => {
  for (const mounted of mountedGuides) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedGuides.clear()
})

describe('GuidePage', () => {
  it('按六个连续章节展示从配置到导出的完整工作流', () => {
    const markup = renderGuide()
    const titles = [
      '准备本地工作区',
      '配置 TikHub 数据来源',
      '配置 AI 处理',
      '创建并校验任务',
      '确认运行与管理任务',
      '按任务导出与复核',
    ]

    expect(markup).toContain('guide-handbook')
    expect(markup).toContain('<ol')
    expect((markup.match(/class="guide-chapter"/g) ?? [])).toHaveLength(6)
    titles.reduce((previousIndex, title) => {
      const currentIndex = markup.indexOf(title)
      expect(currentIndex).toBeGreaterThan(previousIndex)
      return currentIndex
    }, -1)
    expect(markup).toContain('打开设置')
  })

  it('详细说明地区搜索、筛选、任务管理和逐任务导出边界', () => {
    const markup = renderGuide()

    expect(markup).toContain('249 个 ISO 两位代码')
    expect(markup).toContain('中文名、英文名或两位代码')
    expect(markup).toContain('明确公开年龄')
    expect(markup).toContain('明确公开性别')
    expect(markup).toContain('确认运行')
    expect(markup).toContain('取消任务')
    expect(markup).toContain('删除任务')
    expect(markup).toContain('Excel')
    expect(markup).toContain('PDF')
    expect(markup).toContain('提示词版本')
    expect(markup).toContain('Schema')
    expect(markup).toContain('来源证据')
  })

  it('不再借用任务卡、连接卡、导出卡或计划网格', () => {
    const markup = renderGuide()

    expect(markup).not.toContain('glass-panel')
    expect(markup).not.toContain('connection-card')
    expect(markup).not.toContain('task-row')
    expect(markup).not.toContain('export-item')
    expect(markup).not.toContain('plan-grid')
    expect(markup).not.toContain('已纳入')
  })

  it('保留五个官方资源和 Token 请求格式，不再使用独立黑底侧栏', () => {
    const markup = renderGuide()

    expect((markup.match(/class="guide-resource-link"/g) ?? [])).toHaveLength(5)
    expect((markup.match(/data-external-link="true"/g) ?? [])).toHaveLength(5)
    expect(markup).toContain('href="https://user.tikhub.io/register"')
    expect(markup).toContain('href="https://user.tikhub.io/login"')
    expect(markup).toContain('href="https://docs.tikhub.io/"')
    expect(markup).toContain('href="https://tikhub.io/getting-started"')
    expect(markup).toContain('href="https://tikhub.io/pricing"')
    expect(markup).toContain('Authorization')
    expect(markup).toContain('Bearer YOUR_API_KEY')
    expect(markup).toContain('价格与免费额度')
    expect(markup).not.toContain('guide-page__sidebar')
    expect(markup).not.toContain('guide-token-block')
  })

  it('Tauri 无法打开官方资源时显示可见错误反馈', async () => {
    ;(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {}
    openUrlMock.mockRejectedValue(new Error('open failed'))
    const { container } = mountGuide()
    const link = container.querySelector(
      'a[href="https://docs.tikhub.io/"]',
    ) as HTMLAnchorElement

    await act(async () => {
      link.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
      await Promise.resolve()
      await Promise.resolve()
    })

    await vi.waitFor(() => {
      const status = container.querySelector('[role="status"]')
      expect(status?.textContent).toContain('发生未知错误，请稍后重试。')
    })
  })

  it('如实说明工作区私有 JSON、文件权限和真实 AI 到 TikHub 的安全边界', () => {
    const markup = renderGuide()

    expect(markup).toContain('当前工作区私有 JSON')
    expect(markup).toContain('目录权限为 0700')
    expect(markup).toContain('文件权限为 0600')
    expect(markup).toContain('旧配置需要重新输入')
    expect(markup).toContain('不进入数据库、日志、导出或 Webhook')
    expect(markup).toContain('最小真实模型请求')
    expect(markup).toContain('当前启用的提示词正文')
    expect(markup).toContain('collection_plan_v3')
    expect(markup).toContain('用户确认运行后')
    expect(markup).toContain('真实 TikHub 请求')
    expect(markup).not.toContain('系统安全存储引用')
    expect(markup).not.toContain('不发起真实模型请求')
    expect(markup).not.toContain('本地规则引擎')
    expect(markup).not.toContain('只校验配置完整性')
    expect(markup).not.toContain('规则引擎不会调用')
  })
})
