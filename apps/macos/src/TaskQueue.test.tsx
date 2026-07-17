import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import TaskQueue, { capabilitiesForStatus } from './TaskQueue'
import type { WorkbenchRuntimeData } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'

const waitingTask: WorkbenchRuntimeData['tasks'][number] = {
  id: 'task-waiting',
  name: '待确认任务',
  platform: 'TikTok',
  status: '等待确认',
  source: '表单式',
  progress: 0,
  records: 0,
  cost: '预计 3 次请求',
}

function renderQueue(tasks: WorkbenchRuntimeData['tasks']) {
  return renderToStaticMarkup(
    createElement(TaskQueue, {
      tasks,
      isBusy: false,
      onUpdateTask: vi.fn(),
      onCancelTask: vi.fn(),
      onConfirmTask: vi.fn(),
      onExportTask: vi.fn(),
    }),
  )
}

describe('TaskQueue', () => {
  it.each<[TaskStatus, boolean, boolean, boolean, boolean]>([
    ['等待确认', true, true, true, false],
    ['待人工确认', true, true, false, false],
    ['已排队', false, true, false, false],
    ['运行中', false, true, false, false],
    ['成功', false, false, false, true],
    ['部分成功', false, false, false, true],
    ['失败', false, false, false, false],
  ])('%s 状态使用正确的任务操作权限', (status, canEdit, canCancel, canConfirm, canExport) => {
    expect(capabilitiesForStatus(status)).toEqual({
      canEdit,
      canCancel,
      canConfirm,
      canExport,
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

  it('等待确认任务提供编辑、取消与确认运行入口', () => {
    const markup = renderQueue([waitingTask])

    expect(markup).toContain('编辑')
    expect(markup).toContain('取消任务')
    expect(markup).toContain('确认运行')
  })

  it('每条任务可选择 Excel 或 PDF，未完成任务不允许提前导出', () => {
    const markup = renderQueue([waitingTask])

    expect(markup).toContain('Excel 工作簿')
    expect(markup).toContain('PDF 报告')
    expect(markup).toMatch(/<button[^>]*disabled=""[^>]*>[^<]*(?:<[^>]+>)*导出/)
  })

  it('成功任务允许按所选格式导出，但不再显示运行确认', () => {
    const markup = renderQueue([{ ...waitingTask, status: '成功', progress: 100 }])

    expect(markup).not.toContain('确认运行')
    expect(markup).toContain('导出')
    expect(markup).not.toMatch(/<button[^>]*disabled=""[^>]*>[^<]*(?:<[^>]+>)*导出/)
  })

  it('没有真实任务时显示完整空状态', () => {
    const markup = renderQueue([])

    expect(markup).toContain('task-queue__empty')
    expect(markup).toContain('还没有可运行的任务')
    expect(markup).toContain('前往“新建任务”')
  })
})
