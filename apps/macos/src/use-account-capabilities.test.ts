// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AccountCollectionCapabilityView } from './backend-api'
import {
  loadAccountCapabilities,
  useAccountCapabilities,
  type AccountCapabilityLoader,
} from './use-account-capabilities'

const mountedRoots = new Set<{ container: HTMLDivElement; root: Root }>()

function capability(platform: string, empty = false): AccountCollectionCapabilityView {
  return {
    catalog_version: 1,
    platform,
    display_name: platform,
    account_sources: empty ? [] : [{
      key: 'user_search',
      display_name: '搜索用户',
      description: '按关键词搜索公开用户账号。',
      input_kind: 'keyword',
      endpoint_key: `${platform}.user_search`,
      pagination_mode: 'cursor',
      max_page_size: 20,
      max_request_count: 100,
    }],
    field_groups: [{ key: 'profile', display_name: '账号资料' }],
    fields: empty ? [] : [{
      key: 'avatar_url',
      group: 'profile',
      display_name: '头像',
      description: '账号公开头像地址。',
      value_type: 'text',
      availability: 'direct',
      default_selected: true,
      required_operation_keys: [],
      missing_reason: null,
    }],
  }
}

function Probe({ loader, platform }: {
  loader: AccountCapabilityLoader
  platform?: 'TikTok' | '抖音' | '小红书'
}) {
  const state = useAccountCapabilities(platform, loader)
  return createElement('output', {
    'data-empty': String(state.isEmpty),
    'data-error': state.error ?? '',
    'data-loading': String(state.isLoading),
    'data-platform': state.capability?.platform ?? '',
  })
}

async function mountProbe(platform: 'TikTok' | '抖音' | '小红书' | undefined, loader: AccountCapabilityLoader) {
  const container = document.createElement('div')
  const root = createRoot(container)
  mountedRoots.add({ container, root })
  document.body.append(container)
  await act(async () => root.render(createElement(Probe, { loader, platform })))
  return { container, root }
}

beforeEach(() => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
})

afterEach(() => {
  for (const mounted of mountedRoots) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedRoots.clear()
  vi.restoreAllMocks()
})

describe('account collection capabilities', () => {
  it('使用正式平台代码请求后端目录', async () => {
    const loader = vi.fn(async (platform: string) => capability(platform))

    await expect(loadAccountCapabilities('抖音', loader)).resolves.toMatchObject({
      platform: 'douyin',
    })
    expect(loader).toHaveBeenCalledWith('douyin')
  })

  it('平台切换后丢弃旧请求结果', async () => {
    let resolveTikTok: ((value: AccountCollectionCapabilityView) => void) | undefined
    const loader = vi.fn((platform: string) => platform === 'tiktok'
      ? new Promise<AccountCollectionCapabilityView>((resolve) => {
          resolveTikTok = resolve
        })
      : Promise.resolve(capability(platform)))
    const mounted = await mountProbe('TikTok', loader)

    await act(async () => {
      mounted.root.render(createElement(Probe, { loader, platform: '小红书' }))
      await Promise.resolve()
    })
    await act(async () => {
      resolveTikTok?.(capability('tiktok'))
      await Promise.resolve()
    })

    expect(mounted.container.querySelector('output')?.dataset.platform).toBe('xiaohongshu')
  })

  it('区分加载失败和成功但目录为空', async () => {
    const empty = await mountProbe('TikTok', async () => capability('tiktok', true))
    expect(empty.container.querySelector('output')?.dataset.empty).toBe('true')
    expect(empty.container.querySelector('output')?.dataset.error).toBe('')

    const failed = await mountProbe('抖音', async () => Promise.reject(new Error('offline')))
    expect(failed.container.querySelector('output')?.dataset.empty).toBe('false')
    expect(failed.container.querySelector('output')?.dataset.error).toBe(
      'ACCOUNT_CAPABILITY_UNAVAILABLE',
    )
  })
})
