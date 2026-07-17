// @vitest-environment happy-dom

// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { readFileSync } from 'node:fs'
// @ts-expect-error Vitest 在 Node 中运行，应用构建有意不加载 Node 类型。
import { resolve } from 'node:path'
import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import AppSelect from './AppSelect'

const options = [
  { value: 'CN', label: '中国大陆', meta: 'CN' },
  { value: 'US', label: '美国', meta: 'US' },
  { value: 'JP', label: '日本', meta: 'JP' },
]

type MountedSelect = {
  container: HTMLDivElement
  root: Root
}

const mountedSelects = new Set<MountedSelect>()

function mountSelect({
  onChange = vi.fn(),
  searchable = false,
}: {
  onChange?: (value: string) => void
  searchable?: boolean
} = {}) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  document.body.append(container)
  mountedSelects.add(mounted)
  act(() => root.render(createElement(AppSelect, {
    id: 'region-code',
    onChange,
    options,
    placeholder: '请选择国家/地区',
    searchable,
    value: 'US',
  })))
  return mounted
}

function dispatchKey(target: Element, key: string) {
  const event = new KeyboardEvent('keydown', {
    bubbles: true,
    cancelable: true,
    key,
  })
  act(() => target.dispatchEvent(event))
  return event
}

function click(target: Element) {
  act(() => target.dispatchEvent(new MouseEvent('click', {
    bubbles: true,
    cancelable: true,
  })))
}

async function flushAnimationFrame() {
  await act(async () => {
    await new Promise<void>((resolveFrame) => {
      window.requestAnimationFrame(() => resolveFrame())
    })
  })
}

beforeEach(() => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
})

afterEach(() => {
  for (const mounted of mountedSelects) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedSelects.clear()
  document.body.replaceChildren()
})

describe('AppSelect', () => {
  it('关闭态使用应用内列表框触发器而不是原生下拉菜单', () => {
    const markup = renderToStaticMarkup(createElement(AppSelect, {
      id: 'region-code',
      onChange: vi.fn(),
      options,
      placeholder: '请选择国家/地区',
      searchable: true,
      value: 'US',
    }))

    expect(markup).toContain('id="region-code"')
    expect(markup).toContain('aria-haspopup="listbox"')
    expect(markup).toContain('aria-expanded="false"')
    expect(markup).toContain('美国')
    expect(markup).toContain('US')
    expect(markup).not.toContain('<select')
  })

  it('把活动选项关联到实际获焦触发器，且选项不增加 Tab 停靠', () => {
    const { container } = mountSelect()
    const trigger = container.querySelector<HTMLButtonElement>('#region-code')
    expect(trigger).not.toBeNull()
    trigger?.focus()
    click(trigger as HTMLButtonElement)

    const listbox = container.querySelector('[role="listbox"]')
    const renderedOptions = Array.from(
      container.querySelectorAll<HTMLElement>('[role="option"]'),
    )
    expect(document.activeElement).toBe(trigger)
    expect(trigger?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-US')
    expect(listbox?.hasAttribute('aria-activedescendant')).toBe(false)
    expect(renderedOptions).toHaveLength(3)
    expect(renderedOptions.every((option) => option.tabIndex === -1)).toBe(true)
  })

  it('支持方向键、Home、End 和 Enter 选择，并在关闭后恢复触发器焦点', async () => {
    const onChange = vi.fn()
    const { container } = mountSelect({ onChange })
    const trigger = container.querySelector<HTMLButtonElement>('#region-code')
    expect(trigger).not.toBeNull()
    trigger?.focus()

    expect(dispatchKey(trigger as HTMLButtonElement, 'ArrowDown').defaultPrevented)
      .toBe(true)
    expect(trigger?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-US')

    dispatchKey(trigger as HTMLButtonElement, 'Home')
    expect(trigger?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-CN')

    dispatchKey(trigger as HTMLButtonElement, 'End')
    expect(trigger?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-JP')

    dispatchKey(trigger as HTMLButtonElement, 'ArrowUp')
    expect(trigger?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-US')

    dispatchKey(trigger as HTMLButtonElement, 'Enter')
    await flushAnimationFrame()
    expect(onChange).toHaveBeenCalledWith('US')
    expect(trigger?.getAttribute('aria-expanded')).toBe('false')
    expect(document.activeElement).toBe(trigger)
  })

  it('支持 Space 选择当前项', async () => {
    const onChange = vi.fn()
    const { container } = mountSelect({ onChange })
    const trigger = container.querySelector<HTMLButtonElement>('#region-code')
    expect(trigger).not.toBeNull()
    trigger?.focus()

    dispatchKey(trigger as HTMLButtonElement, 'ArrowDown')
    dispatchKey(trigger as HTMLButtonElement, 'End')
    dispatchKey(trigger as HTMLButtonElement, ' ')
    await flushAnimationFrame()

    expect(onChange).toHaveBeenCalledWith('JP')
    expect(document.activeElement).toBe(trigger)
  })

  it('可搜索时由实际获焦输入框承载活动选项，Escape 关闭后恢复焦点', async () => {
    const { container } = mountSelect({ searchable: true })
    const trigger = container.querySelector<HTMLButtonElement>('#region-code')
    expect(trigger).not.toBeNull()
    trigger?.focus()
    click(trigger as HTMLButtonElement)
    await flushAnimationFrame()

    const search = container.querySelector<HTMLInputElement>(
      '.app-select__search input',
    )
    const listbox = container.querySelector('[role="listbox"]')
    expect(search).not.toBeNull()
    expect(document.activeElement).toBe(search)
    expect(search?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-US')
    expect(listbox?.hasAttribute('aria-activedescendant')).toBe(false)

    dispatchKey(search as HTMLInputElement, 'Home')
    expect(search?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-CN')
    dispatchKey(search as HTMLInputElement, 'End')
    expect(search?.getAttribute('aria-activedescendant'))
      .toBe('region-code-option-JP')

    expect(dispatchKey(search as HTMLInputElement, 'Escape').defaultPrevented)
      .toBe(true)
    await flushAnimationFrame()
    expect(trigger?.getAttribute('aria-expanded')).toBe('false')
    expect(document.activeElement).toBe(trigger)
  })

  it('保留明确的键盘焦点样式，并防止弹层和长文本在窄窗溢出', () => {
    const css = readFileSync(resolve('src/AppSelect.css'), 'utf8')

    expect(css).toContain('.app-select__trigger:focus-visible')
    expect(css).toContain('outline: 2px solid var(--focus);')
    expect(css).toContain('box-sizing: border-box;')
    expect(css).toContain('max-width: 100%;')
    expect(css).toContain('overflow-wrap: anywhere;')
    expect(css).toContain('@media (max-width: 560px)')
  })
})
