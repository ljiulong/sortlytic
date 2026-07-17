import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import TaskQueue from './TaskQueue'
import type { WorkbenchRuntimeData } from './use-workbench-backend'

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
})
