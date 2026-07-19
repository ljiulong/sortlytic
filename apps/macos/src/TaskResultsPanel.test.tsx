// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { listTaskResults } from './backend-api'
import { i18n as appI18n } from './i18n'
import TaskResultsPanel from './TaskResultsPanel'

vi.mock('./backend-api', () => ({
  backendErrorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
  listTaskResults: vi.fn(),
}))

const listTaskResultsMock = vi.mocked(listTaskResults)
const mountedRoots = new Set<{ container: HTMLDivElement; root: Root }>()

function mountPanel() {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  document.body.append(container)
  mountedRoots.add(mounted)
  act(() => root.render(createElement(TaskResultsPanel, {
    taskId: 'task-1',
    taskName: '真实采集任务',
  })))
  return mounted
}

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  await appI18n.changeLanguage('zh-CN')
  listTaskResultsMock.mockReset()
})

afterEach(() => {
  for (const mounted of mountedRoots) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedRoots.clear()
})

describe('TaskResultsPanel', () => {
  it('加载完成前不把等待状态伪装成空结果', () => {
    listTaskResultsMock.mockReturnValue(new Promise(() => undefined))
    const mounted = mountPanel()

    expect(mounted.container.textContent).toContain('正在读取已落库结果')
    expect(mounted.container.textContent).not.toContain('这次运行没有可展示的结果')
  })

  it('展示最新成功运行的真实账号数据', async () => {
    listTaskResultsMock.mockResolvedValue({
      task_id: 'task-1',
      task_run_id: 'run-1',
      run_status: 'success',
      total_count: 1,
      offset: 0,
      limit: 50,
      items: [{
        id: 'account-1',
        platform: 'tiktok',
        username: 'KiMeBeauty',
        account: 'kimebeauty',
        platform_user_id: 'user-1',
        profile_text: '公开账号简介',
        country_region: 'US',
        gender: 'female',
        age: 30,
        followers_count: 1234,
        posts_count: 56,
        data_source: 'TikHub API',
        collected_at: '2026-07-19T14:32:17Z',
      }],
    })
    const mounted = mountPanel()

    await act(async () => Promise.resolve())

    expect(listTaskResultsMock).toHaveBeenCalledWith('task-1', 50, 0)
    expect(mounted.container.textContent).toContain('KiMeBeauty')
    expect(mounted.container.textContent).toContain('@kimebeauty')
    expect(mounted.container.textContent).toContain('TikTok')
    expect(mounted.container.textContent).toContain('US')
    expect(mounted.container.textContent).toContain('1,234')
    expect(mounted.container.textContent).toContain('公开账号简介')
  })

  it('区分空结果和读取失败，并允许重试失败请求', async () => {
    listTaskResultsMock
      .mockRejectedValueOnce(new Error('数据库暂时不可读'))
      .mockResolvedValueOnce({
        task_id: 'task-1',
        task_run_id: 'run-1',
        run_status: 'success',
        total_count: 0,
        offset: 0,
        limit: 50,
        items: [],
      })
    const mounted = mountPanel()

    await act(async () => Promise.resolve())
    expect(mounted.container.querySelector('[role="alert"]')?.textContent)
      .toContain('数据库暂时不可读')

    await act(async () => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="重新读取"]',
    )?.click())
    expect(listTaskResultsMock).toHaveBeenCalledTimes(2)
    expect(mounted.container.textContent).toContain('这次运行没有可展示的结果')
  })
})
