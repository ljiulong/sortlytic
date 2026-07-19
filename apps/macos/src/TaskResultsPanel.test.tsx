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

  it('英文界面本地化平台和性别契约值', async () => {
    await appI18n.changeLanguage('en-US')
    listTaskResultsMock.mockResolvedValue({
      task_id: 'task-1',
      task_run_id: 'run-1',
      run_status: 'success',
      age_filter_configured: false,
      gender_filter_configured: false,
      selected_fields: ['gender'],
      total_count: 1,
      offset: 0,
      limit: 50,
      items: [{
        id: 'account-en',
        platform: 'douyin',
        username: 'Account',
        account: 'account',
        platform_user_id: 'user-en',
        profile_text: null,
        country_region: null,
        gender: 'female',
        age: null,
        followers_count: null,
        posts_count: null,
        data_source: 'douyin.user_search',
        collected_at: '2026-07-20T00:00:00Z',
        account_fields_json: { gender: 'female' },
        field_evidence_json: {},
      }],
    })

    const mounted = mountPanel()
    await act(async () => Promise.resolve())
    expect(mounted.container.textContent).toContain('Douyin')
    expect(mounted.container.textContent).toContain('Gender: Female')
    expect(mounted.container.textContent).not.toContain('抖音')
    expect(mounted.container.textContent).not.toContain('Gender: female')
  })

  it('展示最新成功运行的真实账号数据', async () => {
    listTaskResultsMock.mockResolvedValue({
      task_id: 'task-1',
      task_run_id: 'run-1',
      run_status: 'success',
      age_filter_configured: true,
      gender_filter_configured: true,
      selected_fields: ['bio', 'followers_count', 'posts_count'],
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
        account_fields_json: {
          bio: '公开账号简介',
          followers_count: 1234,
          posts_count: 56,
        },
        field_evidence_json: {
          bio: {
            endpoint_key: 'tiktok.account_profile',
            raw_path: 'user.signature',
            collected_at: '2026-07-19T14:32:17Z',
          },
        },
      }],
    })
    const mounted = mountPanel()

    await act(async () => Promise.resolve())

    expect(listTaskResultsMock).toHaveBeenCalledWith('task-1', 50, 0)
    expect(mounted.container.textContent).toContain('KiMeBeauty')
    expect(mounted.container.textContent).toContain('@kimebeauty')
    expect(mounted.container.textContent).toContain('TikTok')
    expect(mounted.container.textContent).toContain('TikHub API')
    expect(mounted.container.textContent).toContain('US')
    expect(mounted.container.textContent).toContain('1,234')
    expect(mounted.container.textContent).toContain('公开账号简介')

    const detailButton = [...mounted.container.querySelectorAll('button')]
      .find((button) => button.textContent?.includes('查看已选字段'))
    expect(detailButton?.getAttribute('aria-expanded')).toBe('false')
    await act(async () => detailButton?.click())
    const details = mounted.container.querySelector('.task-results__details')
    expect(detailButton?.getAttribute('aria-expanded')).toBe('true')
    expect(details?.textContent).toContain('个人简介')
    expect(details?.textContent).toContain('bio')
    expect(details?.textContent).toContain('tiktok.account_profile')
    expect(details?.textContent).toContain('user.signature')
  })

  it('区分空结果和读取失败，并允许重试失败请求', async () => {
    listTaskResultsMock
      .mockRejectedValueOnce(new Error('数据库暂时不可读'))
      .mockResolvedValueOnce({
        task_id: 'task-1',
        task_run_id: 'run-1',
        run_status: 'success',
        age_filter_configured: false,
        gender_filter_configured: false,
        selected_fields: [],
        total_count: 0,
        offset: 0,
        limit: 50,
        items: [],
      })
    const mounted = mountPanel()

    await act(async () => Promise.resolve())
    expect(mounted.container.querySelector('[role="alert"]')?.textContent)
      .toContain('数据库暂时不可读')
    expect(mounted.container.textContent).not.toContain('正在读取已落库结果')

    await act(async () => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="重新读取"]',
    )?.click())
    expect(listTaskResultsMock).toHaveBeenCalledTimes(2)
    expect(mounted.container.textContent).toContain('这次运行没有可展示的结果')
  })

  it('区分任务未设置、未采集到和明确数值零', async () => {
    listTaskResultsMock.mockResolvedValue({
      task_id: 'task-1',
      task_run_id: 'run-1',
      run_status: 'success',
      age_filter_configured: false,
      gender_filter_configured: false,
      selected_fields: ['followers_count', 'posts_count', 'bio'],
      total_count: 1,
      offset: 0,
      limit: 50,
      items: [{
        id: 'account-missing',
        platform: 'tiktok',
        username: '字段口径测试',
        gender: null,
        age: null,
        followers_count: null,
        posts_count: 0,
        profile_text: null,
        data_source: 'TikHub API',
        collected_at: '2026-07-19T14:32:17Z',
        account_fields_json: { posts_count: 0 },
        field_evidence_json: {},
      }],
    })
    const mounted = mountPanel()

    await act(async () => Promise.resolve())

    expect(mounted.container.textContent).toContain('性别：任务未设置')
    expect(mounted.container.textContent).toContain('年龄：任务未设置')
    expect(mounted.container.textContent).toContain('未采集到')
    expect(mounted.container.textContent).toContain('0')
    expect(mounted.container.textContent).not.toContain('未提供')

    await act(async () => [...mounted.container.querySelectorAll('button')]
      .find((button) => button.textContent?.includes('查看已选字段'))?.click())
    const details = mounted.container.querySelector('.task-results__details')
    expect(details?.textContent).toContain('作品数')
    expect(details?.textContent).toContain('0')
    expect(details?.textContent).toContain('个人简介')
    expect(details?.textContent).toContain('未采集到')
  })

  it('已设置性别和年龄条件但接口缺值时显示未采集到', async () => {
    listTaskResultsMock.mockResolvedValue({
      task_id: 'task-1',
      task_run_id: 'run-1',
      run_status: 'success',
      age_filter_configured: true,
      gender_filter_configured: true,
      selected_fields: ['gender', 'age'],
      total_count: 1,
      offset: 0,
      limit: 50,
      items: [{
        id: 'account-missing-filtered-fields',
        platform: 'tiktok',
        username: '缺失筛选字段测试',
        gender: null,
        age: null,
        data_source: 'TikHub API',
        collected_at: '2026-07-19T14:32:17Z',
        account_fields_json: {},
        field_evidence_json: {},
      }],
    })
    const mounted = mountPanel()

    await act(async () => Promise.resolve())

    expect(mounted.container.textContent).toContain('性别：未采集到')
    expect(mounted.container.textContent).toContain('年龄：未采集到')
    expect(mounted.container.textContent).not.toContain('性别：任务未设置')
    expect(mounted.container.textContent).not.toContain('年龄：任务未设置')
  })

  it('以所选结果字段而不是筛选开关判定缺失语义', async () => {
    listTaskResultsMock.mockResolvedValue({
      task_id: 'task-1',
      task_run_id: 'run-1',
      run_status: 'success',
      age_filter_configured: false,
      gender_filter_configured: false,
      selected_fields: ['gender', 'age'],
      total_count: 1,
      offset: 0,
      limit: 50,
      items: [{
        id: 'account-selected-demographics',
        platform: 'douyin',
        username: '字段选择口径测试',
        country_region: null,
        gender: null,
        age: null,
        followers_count: null,
        posts_count: null,
        profile_text: null,
        data_source: 'douyin.user_search',
        collected_at: '2026-07-19T14:32:17Z',
        account_fields_json: {},
        field_evidence_json: {},
      }],
    })
    const mounted = mountPanel()

    await act(async () => Promise.resolve())

    expect(mounted.container.textContent).toContain('性别：未采集到')
    expect(mounted.container.textContent).toContain('年龄：未采集到')
    const rowText = mounted.container.querySelector('tbody tr')?.textContent ?? ''
    expect(rowText.match(/任务未设置/g)).toHaveLength(4)
  })
})
