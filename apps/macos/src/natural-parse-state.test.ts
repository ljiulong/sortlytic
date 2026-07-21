import { describe, expect, it } from 'vitest'
import {
  createIdleNaturalParseState,
  createPreparingNaturalParseState,
  naturalParseStateFromAttempt,
  resolveNaturalParseState,
} from './natural-parse-state'
import type { NaturalParseAttemptView } from './backend-api'

const attempt: NaturalParseAttemptView = {
  id: 'attempt-1',
  task_id: 'task-1',
  intent_text: '用中文查找英国 TikTok 宠物用品账号',
  language: 'zh-CN',
  parse_status: 'running',
  parse_phase: 'requesting_ai',
  ai_run_id: null,
  error_code: null,
  error_message: null,
  retryable: null,
  error_safe_details_json: {},
  provider_id: 'deepseek-profile',
  model_id: 'deepseek-v4-flash',
  prompt_version_id: 'prompt-v5',
  created_at: '2026-07-20T08:00:00Z',
  updated_at: '2026-07-20T08:00:10Z',
}

describe('NaturalParseState', () => {
  it('提交后立即进入 preparing 并保留原始输入', () => {
    const state = createPreparingNaturalParseState('  用中文查找英国 TikTok 宠物用品账号  ')

    expect(state.phase).toBe('preparing')
    expect(state.intentText).toBe('用中文查找英国 TikTok 宠物用品账号')
    expect(state.draftPreserved).toBe(true)
    expect(state.startedAt).toBeTruthy()
  })

  it('从持久化运行记录恢复等待模型阶段和模型信息', () => {
    expect(naturalParseStateFromAttempt(attempt)).toMatchObject({
      phase: 'requesting_ai',
      taskId: 'task-1',
      attemptId: 'attempt-1',
      providerId: 'deepseek-profile',
      modelId: 'deepseek-v4-flash',
      promptVersionId: 'prompt-v5',
      draftPreserved: true,
    })
  })

  it('失败状态保留真实中文错误、码、阶段、重试信息和安全详情', () => {
    const state = naturalParseStateFromAttempt({
      ...attempt,
      parse_status: 'failed',
      parse_phase: 'requesting_ai',
      error_code: 'MODEL_RATE_LIMIT',
      error_message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
      retryable: true,
      error_safe_details_json: { retry_after: '17' },
      updated_at: '2026-07-20T08:00:17Z',
    })

    expect(state.phase).toBe('failed')
    expect(state.finishedAt).toBe('2026-07-20T08:00:17Z')
    expect(state.problem).toEqual({
      code: 'MODEL_RATE_LIMIT',
      stage: 'requesting_ai',
      message: 'AI 服务请求过于频繁或额度不足，请稍后重试',
      retryable: true,
      safeDetails: { retry_after: '17' },
    })
  })

  it('业务不完整恢复为 needs_review，中断恢复为可重试 failed', () => {
    const needsReview = naturalParseStateFromAttempt({
      ...attempt,
      parse_status: 'needs_review',
      parse_phase: 'needs_review',
    })
    const interrupted = naturalParseStateFromAttempt({
      ...attempt,
      parse_status: 'interrupted',
      parse_phase: 'requesting_ai',
    })

    expect(needsReview.phase).toBe('needs_review')
    expect(needsReview.problem?.message).toContain('需要补充信息')
    expect(interrupted.phase).toBe('failed')
    expect(interrupted.problem).toMatchObject({
      code: 'MODEL_REQUEST_INTERRUPTED',
      retryable: true,
    })
  })

  it('有效记录恢复为 success 并保留完成时间', () => {
    const state = naturalParseStateFromAttempt({
      ...attempt,
      parse_status: 'valid',
      parse_phase: 'success',
    })

    expect(state.phase).toBe('success')
    expect(state.finishedAt).toBe(attempt.updated_at)
    expect(state.problem).toBeUndefined()
  })

  it('应用启动时没有计划预览则不把历史有效记录恢复为孤立成功反馈', () => {
    const state = resolveNaturalParseState(createIdleNaturalParseState(), [{
      ...attempt,
      parse_status: 'valid',
      parse_phase: 'success',
    }])

    expect(state).toEqual(createIdleNaturalParseState())
  })

  it('应用启动时仍恢复最近失败记录的可操作反馈', () => {
    const state = resolveNaturalParseState(createIdleNaturalParseState(), [{
      ...attempt,
      parse_status: 'failed',
      parse_phase: 'requesting_ai',
      error_code: 'MODEL_REQUEST_ERROR',
      error_message: 'AI 服务请求超时',
      retryable: true,
    }])

    expect(state).toMatchObject({
      phase: 'failed',
      taskId: 'task-1',
      problem: {
        code: 'MODEL_REQUEST_ERROR',
        message: 'AI 服务请求超时',
        retryable: true,
      },
    })
  })
})
