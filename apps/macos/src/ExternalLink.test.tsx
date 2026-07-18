// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import ExternalLink from './ExternalLink'
import {
  isAllowedExternalUrl,
  openAllowedExternalUrl,
} from './external-link-policy'

const openUrlMock = vi.fn()

vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: openUrlMock }))

type MountedLink = {
  container: HTMLDivElement
  root: Root
}

const mountedLinks = new Set<MountedLink>()

function mountLink(onOpenError = vi.fn()) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  mountedLinks.add(mounted)
  document.body.append(container)
  act(() => root.render(
    <ExternalLink
      href="https://github.com/ljiulong/sortlytic"
      onOpenError={onOpenError}
    >
      GitHub
    </ExternalLink>,
  ))
  return { ...mounted, link: container.querySelector('a') as HTMLAnchorElement }
}

async function click(link: HTMLAnchorElement) {
  await act(async () => {
    link.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
    await Promise.resolve()
  })
}

beforeEach(() => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  openUrlMock.mockReset()
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

afterEach(() => {
  for (const mounted of mountedLinks) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedLinks.clear()
})

describe('ExternalLink', () => {
  it('保留可复制的真实 href 和标准浏览器回退属性', () => {
    const markup = renderToStaticMarkup(
      <ExternalLink
        href="https://github.com/ljiulong/sortlytic"
        onOpenError={vi.fn()}
      >
        GitHub
      </ExternalLink>,
    )

    expect(markup).toContain('href="https://github.com/ljiulong/sortlytic"')
    expect(markup).toContain('target="_blank"')
    expect(markup).toContain('rel="noreferrer"')
    expect(markup).toContain('data-external-link="true"')
  })

  it('只允许产品实际使用的固定 HTTPS 地址', async () => {
    expect(isAllowedExternalUrl('https://docs.tikhub.io/')).toBe(true)
    expect(isAllowedExternalUrl('https://example.com')).toBe(false)
    await expect(openAllowedExternalUrl('file:///tmp/secret')).rejects.toThrow('未授权')
    expect(openUrlMock).not.toHaveBeenCalled()
  })

  it('浏览器预览保留原生链接行为，不调用 Tauri opener', async () => {
    const { link } = mountLink()
    const event = new MouseEvent('click', { bubbles: true, cancelable: true })

    link.addEventListener('click', (clickEvent) => clickEvent.preventDefault(), { once: true })
    await act(async () => link.dispatchEvent(event))

    expect(openUrlMock).not.toHaveBeenCalled()
  })

  it('Tauri 环境阻止 WebView 导航并调用系统 opener', async () => {
    ;(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {}
    openUrlMock.mockResolvedValue(undefined)
    const { link } = mountLink()

    await click(link)

    expect(openUrlMock).toHaveBeenCalledWith('https://github.com/ljiulong/sortlytic')
  })

  it('原生打开失败时交给调用方显示错误', async () => {
    ;(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {}
    const error = new Error('open failed')
    const onOpenError = vi.fn()
    openUrlMock.mockRejectedValue(error)
    const { link } = mountLink(onOpenError)

    await click(link)

    expect(onOpenError).toHaveBeenCalledWith(error)
  })
})
