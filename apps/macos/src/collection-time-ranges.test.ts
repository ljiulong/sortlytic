// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  extractProviderTimeRanges,
  loadPlatformTimeRanges,
  useCollectionTimeRanges,
} from './collection-time-ranges'

const mountedRoots = new Set<{ container: HTMLDivElement; root: Root }>()

type CapabilityLoader = (platform: string) => Promise<Array<{
  data_type: string
  provider_time_ranges: string[]
}>>

function Probe({
  loader,
  platform,
}: {
  loader?: CapabilityLoader
  platform?: 'TikTok' | '抖音' | '小红书'
}) {
  const state = useCollectionTimeRanges(platform, loader)
  return createElement('output', {
    'data-error': state.error ?? '',
    'data-loading': String(state.isLoading),
    'data-values': state.values.join(','),
  })
}

async function mountProbe(
  platform?: 'TikTok' | '抖音' | '小红书',
  loader?: CapabilityLoader,
) {
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

describe('collection time range capabilities', () => {
  it('只读取关键词搜索端点范围，并按天数去重排序', () => {
    expect(extractProviderTimeRanges([
      { data_type: 'comments', provider_time_ranges: [] },
      { data_type: 'keyword_search', provider_time_ranges: ['180', '7', '30', '7', '1'] },
    ])).toEqual(['1', '7', '30', '180'])
  })

  it('按正式后端代码读取三个平台，不在前端伪造响应', async () => {
    const loader = vi.fn(async () => ([
      { data_type: 'keyword_search', provider_time_ranges: ['1', '7', '180'] },
    ]))

    await expect(loadPlatformTimeRanges('小红书', loader)).resolves.toEqual(['1', '7', '180'])
    expect(loader).toHaveBeenCalledWith('xiaohongshu')
  })

  it('平台切换时丢弃旧请求结果，避免保留上个平台非法范围', async () => {
    let resolveTikTok: ((value: Array<{
      data_type: string
      provider_time_ranges: string[]
    }>) => void) | undefined
    const loader = vi.fn((platform: string) => platform === 'tiktok'
      ? new Promise<Array<{ data_type: string; provider_time_ranges: string[] }>>((resolve) => {
          resolveTikTok = resolve
        })
      : Promise.resolve([
          { data_type: 'keyword_search', provider_time_ranges: ['1', '7', '180'] },
        ]))
    const mounted = await mountProbe('TikTok', loader)

    await act(async () => {
      mounted.root.render(createElement(Probe, { loader, platform: '小红书' }))
      await Promise.resolve()
    })
    await act(async () => {
      resolveTikTok?.([
        { data_type: 'keyword_search', provider_time_ranges: ['1', '7', '30', '180'] },
      ])
      await Promise.resolve()
    })

    const output = mounted.container.querySelector('output')
    expect(output?.dataset.values).toBe('1,7,180')
  })
})
