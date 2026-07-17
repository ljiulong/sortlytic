import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import GuidePage from './GuidePage'

function renderGuide() {
  return renderToStaticMarkup(createElement(GuidePage, { onOpenSettings: vi.fn() }))
}

describe('GuidePage', () => {
  it('按六个连续章节展示从配置到导出的完整工作流', () => {
    const markup = renderGuide()
    const titles = [
      '准备本地工作区',
      '配置 TikHub 数据来源',
      '配置 AI 处理',
      '创建并校验任务',
      '确认运行与管理任务',
      '按任务导出与复核',
    ]

    expect(markup).toContain('guide-handbook')
    expect(markup).toContain('<ol')
    expect((markup.match(/class="guide-chapter"/g) ?? [])).toHaveLength(6)
    titles.reduce((previousIndex, title) => {
      const currentIndex = markup.indexOf(title)
      expect(currentIndex).toBeGreaterThan(previousIndex)
      return currentIndex
    }, -1)
    expect(markup).toContain('打开设置')
  })

  it('详细说明地区搜索、筛选、任务管理和逐任务导出边界', () => {
    const markup = renderGuide()

    expect(markup).toContain('249 个 ISO 两位代码')
    expect(markup).toContain('中文名、英文名或两位代码')
    expect(markup).toContain('明确公开年龄')
    expect(markup).toContain('明确公开性别')
    expect(markup).toContain('确认运行')
    expect(markup).toContain('取消任务')
    expect(markup).toContain('删除任务')
    expect(markup).toContain('Excel')
    expect(markup).toContain('PDF')
    expect(markup).toContain('提示词版本')
    expect(markup).toContain('Schema')
    expect(markup).toContain('来源证据')
  })

  it('不再借用任务卡、连接卡、导出卡或计划网格', () => {
    const markup = renderGuide()

    expect(markup).not.toContain('glass-panel')
    expect(markup).not.toContain('connection-card')
    expect(markup).not.toContain('task-row')
    expect(markup).not.toContain('export-item')
    expect(markup).not.toContain('plan-grid')
    expect(markup).not.toContain('已纳入')
  })

  it('保留五个官方资源和 Token 请求格式，不再使用独立黑底侧栏', () => {
    const markup = renderGuide()

    expect((markup.match(/class="guide-resource-link"/g) ?? [])).toHaveLength(5)
    expect(markup).toContain('Authorization')
    expect(markup).toContain('Bearer YOUR_API_KEY')
    expect(markup).toContain('价格与免费额度')
    expect(markup).not.toContain('guide-page__sidebar')
    expect(markup).not.toContain('guide-token-block')
  })
})
