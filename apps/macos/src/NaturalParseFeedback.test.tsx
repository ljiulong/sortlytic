import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import NaturalParseFeedback from './NaturalParseFeedback'
import type { NaturalParseState } from './natural-parse-state'

function render(state: NaturalParseState) {
  return renderToStaticMarkup(createElement(NaturalParseFeedback, {
    state,
    onRetry: vi.fn(),
    onOpenAiSettings: vi.fn(),
    onSwitchToForm: vi.fn(),
    onViewDiagnostics: vi.fn(),
  }))
}

describe('NaturalParseFeedback', () => {
  it('解析中使用 status 语义并显示真实阶段、模型和安全边界', () => {
    const markup = render({
      phase: 'requesting_ai',
      intentText: '查找英国 TikTok 宠物账号',
      startedAt: new Date().toISOString(),
      providerId: '生产 DeepSeek',
      modelId: 'deepseek-v4-flash',
      draftPreserved: true,
    })

    expect(markup).toContain('role="status"')
    expect(markup).toContain('aria-live="polite"')
    expect(markup).toContain('等待模型响应')
    expect(markup).toContain('deepseek-v4-flash')
    expect(markup).toContain('解析不会自动调用 TikHub')
  })

  it('失败使用 alert 语义并显示真实原因、错误码、修改方式和草稿保留', () => {
    const markup = render({
      phase: 'failed',
      taskId: 'task-1',
      intentText: '查找英国 TikTok 宠物账号',
      startedAt: '2026-07-20T08:00:00Z',
      finishedAt: '2026-07-20T08:00:17Z',
      problem: {
        code: 'MODEL_RATE_LIMIT',
        stage: 'requesting_ai',
        message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
        retryable: true,
        safeDetails: { retry_after: '17' },
      },
      draftPreserved: true,
    })

    expect(markup).toContain('role="alert"')
    expect(markup).toContain('aria-live="assertive"')
    expect(markup).toContain('AI 服务请求过于频繁或额度不足')
    expect(markup).toContain('MODEL_RATE_LIMIT')
    expect(markup).toContain('失败阶段</dt><dd>等待模型响应')
    expect(markup).toContain('修改方式')
    expect(markup).toContain('已保留')
    expect(markup).toContain('重新解析')
    expect(markup).toContain('查看诊断')
  })

  it('配置错误显示打开 AI 设置，Schema 错误显示切换表单', () => {
    const base = {
      phase: 'failed' as const,
      intentText: '测试',
      draftPreserved: true,
    }
    const authMarkup = render({
      ...base,
      problem: {
        code: 'MODEL_AUTH_ERROR',
        stage: 'preparing',
        message: 'AI 服务鉴权失败',
        retryable: false,
        safeDetails: {},
      },
    })
    const configMarkup = render({
      ...base,
      problem: {
        code: 'MODEL_CONFIG_ERROR',
        stage: 'preparing',
        message: '尚未设置当前 AI 配置，请先在设置中完成真实连通性测试',
        retryable: false,
        safeDetails: {},
      },
    })
    const legacyConfigMarkup = render({
      ...base,
      problem: {
        code: 'VALIDATION_ERROR',
        stage: 'preparing',
        message: '尚未设置当前 AI 配置，请先在设置中完成真实连通性测试',
        retryable: false,
        safeDetails: {},
      },
    })
    const schemaMarkup = render({
      ...base,
      problem: {
        code: 'MODEL_SCHEMA_ERROR',
        stage: 'validating_intent',
        message: '模型输出缺少 region_code',
        retryable: false,
        safeDetails: {},
      },
    })

    expect(authMarkup).toContain('打开 AI 设置')
    expect(configMarkup).toContain('打开 AI 设置')
    expect(configMarkup).not.toContain('切换到表单修正')
    expect(legacyConfigMarkup).toContain('打开 AI 设置')
    expect(legacyConfigMarkup).not.toContain('切换到表单修正')
    expect(schemaMarkup).toContain('切换到表单修正')
  })

  it('待补充状态展示已识别平台、地区、语言、实际检索词和缺失字段', () => {
    const markup = render({
      phase: 'needs_review',
      taskId: 'task-needs-review',
      intentText: '用中文查找英国 TikTok 宠物用品账号',
      finishedAt: '2026-07-20T08:00:17Z',
      problem: {
        code: 'VALIDATION_ERROR',
        stage: 'validating_intent',
        message: '解析完成，需要补充预算',
        retryable: false,
        safeDetails: {
          issues: ['缺少预算上限'],
          missing_fields: ['budget_limit_micros'],
          intent: {
            platform: 'tiktok',
            region_code: 'GB',
            query_locale: 'en-GB',
            source_input: 'pet supplies',
          },
        },
      },
      draftPreserved: true,
    })

    expect(markup).toContain('已识别需求')
    expect(markup).toContain('<dt>平台</dt><dd>TikTok</dd>')
    expect(markup).toContain('<dt>国家地区</dt><dd>GB</dd>')
    expect(markup).toContain('<dt>目标检索语言</dt><dd>en-GB</dd>')
    expect(markup).toContain('<dt>实际检索词</dt><dd>pet supplies</dd>')
    expect(markup).toContain('待补充字段')
    expect(markup).toContain('预算上限')
  })

  it('临时模型请求错误和旧版临时协议记录都提供重新解析', () => {
    const base = {
      phase: 'failed' as const,
      intentText: '查找英国 TikTok 宠物账号',
      draftPreserved: true,
    }
    const requestMarkup = render({
      ...base,
      problem: {
        code: 'MODEL_REQUEST_ERROR',
        stage: 'requesting_ai',
        message: 'AI 服务请求超时',
        retryable: true,
        safeDetails: { transport_kind: 'timeout' },
      },
    })
    const legacyMarkup = render({
      ...base,
      problem: {
        code: 'MODEL_PROTOCOL_ERROR',
        stage: 'requesting_ai',
        message: 'AI 服务暂时不可用，请稍后重试',
        retryable: true,
        safeDetails: { http_status: '503' },
      },
    })

    for (const markup of [requestMarkup, legacyMarkup]) {
      expect(markup).toContain('重新解析')
      expect(markup).not.toContain('打开 AI 设置')
      expect(markup).toContain('保留当前输入')
    }
  })

  it('成功状态明确提示在计划预览确认目标与步骤', () => {
    const markup = render({
      phase: 'success',
      intentText: '测试',
      finishedAt: '2026-07-20T08:00:17Z',
      draftPreserved: true,
    })

    expect(markup).toContain('安全计划已生成')
    expect(markup).toContain('实际检索词与后端生成步骤')
    expect(markup).not.toContain('role="alert"')
  })
})
