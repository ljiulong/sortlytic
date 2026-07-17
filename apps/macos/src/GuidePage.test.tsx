import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import GuidePage from './GuidePage'

function renderGuide() {
  return renderToStaticMarkup(createElement(GuidePage, { onOpenSettings: vi.fn() }))
}

describe('GuidePage', () => {
  it('按连续步骤流展示完整上手顺序', () => {
    const markup = renderGuide()
    const titles = [
      '注册并验证账号',
      '创建 API Token',
      '添加到本地应用',
      '先小样本验证',
    ]

    expect(markup).toContain('guide-flow')
    expect(markup).toContain('<ol')
    titles.reduce((previousIndex, title) => {
      const currentIndex = markup.indexOf(title)
      expect(currentIndex).toBeGreaterThan(previousIndex)
      return currentIndex
    }, -1)
    expect(markup).toContain('打开设置')
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

  it('保留五个官方资源和 Token 请求格式', () => {
    const markup = renderGuide()

    expect((markup.match(/class="guide-resource-link"/g) ?? [])).toHaveLength(5)
    expect(markup).toContain('Authorization')
    expect(markup).toContain('Bearer YOUR_API_KEY')
    expect(markup).toContain('价格与免费额度')
  })
})
