import { createElement, type ComponentProps } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import Dashboard from './Dashboard'

const baseProps: ComponentProps<typeof Dashboard> = {
  workspace: {
    name: '本地工作区',
    storage: '/Users/test/Sortlytic',
    lastBackup: '尚未备份',
    health: '可用',
  },
  connections: [],
  metrics: [
    { label: '本地任务', value: '—', delta: '正在读取真实数据', tone: 'info' },
    { label: '入库记录', value: '—', delta: '正在读取真实数据', tone: 'info' },
  ],
  records: [],
  promptRuns: [],
  isBusy: false,
  selectedRecordId: '',
  onCreateTask: vi.fn(),
  onRefresh: vi.fn(),
  onSelectRecord: vi.fn(),
}

function renderDashboard(overrides: Partial<ComponentProps<typeof Dashboard>> = {}) {
  return renderToStaticMarkup(createElement(Dashboard, { ...baseProps, ...overrides }))
}

describe('Dashboard', () => {
  it('没有真实记录时显示完整空状态，不渲染空表格或预留检查栏', () => {
    const markup = renderDashboard()

    expect(markup).toContain('dashboard-empty-state')
    expect(markup).toContain('尚无真实记录')
    expect(markup).toContain('新建任务')
    expect(markup).not.toContain('<table')
    expect(markup).not.toContain('dashboard__inspector')
    expect(markup).toContain('data-with-inspector="false"')
  })

  it('未读取到的指标使用普通弱化值，不伪装成关键数字', () => {
    const markup = renderDashboard()

    expect(markup).toContain('data-available="false"')
    expect(markup).toContain('正在读取真实数据')
  })

  it('运行概览的主指标不使用反相黑底', () => {
    const markup = renderDashboard()

    expect(markup).not.toContain('overview-primary-metric')
    expect(markup).toContain('overview-fact overview-fact--lead')
  })

  it('有真实记录时显示数据表和对应来源检查区', () => {
    const markup = renderDashboard({
      records: [{
        id: 'record-1',
        platform: '小红书',
        title: '真实记录标题',
        author: '公开作者',
        region: 'CN',
        status: '已校验',
        sentiment: '中性',
        confidence: 0.92,
        engagement: 12,
        source: 'https://example.com/source',
        insight: '已验证洞察',
        evidence: '来源记录 record-1',
      }],
    })

    expect(markup).toContain('<table')
    expect(markup).toContain('dashboard__inspector')
    expect(markup).toContain('data-with-inspector="true"')
    expect(markup).toContain('真实记录标题')
    expect(markup).toContain('来源记录 record-1')
  })
})
