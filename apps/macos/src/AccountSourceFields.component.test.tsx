// @vitest-environment happy-dom

import { act } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import AccountSourceFields from './AccountSourceFields'
import type { AccountCollectionCapabilityView } from './backend-api'
import type { AccountSourceKey } from './collection-options'
import { i18n } from './i18n'
import type { Platform } from './workbench-data'

const mountedRoots = new Set<{ container: HTMLDivElement; root: Root }>()

const capability: AccountCollectionCapabilityView = {
  catalog_version: 1,
  platform: 'tiktok',
  display_name: 'TikTok',
  account_sources: [
    {
      key: 'user_search',
      display_name: '搜索用户',
      description: '按关键词搜索公开账号。',
      input_kind: 'keyword',
      endpoint_key: 'tiktok.user_search',
      pagination_mode: 'cursor',
      max_page_size: 20,
      max_request_count: 100,
    },
  ],
  field_groups: [{ key: 'profile', display_name: '账号资料' }],
  fields: [
    {
      key: 'avatar_url',
      group: 'profile',
      display_name: '头像',
      description: '账号公开头像地址。',
      value_type: 'text',
      availability: 'direct',
      default_selected: true,
      required_operation_keys: [],
    },
  ],
}

const sourceInputRegistration = {
  name: 'keyword' as const,
  onBlur: vi.fn(),
  onChange: vi.fn(),
  ref: vi.fn(),
}

const capabilityLoader = () => Promise.resolve(capability)

function renderFields(
  root: Root,
  callbacks: {
    onAccountSourceChange: (source?: AccountSourceKey) => void
    onPlatformChange: (platform?: Platform) => void
    onSelectedFieldsChange: (fields: string[]) => void
  },
) {
  root.render(
    <AccountSourceFields
      accountSource="user_search"
      capabilityLoader={capabilityLoader}
      onAccountSourceChange={callbacks.onAccountSourceChange}
      onPlatformChange={callbacks.onPlatformChange}
      onSelectedFieldsChange={callbacks.onSelectedFieldsChange}
      platform="TikTok"
      selectedFields={['avatar_url']}
      sourceInputRegistration={sourceInputRegistration}
    />,
  )
}

async function mountFields() {
  const container = document.createElement('div')
  const root = createRoot(container)
  mountedRoots.add({ container, root })
  document.body.append(container)
  const callbacks = {
    onAccountSourceChange: vi.fn(),
    onPlatformChange: vi.fn(),
    onSelectedFieldsChange: vi.fn(),
  }
  await act(async () => renderFields(root, callbacks))
  return { callbacks, container, root }
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

describe('AccountSourceFields', () => {
  it('用独立标签关联应用内选择器，不把按钮嵌套进 label', async () => {
    const { container } = await mountFields()

    expect(container.querySelector('label[for="platform"]')?.textContent).toBe('平台')
    expect(container.querySelector('label[for="account-source"]')?.textContent).toBe('账号来源')
    expect(container.querySelector('label button')).toBeNull()
  })

  it('父组件回调引用变化时不会重复协调同一份平台能力', async () => {
    const { callbacks, root } = await mountFields()
    expect(callbacks.onSelectedFieldsChange).toHaveBeenCalledTimes(1)

    const nextCallbacks = {
      onAccountSourceChange: vi.fn(),
      onPlatformChange: vi.fn(),
      onSelectedFieldsChange: vi.fn(),
    }
    await act(async () => renderFields(root, nextCallbacks))

    expect(nextCallbacks.onSelectedFieldsChange).not.toHaveBeenCalled()
  })

  it('英文界面按稳定 key 翻译来源和动态输入文案', async () => {
    await i18n.changeLanguage('en-US')
    const { container } = await mountFields()

    expect(container.querySelector('label[for="platform"]')?.textContent).toBe('Platform')
    expect(container.querySelector('label[for="account-source"]')?.textContent).toBe('Account source')
    expect(container.querySelector('label[for="source-input"]')?.textContent).toBe('Keyword')
    expect(container.querySelector('#account-source')?.textContent).toContain('Search users')
  })
})
