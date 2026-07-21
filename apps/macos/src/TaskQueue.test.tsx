// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ExportJobView } from './backend-api'
import TaskQueue from './TaskQueue'
import {
  capabilitiesForStatus,
  confirmationForTaskAction,
  taskExportFormatOptions,
} from './task-queue-config'
import { i18n as appI18n } from './i18n'
import type { TaskExportInput, WorkbenchRuntimeData } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'

const openPathMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/plugin-opener', () => ({ openPath: openPathMock }))
vi.mock('./TaskResultsPanel', () => ({
  default: ({ taskId }: { taskId: string }) => createElement(
    'div',
    { 'data-testid': 'task-results-panel' },
    `结果面板 ${taskId}`,
  ),
}))

const waitingTask: WorkbenchRuntimeData['tasks'][number] = {
  id: 'task-waiting',
  name: '待确认任务',
  platform: 'TikTok',
  status: '等待确认',
  source: '表单式',
  sourceType: 'form',
  progress: 0,
  records: 0,
  cost: '预计 3 次请求',
  requestCount: 3,
  dataTypeCode: 'keyword_search',
}

const mountedQueues = new Set<{ container: HTMLDivElement; root: Root }>()

function mountQueue(
  onConfirmTask: (taskId: string) => Promise<unknown>,
  tasks: WorkbenchRuntimeData['tasks'] = [waitingTask],
  onExportTask: (input: TaskExportInput) => Promise<ExportJobView> = vi.fn(),
  onDeleteTask: (taskId: string) => Promise<unknown> = vi.fn(),
  onRetryNaturalTask: (taskId: string, intentText: string) => Promise<unknown> = vi.fn(),
  onRetryTask: (taskId: string) => Promise<unknown> = vi.fn(),
  onEditTask: (taskId: string) => void = vi.fn(),
) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  document.body.append(container)
  mountedQueues.add(mounted)
  act(() => root.render(createElement(TaskQueue, {
    tasks,
    isBusy: false,
    onEditTask,
    onCancelTask: vi.fn(),
    onConfirmTask,
    onDeleteTask,
    onExportTask,
    onRetryNaturalTask,
    onRetryTask,
  })))
  return mounted
}

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  await appI18n.changeLanguage('zh-CN')
  openPathMock.mockReset()
  openPathMock.mockResolvedValue(undefined)
})

afterEach(() => {
  for (const mounted of mountedQueues) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedQueues.clear()
})

function renderQueue(tasks: WorkbenchRuntimeData['tasks']) {
  return renderToStaticMarkup(
    createElement(TaskQueue, {
      tasks,
      isBusy: false,
      onEditTask: vi.fn(),
      onCancelTask: vi.fn(),
      onConfirmTask: vi.fn(),
      onDeleteTask: vi.fn(),
      onExportTask: vi.fn(),
    }),
  )
}

describe('TaskQueue', () => {
  it.each<[TaskStatus, boolean, boolean, boolean, boolean, boolean]>([
    ['等待确认', true, true, true, false, true],
    ['待人工确认', true, true, false, false, true],
    ['已排队', false, true, false, false, false],
    ['运行中', false, true, false, false, false],
    ['成功', true, false, false, true, true],
    ['部分成功', true, false, false, true, true],
    ['失败', true, false, false, false, true],
    ['已取消', true, false, false, false, true],
  ])('%s 状态使用正确的任务操作权限', (
    status,
    canEdit,
    canCancel,
    canConfirm,
    canExport,
    canDelete,
  ) => {
    expect(capabilitiesForStatus(status)).toEqual({
      canEdit,
      canCancel,
      canConfirm,
      canExport,
      canDelete,
    })
  })

  it('任务卡使用内容区、统计区与底部操作区，不再使用窄列按钮塔', () => {
    const markup = renderQueue([waitingTask])

    const headerIndex = markup.indexOf('task-card__header')
    const statsIndex = markup.indexOf('task-card__stats')
    const footerIndex = markup.indexOf('task-card__footer')

    expect(headerIndex).toBeGreaterThan(-1)
    expect(statsIndex).toBeGreaterThan(headerIndex)
    expect(footerIndex).toBeGreaterThan(statsIndex)
    expect(markup).toContain('task-card__actions')
    expect(markup).toContain('task-card__export')
    expect(markup).not.toContain('progress-cell')
    expect(markup).toContain('role="progressbar"')
    expect(markup).toContain('aria-valuenow="0"')
  })

  it('自然语言失败任务显示解析失败、原始需求、真实错误和修改方式', () => {
    const markup = renderQueue([{
      ...waitingTask,
      id: 'task-natural-failed',
      source: '自然语言',
      sourceType: 'natural_language',
      status: '待人工确认',
      naturalParseAttempt: {
        id: 'attempt-1',
        task_id: 'task-natural-failed',
        intent_text: '用中文查找英国 TikTok 宠物用品账号',
        parse_status: 'failed',
        parse_phase: 'requesting_ai',
        error_code: 'MODEL_RATE_LIMIT',
        error_message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
        retryable: true,
        error_safe_details_json: { retry_after: '17' },
        created_at: '2026-07-20T08:00:00Z',
        updated_at: '2026-07-20T08:00:17Z',
      },
    }])

    expect(markup).toContain('解析失败')
    expect(markup).toContain('原始需求：用中文查找英国 TikTok 宠物用品账号')
    expect(markup).toContain('MODEL_RATE_LIMIT')
    expect(markup).toContain('AI 服务请求过于频繁或额度不足')
    expect(markup).toContain('修改方式')
    expect(markup).toContain('重新尝试')
    expect(markup).not.toContain('发生未知错误')
  })

  it('自然语言待补充任务显示待修正而不是待人工确认', () => {
    const markup = renderQueue([{
      ...waitingTask,
      id: 'task-natural-needs-review',
      status: '待人工确认',
      naturalParseAttempt: {
        id: 'attempt-needs-review',
        task_id: 'task-natural-needs-review',
        intent_text: '查找英国 TikTok 宠物用品账号',
        parse_status: 'needs_review',
        parse_phase: 'needs_review',
        error_code: 'VALIDATION_ERROR',
        error_message: '解析完成，需要补充预算',
        retryable: false,
        error_safe_details_json: { missing_fields: ['budget_limit_micros'] },
        created_at: '2026-07-20T08:00:00Z',
        updated_at: '2026-07-20T08:00:17Z',
      },
    }])

    expect(markup).toContain('>待修正</span>')
    expect(markup).not.toContain('>待人工确认</span>')
    expect(markup).toContain('解析完成，需要补充信息')
  })

  it('查看解析记录打开当前持久任务编辑器', () => {
    const onEditTask = vi.fn()
    const mounted = mountQueue(
      vi.fn(),
      [{
        ...waitingTask,
        id: 'task-parse-history',
        naturalParseAttempt: {
          id: 'attempt-history',
          task_id: 'task-parse-history',
          intent_text: '查找英国 TikTok 宠物用品账号',
          parse_status: 'failed',
          parse_phase: 'requesting_ai',
          error_code: 'MODEL_AUTH_ERROR',
          error_message: 'AI 服务鉴权失败',
          retryable: false,
          error_safe_details_json: {},
          created_at: '2026-07-20T08:00:00Z',
          updated_at: '2026-07-20T08:00:17Z',
        },
      }],
      vi.fn(),
      vi.fn(),
      vi.fn(),
      vi.fn(),
      onEditTask,
    )

    act(() => [...mounted.container.querySelectorAll('button')]
      .find((button) => button.textContent?.includes('查看解析记录'))?.click())

    expect(onEditTask).toHaveBeenCalledWith('task-parse-history')
  })

  it('自然语言任务卡串行重试并在失败时显示卡内错误', async () => {
    let rejectRetry!: (error: Error) => void
    const pendingRetry = new Promise<unknown>((_resolve, reject) => {
      rejectRetry = reject
    })
    const onRetryNaturalTask = vi.fn(() => pendingRetry)
    const naturalTask: WorkbenchRuntimeData['tasks'][number] = {
      ...waitingTask,
      id: 'task-natural-retry',
      source: '自然语言',
      sourceType: 'natural_language',
      status: '待人工确认',
      naturalParseAttempt: {
        id: 'attempt-retry',
        task_id: 'task-natural-retry',
        intent_text: '查找英国 TikTok 宠物用品账号',
        parse_status: 'failed',
        parse_phase: 'requesting_ai',
        error_code: 'MODEL_RATE_LIMIT',
        error_message: 'AI 服务请求过于频繁',
        retryable: true,
        error_safe_details_json: { retry_after: '5' },
        created_at: '2026-07-20T08:00:00Z',
        updated_at: '2026-07-20T08:01:30Z',
      },
    }
    const mounted = mountQueue(
      vi.fn(),
      [naturalTask],
      vi.fn(),
      vi.fn(),
      onRetryNaturalTask,
    )
    const retryButton = [...mounted.container.querySelectorAll('button')]
      .find((button) => button.textContent?.includes('重新尝试'))

    act(() => {
      retryButton?.click()
      retryButton?.click()
    })

    expect(onRetryNaturalTask).toHaveBeenCalledTimes(1)
    expect(onRetryNaturalTask).toHaveBeenCalledWith(
      'task-natural-retry',
      '查找英国 TikTok 宠物用品账号',
    )
    expect(retryButton?.disabled).toBe(true)

    await act(async () => {
      rejectRetry(new Error('AI 临时网络错误，请重新解析'))
      await pendingRetry.catch(() => undefined)
    })

    expect(mounted.container.textContent).toContain('AI 临时网络错误，请重新解析')
    expect(retryButton?.disabled).toBe(false)
  })

  it('等待确认任务提供编辑、取消、删除与确认运行四个独立入口', () => {
    const markup = renderQueue([waitingTask])

    expect(markup).toContain('编辑')
    expect(markup).toContain('title="编辑任务计划"')
    expect(markup).toContain('title="取消任务"')
    expect(markup).toContain('title="删除任务"')
    expect(markup).toContain('确认运行')
  })

  it('编辑入口打开独立完整编辑页，不再只切换任务名称输入框', () => {
    const onEditTask = vi.fn()
    const container = document.createElement('div')
    const root = createRoot(container)
    act(() => root.render(createElement(TaskQueue, {
      tasks: [waitingTask],
      isBusy: false,
      onEditTask,
      onCancelTask: vi.fn(),
      onConfirmTask: vi.fn(),
      onDeleteTask: vi.fn(),
      onExportTask: vi.fn(),
    })))

    const editButton = [...container.querySelectorAll('button')]
      .find((button) => button.textContent?.includes('编辑'))
    act(() => editButton?.click())

    expect(onEditTask).toHaveBeenCalledWith(waitingTask.id)
    expect(container.querySelector('[id^="task-name-"]')).toBeNull()
    act(() => root.unmount())
  })

  it('两段确认只调用一次运行入口，并在预检失败时显示尚未入队原因', async () => {
    const onConfirmTask = vi.fn(async () => {
      throw new Error('实时计价请求过于频繁，请稍后重试')
    })
    const mounted = mountQueue(onConfirmTask)
    const openConfirmation = Array.from(mounted.container.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('确认运行'))

    act(() => openConfirmation?.click())

    expect(onConfirmTask).not.toHaveBeenCalled()
    expect(mounted.container.textContent).toContain('确认后任务将进入执行队列并可能产生费用')

    const submitConfirmation = mounted.container.querySelector<HTMLButtonElement>(
      '.task-card__confirmation .primary-button',
    )
    await act(async () => submitConfirmation?.click())

    expect(onConfirmTask).toHaveBeenCalledOnce()
    expect(onConfirmTask).toHaveBeenCalledWith(waitingTask.id)
    expect(mounted.container.textContent).toContain(
      '任务尚未入队：实时计价请求过于频繁，请稍后重试',
    )
  })

  it('确认运行成功后进入该任务独立预览，并可返回完整任务列表', async () => {
    const otherTask = {
      ...waitingTask,
      id: 'task-other',
      name: '其他等待任务',
    }
    const onConfirmTask = vi.fn(async () => undefined)
    const mounted = mountQueue(onConfirmTask, [waitingTask, otherTask])
    const openConfirmation = Array.from(mounted.container.querySelectorAll('button'))
      .find((button) => button.textContent?.includes('确认运行'))

    act(() => openConfirmation?.click())
    const submitConfirmation = mounted.container.querySelector<HTMLButtonElement>(
      '.task-card__confirmation .primary-button',
    )
    await act(async () => submitConfirmation?.click())

    expect(onConfirmTask).toHaveBeenCalledWith(waitingTask.id)
    expect(mounted.container.textContent).toContain(waitingTask.name)
    expect(mounted.container.textContent).not.toContain(otherTask.name)

    const backToList = Array.from(mounted.container.querySelectorAll('button'))
      .find((button) => button.getAttribute('aria-label') === '返回任务列表')
    expect(backToList).toBeTruthy()

    act(() => backToList?.click())
    expect(mounted.container.textContent).toContain(waitingTask.name)
    expect(mounted.container.textContent).toContain(otherTask.name)
  })

  it('无效计划明确显示计划需修正，且不提供确认运行入口', () => {
    const markup = renderQueue([{ ...waitingTask, status: '待人工确认' }])

    expect(markup).toContain('计划需修正')
    expect(markup).not.toContain('>待人工确认</span>')
    expect(markup).not.toContain('确认运行')
  })

  it('取消和删除使用不同确认文案，删除明确提示关联数据不可恢复', () => {
    expect(confirmationForTaskAction('confirm-cancel')).toMatchObject({
      ariaLabel: '确认取消任务',
      buttonLabel: '确认取消',
    })
    expect(confirmationForTaskAction('confirm-delete')).toMatchObject({
      ariaLabel: '确认删除任务',
      buttonLabel: '确认删除',
    })
    expect(confirmationForTaskAction('confirm-cancel').message).toContain('保留任务与运行记录')
    expect(confirmationForTaskAction('confirm-cancel').message).toContain('可能仍会完成并产生费用')
    expect(confirmationForTaskAction('confirm-cancel').message).toContain('不会写入本地')
    expect(confirmationForTaskAction('confirm-delete').message).toContain('关联本地数据')
    expect(confirmationForTaskAction('confirm-delete').message).toContain('无法恢复')
  })

  it('全选只选择可删除任务，批量删除经过统一二次确认', async () => {
    const deletableSuccess = {
      ...waitingTask,
      id: 'task-success',
      name: '成功任务',
      status: '成功' as const,
      progress: 100,
    }
    const deletableCancelled = {
      ...waitingTask,
      id: 'task-cancelled',
      name: '已取消任务',
      status: '已取消' as const,
    }
    const queuedTask = {
      ...waitingTask,
      id: 'task-queued',
      name: '已排队任务',
      status: '已排队' as const,
    }
    const runningTask = {
      ...waitingTask,
      id: 'task-running',
      name: '运行中任务',
      status: '运行中' as const,
    }
    const onDeleteTask = vi.fn(async () => undefined)
    const mounted = mountQueue(
      vi.fn(async () => undefined),
      [deletableSuccess, deletableCancelled, queuedTask, runningTask],
      vi.fn(),
      onDeleteTask,
    )
    expect(mounted.container.querySelector('input[type="checkbox"]')).toBeNull()
    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="批量管理"]',
    )?.click())
    const selectAll = mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择全部可删除任务"]',
    )

    expect(selectAll).toBeTruthy()
    act(() => selectAll?.click())

    expect(mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择任务 成功任务"]',
    )?.checked).toBe(true)
    expect(mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择任务 已取消任务"]',
    )?.checked).toBe(true)
    expect(mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择任务 已排队任务"]',
    )?.disabled).toBe(true)
    expect(mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择任务 运行中任务"]',
    )?.disabled).toBe(true)
    expect(mounted.container.textContent).toContain('已选择 2 条任务')

    const bulkDelete = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="批量删除"]',
    )
    act(() => bulkDelete?.click())

    expect(onDeleteTask).not.toHaveBeenCalled()
    expect(mounted.container.textContent).toContain('将永久删除选中的 2 条任务及关联本地数据')

    const confirmBulkDelete = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="确认批量删除"]',
    )
    await act(async () => confirmBulkDelete?.click())

    expect(onDeleteTask.mock.calls).toEqual([
      [deletableSuccess.id],
      [deletableCancelled.id],
    ])
    expect(onDeleteTask).not.toHaveBeenCalledWith(queuedTask.id)
    expect(onDeleteTask).not.toHaveBeenCalledWith(runningTask.id)
  })

  it('批量删除部分失败时只保留失败任务并显示原因', async () => {
    const successTask = {
      ...waitingTask,
      id: 'task-delete-success',
      name: '可成功删除任务',
      status: '成功' as const,
    }
    const failedTask = {
      ...waitingTask,
      id: 'task-delete-failed',
      name: '删除失败任务',
      status: '失败' as const,
    }
    const onDeleteTask = vi.fn(async (taskId: string) => {
      if (taskId === failedTask.id) throw new Error('本地文件仍被占用')
    })
    const mounted = mountQueue(
      vi.fn(async () => undefined),
      [successTask, failedTask],
      vi.fn(),
      onDeleteTask,
    )

    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="批量管理"]',
    )?.click())
    act(() => mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择全部可删除任务"]',
    )?.click())
    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="批量删除"]',
    )?.click())
    await act(async () => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="确认批量删除"]',
    )?.click())

    expect(mounted.container.querySelector('[role="alert"]')?.textContent)
      .toContain('1 / 2 条失败：本地文件仍被占用')
    expect(mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择任务 可成功删除任务"]',
    )?.checked).toBe(false)
    expect(mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择任务 删除失败任务"]',
    )?.checked).toBe(true)
  })

  it('删除选择框只在批量管理模式显示，退出时清空选择', () => {
    const mounted = mountQueue(vi.fn(async () => undefined), [{
      ...waitingTask,
      status: '成功',
      name: '可批量管理任务',
    }])

    expect(mounted.container.querySelector('input[type="checkbox"]')).toBeNull()
    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="批量管理"]',
    )?.click())

    const taskSelection = mounted.container.querySelector<HTMLInputElement>(
      'input[aria-label="选择任务 可批量管理任务"]',
    )
    expect(taskSelection).toBeTruthy()
    act(() => taskSelection?.click())
    expect(mounted.container.textContent).toContain('已选择 1 条任务')

    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="完成"]',
    )?.click())
    expect(mounted.container.querySelector('input[type="checkbox"]')).toBeNull()

    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="批量管理"]',
    )?.click())
    expect(mounted.container.textContent).toContain('已选择 0 条任务')
  })

  it('每条任务可选择 Excel 或 PDF，未完成任务不允许提前导出', () => {
    const markup = renderQueue([waitingTask])

    expect(taskExportFormatOptions).toEqual([
      { value: 'xlsx', label: 'Excel 工作簿' },
      { value: 'pdf', label: 'PDF 报告' },
    ])
    expect(markup).toMatch(/<button[^>]*id="task-export-format-task-waiting"[^>]*aria-haspopup="listbox"/)
    expect(markup).not.toContain('<select')
    expect(markup).toMatch(/<button[^>]*disabled=""[^>]*>[^<]*(?:<[^>]+>)*导出/)
  })

  it('成功任务允许按所选格式导出，但不再显示运行确认', () => {
    const markup = renderQueue([{ ...waitingTask, status: '成功', progress: 100 }])

    expect(markup).not.toContain('确认运行')
    expect(markup).toContain('导出')
    expect(markup).not.toMatch(/<button[^>]*disabled=""[^>]*>[^<]*(?:<[^>]+>)*导出/)
  })

  it('成功任务可进入应用内结果预览并返回任务列表', () => {
    const successTask = {
      ...waitingTask,
      id: 'task-success-results',
      name: '有结果的成功任务',
      status: '成功' as const,
      progress: 100,
      records: 2,
    }
    const otherTask = {
      ...waitingTask,
      id: 'task-other-results',
      name: '其他任务',
    }
    const mounted = mountQueue(vi.fn(async () => undefined), [successTask, otherTask])

    expect(mounted.container.querySelector('[data-testid="task-results-panel"]')).toBeNull()
    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="查看结果"]',
    )?.click())

    expect(mounted.container.textContent).toContain('结果面板 task-success-results')
    expect(mounted.container.textContent).not.toContain(otherTask.name)

    act(() => mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="返回任务列表"]',
    )?.click())
    expect(mounted.container.textContent).toContain(otherTask.name)
    expect(mounted.container.querySelector('[data-testid="task-results-panel"]')).toBeNull()
  })

  it('导出成功后自动打开绝对路径，并在当前任务提供再次打开入口', async () => {
    const filePath = '/Users/test/Library/Application Support/Sortlytic/exports/result.xlsx'
    const exportJob: ExportJobView = {
      id: 'export-1',
      report_id: 'report-1',
      export_type: 'xlsx',
      status: 'success',
      file_path: filePath,
      file_hash: 'sha256',
      file_size: 2048,
      created_at: '2026-07-19T00:00:00Z',
      completed_at: '2026-07-19T00:00:01Z',
    }
    const onExportTask = vi.fn(async () => exportJob)
    const mounted = mountQueue(
      vi.fn(async () => undefined),
      [{ ...waitingTask, status: '成功', progress: 100 }],
      onExportTask,
    )
    const exportButton = mounted.container.querySelector<HTMLButtonElement>('button[aria-label="导出"]')

    await act(async () => exportButton?.click())

    expect(onExportTask).toHaveBeenCalledWith({ taskId: waitingTask.id, format: 'xlsx' })
    expect(openPathMock).toHaveBeenCalledWith(filePath)
    expect(mounted.container.textContent).toContain('导出文件已生成')
    expect(mounted.container.textContent).toContain(filePath)

    const openAgain = mounted.container.querySelector<HTMLButtonElement>(
      'button[aria-label="打开导出文件"]',
    )
    expect(openAgain).toBeTruthy()

    await act(async () => openAgain?.click())
    expect(openPathMock).toHaveBeenCalledTimes(2)
  })

  it('导出失败时在当前任务显示明确错误', async () => {
    const onExportTask = vi.fn(async (): Promise<ExportJobView> => {
      throw new Error('导出目录不可写')
    })
    const mounted = mountQueue(
      vi.fn(async () => undefined),
      [{ ...waitingTask, status: '成功', progress: 100 }],
      onExportTask,
    )
    const exportButton = mounted.container.querySelector<HTMLButtonElement>('button[aria-label="导出"]')

    await act(async () => exportButton?.click())

    expect(openPathMock).not.toHaveBeenCalled()
    expect(mounted.container.querySelector('[role="alert"]')?.textContent)
      .toContain('导出失败：导出目录不可写')
  })

  it('文件已生成但系统打开失败时保留路径并显示打开错误', async () => {
    const filePath = '/Users/test/Sortlytic/exports/result.pdf'
    const onExportTask = vi.fn(async (): Promise<ExportJobView> => ({
      id: 'export-pdf',
      report_id: 'report-pdf',
      export_type: 'pdf',
      status: 'success',
      file_path: filePath,
      created_at: '2026-07-19T00:00:00Z',
    }))
    openPathMock.mockRejectedValueOnce(new Error('系统没有关联应用'))
    const mounted = mountQueue(
      vi.fn(async () => undefined),
      [{ ...waitingTask, status: '成功', progress: 100 }],
      onExportTask,
    )
    const exportButton = mounted.container.querySelector<HTMLButtonElement>('button[aria-label="导出"]')

    await act(async () => exportButton?.click())

    expect(mounted.container.textContent).toContain(filePath)
    expect(mounted.container.querySelector('[role="alert"]')?.textContent)
      .toContain('文件已生成，但无法打开：系统没有关联应用')
    expect(mounted.container.querySelector('button[aria-label="打开导出文件"]')).toBeTruthy()
  })

  it('导出成功但缺少后端文件路径时拒绝伪装为可打开', async () => {
    const onExportTask = vi.fn(async (): Promise<ExportJobView> => ({
      id: 'export-without-path',
      report_id: 'report-without-path',
      export_type: 'xlsx',
      status: 'success',
      created_at: '2026-07-19T00:00:00Z',
    }))
    const mounted = mountQueue(
      vi.fn(async () => undefined),
      [{ ...waitingTask, status: '成功', progress: 100 }],
      onExportTask,
    )
    const exportButton = mounted.container.querySelector<HTMLButtonElement>('button[aria-label="导出"]')

    await act(async () => exportButton?.click())

    expect(openPathMock).not.toHaveBeenCalled()
    expect(mounted.container.querySelector('[role="alert"]')?.textContent)
      .toContain('导出成功，但后端未返回文件路径')
  })

  it('失败任务展示最新运行阶段、安全错误和重试状态', () => {
    const markup = renderQueue([{
      ...waitingTask,
      status: '失败',
      latestRun: {
        id: 'run-2',
        attemptNumber: 2,
        status: 'failed',
        currentStage: '持久化采集结果',
        currentStageCode: 'PERSISTING_RESULTS',
        errorCode: 'TIKHUB_REQUEST_ERROR',
        errorMessage: 'TikHub 请求超时',
        retryable: true,
        startedAt: '2026-07-17T08:00:00Z',
        endedAt: '2026-07-17T08:00:30Z',
      },
    }])

    expect(markup).toContain('task-card__run-details')
    expect(markup).toContain('最近一次运行')
    expect(markup).toContain('第 2 次尝试')
    expect(markup).toContain('持久化采集结果')
    expect(markup).toContain('TIKHUB_REQUEST_ERROR')
    expect(markup).toContain('TikHub 请求超时')
    expect(markup).toContain('<dt>可重试</dt><dd>是</dd>')
    expect(markup).toContain('dateTime="2026-07-17T08:00:00Z"')
    expect(markup).toContain('查看运行日志')
    expect(markup).toContain('aria-controls=')
  })

  it('运行失败的重新尝试调用 retry_task 入口并保持单次在途', async () => {
    let resolveRetry!: () => void
    const pendingRetry = new Promise<void>((resolve) => {
      resolveRetry = resolve
    })
    const onConfirmTask = vi.fn(async () => undefined)
    const onRetryTask = vi.fn(() => pendingRetry)
    const failedTask: WorkbenchRuntimeData['tasks'][number] = {
      ...waitingTask,
      id: 'task-run-retry',
      status: '失败',
      latestRun: {
        id: 'run-failed',
        attemptNumber: 1,
        status: 'failed',
        currentStage: '执行失败',
        currentStageCode: 'EXECUTION_FAILED',
        errorCode: 'TIKHUB_RATE_LIMIT',
        errorMessage: 'TikHub 暂时触发限流',
        retryable: true,
        startedAt: '2026-07-21T00:00:00Z',
        endedAt: '2026-07-21T00:00:10Z',
      },
    }
    const mounted = mountQueue(
      onConfirmTask,
      [failedTask],
      vi.fn(),
      vi.fn(),
      vi.fn(),
      onRetryTask,
    )
    const retryButton = [...mounted.container.querySelectorAll('button')]
      .find((button) => button.textContent?.includes('重新尝试'))

    act(() => {
      retryButton?.click()
      retryButton?.click()
    })

    expect(onRetryTask).toHaveBeenCalledOnce()
    expect(onRetryTask).toHaveBeenCalledWith('task-run-retry')
    expect(onConfirmTask).not.toHaveBeenCalled()
    expect(retryButton?.disabled).toBe(true)
    expect(retryButton?.textContent).toContain('正在重新尝试')

    await act(async () => {
      resolveRetry()
      await pendingRetry
    })
    expect(retryButton?.disabled).toBe(false)
    expect(retryButton?.textContent).toContain('重新尝试')
  })

  it('没有真实任务时显示完整空状态', () => {
    const markup = renderQueue([])

    expect(markup).toContain('task-queue__empty')
    expect(markup).toContain('还没有可运行的任务')
    expect(markup).toContain('前往“新建任务”')
  })

  it('Schema v4 任务卡使用账号数据文案而不是评论用户回退', () => {
    const markup = renderQueue([{ ...waitingTask, dataTypeCode: 'account' }])

    expect(markup).toContain('账号数据')
    expect(markup).not.toContain('评论用户')
  })

  it('英文模式使用本地化的任务来源、请求估算和数据类型', async () => {
    await appI18n.changeLanguage('en-US')
    const markup = renderQueue([{ ...waitingTask, name: 'Research task', sourceType: 'natural_language' }])

    expect(markup).toContain('Natural language')
    expect(markup).toContain('Estimated 3 requests')
    expect(markup).toContain('Search-result accounts')
    expect(markup).not.toContain('自然语言')
    expect(markup).not.toContain('预计 3 次请求')
    expect(markup).not.toContain('搜索结果账号')

    await appI18n.changeLanguage('zh-CN')
  })

  it('英文模式使用稳定代码翻译运行阶段和安全错误', async () => {
    await appI18n.changeLanguage('en-US')
    const markup = renderQueue([{
      ...waitingTask,
      name: 'Failed task',
      status: '失败',
      latestRun: {
        id: 'run-english',
        attemptNumber: 1,
        status: 'failed',
        currentStage: '持久化采集结果',
        currentStageCode: 'PERSISTING_RESULTS',
        errorCode: 'TIKHUB_REQUEST_ERROR',
        errorMessage: 'TikHub 请求超时',
        retryable: true,
        startedAt: '2026-07-17T08:00:00Z',
        endedAt: '2026-07-17T08:00:30Z',
      },
    }])

    expect(markup).toContain('Saving collected results')
    expect(markup).toContain('The TikHub request failed before completion.')
    expect(markup).not.toContain('持久化采集结果')
    expect(markup).not.toContain('TikHub 请求超时')

    const unknownMarkup = renderQueue([{
      ...waitingTask,
      status: '失败',
      latestRun: {
        id: 'run-unknown',
        attemptNumber: 1,
        status: 'failed',
        currentStage: '历史自定义阶段',
        currentStageCode: 'UNKNOWN_STAGE',
        errorCode: 'CUSTOM_FAILURE',
        errorMessage: '历史中文错误',
        retryable: false,
        startedAt: '2026-07-17T08:00:00Z',
      },
    }])
    expect(unknownMarkup).toContain('Unrecognized run stage (UNKNOWN_STAGE)')
    expect(unknownMarkup).toContain('Use error code CUSTOM_FAILURE for diagnosis.')
    expect(unknownMarkup).not.toContain('历史自定义阶段')
    expect(unknownMarkup).not.toContain('历史中文错误')

    await appI18n.changeLanguage('zh-CN')
  })
})
