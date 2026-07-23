import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import TaskProblemPanel from './TaskProblemPanel'

describe('TaskProblemPanel', () => {
  it('自然语言历史失败显示原因、码、修改方式、重试状态、时间和保留状态', () => {
    const markup = renderToStaticMarkup(createElement(TaskProblemPanel, {
      kind: 'natural_parse',
      code: 'MODEL_RATE_LIMIT',
      message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
      retryable: true,
      attemptedAt: '2026-07-20T08:00:17Z',
      safeDetails: { retry_after: '17' },
      draftPreserved: true,
    }))

    expect(markup).toContain('aria-label="自然语言解析失败详情"')
    expect(markup).toContain('role="alert"')
    expect(markup).toContain('aria-live="assertive"')
    expect(markup).toContain('MODEL_RATE_LIMIT')
    expect(markup).toContain('修改方式')
    expect(markup).toContain('可重试')
    expect(markup).toContain('Retry-After：17')
    expect(markup).toContain('已保留')
  })

  it('失败摘要不重复状态和错误码，并把技术事实收进默认折叠详情', () => {
    const markup = renderToStaticMarkup(createElement(TaskProblemPanel, {
      kind: 'natural_parse',
      code: 'MODEL_CONFIG_ERROR',
      message: '尚未设置当前 AI 配置，请先在设置中完成真实连通性测试',
      retryable: false,
      attemptedAt: '2026-07-20T08:00:17Z',
      onAction: () => undefined,
    }))

    expect(markup.match(/MODEL_CONFIG_ERROR/g)).toHaveLength(1)
    expect(markup).not.toContain('>解析失败</span>')
    expect(markup).toContain('<details class="task-problem__details">')
    expect(markup).toContain('<summary><span>技术详情</span>')
    expect(markup).toContain('class="ghost-button task-problem__action"')
    expect(markup).toContain('<span>打开 AI 设置</span>')
  })

  it('白名单运行错误显示可直接修正的编辑指引', () => {
    const markup = renderToStaticMarkup(createElement(TaskProblemPanel, {
      kind: 'run',
      code: 'VALIDATION_ERROR',
      message: '地区和时间范围不能作为“小红书 · 搜索用户”端点的请求参数',
      retryable: false,
      attemptedAt: '2026-07-20T08:00:17Z',
      onAction: () => undefined,
    }))

    expect(markup).toContain('移除当前来源不支持的地区或时间条件')
    expect(markup).toContain('更换具有明确筛选能力的平台或来源')
    expect(markup).toContain('编辑任务')
    expect(markup).toContain('查看诊断')
    expect(markup).toContain('role="alert"')
  })

  it('结构化意图需要补充时不误称为解析失败', () => {
    const markup = renderToStaticMarkup(createElement(TaskProblemPanel, {
      kind: 'natural_parse',
      naturalState: 'needs_review',
      message: '缺少国家地区和预算',
      retryable: false,
      attemptedAt: '2026-07-20T08:00:17Z',
    }))

    expect(markup).toContain('aria-label="自然语言解析待补充详情"')
    expect(markup).toContain('role="status"')
    expect(markup).toContain('aria-live="polite"')
    expect(markup).toContain('解析完成，需要补充信息')
    expect(markup).not.toContain('解析失败')
  })
})
