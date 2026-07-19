// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { renderToStaticMarkup } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import TaskQueue, {
  capabilitiesForStatus,
  confirmationForTaskAction,
  taskExportFormatOptions,
} from './TaskQueue'
import { i18n as appI18n } from './i18n'
import type { WorkbenchRuntimeData } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'

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
) {
  const container = document.createElement('div')
  const root = createRoot(container)
  const mounted = { container, root }
  document.body.append(container)
  mountedQueues.add(mounted)
  act(() => root.render(createElement(TaskQueue, {
    tasks,
    isBusy: false,
    onUpdateTask: vi.fn(),
    onCancelTask: vi.fn(),
    onConfirmTask,
    onDeleteTask: vi.fn(),
    onExportTask: vi.fn(),
  })))
  return mounted
}

beforeEach(async () => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  await appI18n.changeLanguage('zh-CN')
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
      onUpdateTask: vi.fn(),
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
    ['成功', false, false, false, true, true],
    ['部分成功', false, false, false, true, true],
    ['失败', false, false, false, false, true],
    ['已取消', false, false, false, false, true],
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

  it('等待确认任务提供编辑、取消、删除与确认运行四个独立入口', () => {
    const markup = renderQueue([waitingTask])

    expect(markup).toContain('编辑')
    expect(markup).toContain('title="取消任务"')
    expect(markup).toContain('title="删除任务"')
    expect(markup).toContain('确认运行')
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

  it('没有真实任务时显示完整空状态', () => {
    const markup = renderQueue([])

    expect(markup).toContain('task-queue__empty')
    expect(markup).toContain('还没有可运行的任务')
    expect(markup).toContain('前往“新建任务”')
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
