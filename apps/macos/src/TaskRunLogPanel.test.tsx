// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import TaskRunLogPanel from './TaskRunLogPanel'
import type { TaskLogView } from './backend-api'
import { i18n } from './i18n'

type MountedPanel = {
  container: HTMLDivElement
  root: Root
}

const mountedRoots: MountedPanel[] = []

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  await i18n.changeLanguage('zh-CN')
})

function mountPanel(loader: (runId: string) => Promise<TaskLogView[]>) {
  const container = document.createElement('div')
  document.body.append(container)
  const root = createRoot(container)

  act(() => root.render(createElement(TaskRunLogPanel, {
    loadLogs: loader,
    runId: 'run-2',
  })))

  const mounted = { container, root }
  mountedRoots.push(mounted)
  return mounted
}

afterEach(() => {
  for (const mounted of mountedRoots.splice(0)) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  vi.useRealTimers()
})

describe('TaskRunLogPanel', () => {
  it('只在展开后按运行 ID 加载并展示后端安全日志', async () => {
    const loader = vi.fn().mockResolvedValue([{
      id: 'log-1',
      task_run_id: 'run-2',
      stage: 'fetching',
      level: 'error',
      message: 'TikHub 请求超时',
      safe_details_json: { endpoint: '/api/v1/search', attempt: 2 },
      created_at: '2026-07-17T08:00:20Z',
    } satisfies TaskLogView])
    const mounted = mountPanel(loader)
    const toggle = mounted.container.querySelector<HTMLButtonElement>('button')

    expect(loader).not.toHaveBeenCalled()
    expect(toggle?.getAttribute('aria-expanded')).toBe('false')

    await act(async () => toggle?.click())

    expect(loader).toHaveBeenCalledWith('run-2')
    expect(toggle?.getAttribute('aria-expanded')).toBe('true')
    expect(mounted.container.textContent).toContain('运行日志')
    expect(mounted.container.textContent).toContain('fetching')
    expect(mounted.container.textContent).toContain('TikHub 请求超时')
    expect(mounted.container.textContent).toContain('/api/v1/search')
    expect(mounted.container.querySelector('time')?.getAttribute('datetime'))
      .toBe('2026-07-17T08:00:20Z')
  })

  it('加载失败时展示错误并允许原位重试', async () => {
    const loader = vi.fn()
      .mockRejectedValueOnce(new Error('database busy'))
      .mockResolvedValueOnce([])
    const mounted = mountPanel(loader)
    const toggle = mounted.container.querySelector<HTMLButtonElement>('button')

    await act(async () => toggle?.click())

    expect(mounted.container.querySelector('.task-run-logs')?.getAttribute('aria-busy')).toBe('false')
    expect(mounted.container.textContent).toContain('无法读取运行日志')

    const retry = Array.from(mounted.container.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('重试'))
    await act(async () => retry?.click())

    expect(loader).toHaveBeenCalledTimes(2)
    expect(mounted.container.textContent).toContain('这次运行还没有日志')
  })

  it('折叠后再次展开时重新加载当前运行的日志', async () => {
    const loader = vi.fn()
      .mockResolvedValueOnce([{
        id: 'log-1',
        task_run_id: 'run-2',
        stage: 'fetching',
        level: 'info',
        message: '开始采集',
        safe_details_json: null,
        created_at: '2026-07-17T08:00:20Z',
      } satisfies TaskLogView])
      .mockResolvedValueOnce([{
        id: 'log-2',
        task_run_id: 'run-2',
        stage: '已完成',
        level: 'info',
        message: '采集完成',
        safe_details_json: null,
        created_at: '2026-07-17T08:01:20Z',
      } satisfies TaskLogView])
    const mounted = mountPanel(loader)
    const toggle = mounted.container.querySelector<HTMLButtonElement>('button')

    await act(async () => toggle?.click())

    expect(mounted.container.textContent).toContain('开始采集')

    await act(async () => toggle?.click())
    await act(async () => toggle?.click())

    expect(loader).toHaveBeenCalledTimes(2)
    expect(loader).toHaveBeenLastCalledWith('run-2')
    expect(mounted.container.textContent).toContain('采集完成')
    expect(mounted.container.textContent).not.toContain('开始采集')
  })

  it('展开运行日志时定期刷新并在读取到终态日志后停止', async () => {
    vi.useFakeTimers()
    const loader = vi.fn()
      .mockResolvedValueOnce([{
        id: 'log-1',
        task_run_id: 'run-2',
        stage: '执行采集',
        level: 'info',
        message: '正在采集',
        safe_details_json: null,
        created_at: '2026-07-17T08:00:20Z',
      } satisfies TaskLogView])
      .mockResolvedValueOnce([{
        id: 'log-2',
        task_run_id: 'run-2',
        stage: '已完成',
        level: 'info',
        message: '采集完成',
        safe_details_json: null,
        created_at: '2026-07-17T08:01:20Z',
      } satisfies TaskLogView])
    const mounted = mountPanel(loader)
    const toggle = mounted.container.querySelector<HTMLButtonElement>('button')

    await act(async () => toggle?.click())
    expect(loader).toHaveBeenCalledTimes(1)

    await act(async () => vi.advanceTimersByTimeAsync(3_000))
    expect(loader).toHaveBeenCalledTimes(2)
    expect(mounted.container.textContent).toContain('采集完成')

    await act(async () => vi.advanceTimersByTimeAsync(9_000))
    expect(loader).toHaveBeenCalledTimes(2)
  })

  it('忽略折叠前发起但在重新展开后才返回的过期请求', async () => {
    let resolveFirst!: (logs: TaskLogView[]) => void
    let resolveSecond!: (logs: TaskLogView[]) => void
    const firstRequest = new Promise<TaskLogView[]>((resolve) => {
      resolveFirst = resolve
    })
    const secondRequest = new Promise<TaskLogView[]>((resolve) => {
      resolveSecond = resolve
    })
    const loader = vi.fn()
      .mockReturnValueOnce(firstRequest)
      .mockReturnValueOnce(secondRequest)
    const mounted = mountPanel(loader)
    const toggle = mounted.container.querySelector<HTMLButtonElement>('button')

    act(() => toggle?.click())
    act(() => toggle?.click())
    act(() => toggle?.click())

    await act(async () => resolveSecond([{
      id: 'log-current',
      task_run_id: 'run-2',
      stage: '已完成',
      level: 'info',
      message: '最新日志',
      safe_details_json: null,
      created_at: '2026-07-17T08:01:20Z',
    }]))
    await act(async () => resolveFirst([{
      id: 'log-stale',
      task_run_id: 'run-2',
      stage: '执行采集',
      level: 'info',
      message: '过期日志',
      safe_details_json: null,
      created_at: '2026-07-17T08:00:20Z',
    }]))

    expect(loader).toHaveBeenCalledTimes(2)
    expect(mounted.container.textContent).toContain('最新日志')
    expect(mounted.container.textContent).not.toContain('过期日志')
  })
})
