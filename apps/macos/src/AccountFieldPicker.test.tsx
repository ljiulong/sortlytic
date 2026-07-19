// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AccountCollectionCapabilityView } from './backend-api'
import AccountFieldPicker from './AccountFieldPicker'
import type { AccountSourceKey } from './collection-options'
import { i18n } from './i18n'

const mountedRoots = new Set<{ container: HTMLDivElement; root: Root }>()

const capability: AccountCollectionCapabilityView = {
  catalog_version: 1,
  platform: 'tiktok',
  display_name: 'TikTok',
  account_sources: [],
  field_groups: [
    { key: 'profile', display_name: '账号资料' },
    { key: 'demographics', display_name: '人口属性' },
  ],
  fields: [
    {
      key: 'avatar_url',
      group: 'profile',
      display_name: '头像',
      description: '账号公开头像地址。',
      value_type: 'text',
      availability: 'enrichment',
      default_selected: true,
      required_operation_keys: ['enrich.profile'],
      covered_by_source_keys: ['user_search', 'direct_account'],
    },
    {
      key: 'country_region',
      group: 'profile',
      display_name: '国家或地区',
      description: '平台接口明确返回的账号国家或地区。',
      value_type: 'text',
      availability: 'enrichment',
      default_selected: true,
      required_operation_keys: ['enrich.account_country'],
    },
    {
      key: 'gender',
      group: 'demographics',
      display_name: '性别',
      description: '只使用平台接口明确返回的性别。',
      value_type: 'text',
      availability: 'unsupported',
      default_selected: false,
      required_operation_keys: [],
      missing_reason: 'TikTok 当前资料接口未明确提供。',
      supported_platforms: ['douyin'],
    },
  ],
}

async function mountPicker(
  selectedFields = ['avatar_url', 'country_region'],
  accountSource: AccountSourceKey | null = 'user_search',
) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const onChange = vi.fn()
  mountedRoots.add({ container, root })
  document.body.append(container)
  await act(async () => {
    root.render(
      <AccountFieldPicker
        accountSource={accountSource ?? undefined}
        capability={capability}
        onChange={onChange}
        selectedFields={selectedFields}
      />,
    )
  })
  return { container, onChange }
}

function buttonByText(container: HTMLElement, text: string) {
  return [...container.querySelectorAll('button')]
    .find((button) => button.textContent?.includes(text))
}

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  await i18n.changeLanguage('zh-CN')
})

afterEach(() => {
  for (const mounted of mountedRoots) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedRoots.clear()
  vi.restoreAllMocks()
})

describe('AccountFieldPicker', () => {
  it('默认保持紧凑摘要，并通过 aria-expanded 内联展开', async () => {
    const { container } = await mountPicker()
    const configure = buttonByText(container, '配置字段')

    expect(container.textContent).toContain('6 个基础字段 + 2 个扩展字段')
    expect(container.textContent).toContain('其中 1 项需要补全请求')
    expect(configure?.getAttribute('aria-expanded')).toBe('false')

    await act(async () => configure?.click())
    expect(buttonByText(container, '收起字段')?.getAttribute('aria-expanded')).toBe('true')
    expect(container.textContent).toContain('需补全，会增加请求')
  })

  it('未选择来源时等待来源，选择后只计算未被来源覆盖的字段', async () => {
    const pending = await mountPicker(undefined, null)
    expect(pending.container.textContent).toContain('选择账号来源后计算补全请求')

    const sourced = await mountPicker()
    expect(sourced.container.textContent).toContain('其中 1 项需要补全请求')
    await act(async () => buttonByText(sourced.container, '配置字段')?.click())
    const avatar = [...sourced.container.querySelectorAll('label')]
      .find((label) => label.textContent?.includes('avatar_url'))
    expect(avatar?.textContent).toContain('直接提供')
  })

  it('分类折叠状态与 aria-expanded 和字段可见性保持一致', async () => {
    const { container } = await mountPicker()
    await act(async () => buttonByText(container, '配置字段')?.click())
    const panel = container.querySelector<HTMLElement>('.account-field-picker__panel[data-active="true"]')
    const toggle = panel?.querySelector<HTMLButtonElement>('.account-field-picker__group-header > button:first-child')
    const rows = panel?.querySelector<HTMLElement>('.account-field-picker__rows')

    expect(toggle?.getAttribute('aria-expanded')).toBe('true')
    expect(rows?.hasAttribute('hidden')).toBe(false)
    await act(async () => toggle?.click())
    expect(toggle?.getAttribute('aria-expanded')).toBe('false')
    expect(rows?.hasAttribute('hidden')).toBe(true)
  })

  it('全部可用不会选择不支持字段，恢复操作使用核心预设', async () => {
    const { container, onChange } = await mountPicker([])
    await act(async () => buttonByText(container, '配置字段')?.click())
    await act(async () => buttonByText(container, '选择全部可用字段')?.click())
    expect(onChange).toHaveBeenLastCalledWith(['avatar_url', 'country_region'])

    await act(async () => buttonByText(container, '恢复核心字段')?.click())
    expect(onChange).toHaveBeenLastCalledWith(['avatar_url', 'country_region'])
    const gender = [...container.querySelectorAll('label')]
      .find((label) => label.textContent?.includes('gender'))
      ?.querySelector('input')
    expect(gender?.disabled).toBe(true)
    expect(container.textContent).toContain('TikTok 当前资料接口未明确提供。')
    expect(container.textContent).toContain('支持平台：抖音')
  })

  it('搜索中文名称、字段代码和说明时保留分类结构', async () => {
    const { container } = await mountPicker()
    await act(async () => buttonByText(container, '配置字段')?.click())
    const search = container.querySelector<HTMLInputElement>('input[type="search"]')
    await act(async () => {
      if (!search) return
      const nativeSetter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        'value',
      )?.set
      nativeSetter?.call(search, 'country_region')
      search.dispatchEvent(new Event('input', { bubbles: true }))
    })

    expect(container.textContent).toContain('账号资料')
    expect(container.textContent).toContain('country_region')
    expect(container.textContent).not.toContain('avatar_url')
  })

  it('搜索命中其他分类时激活首个命中分类', async () => {
    const { container } = await mountPicker()
    await act(async () => buttonByText(container, '配置字段')?.click())
    const search = container.querySelector<HTMLInputElement>('input[type="search"]')
    await act(async () => {
      const nativeSetter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        'value',
      )?.set
      nativeSetter?.call(search, 'gender')
      search?.dispatchEvent(new Event('input', { bubbles: true }))
    })

    const demographics = [...container.querySelectorAll<HTMLElement>('.account-field-picker__panel')]
      .find((panel) => panel.textContent?.includes('gender'))
    expect(demographics?.dataset.active).toBe('true')
  })
})
