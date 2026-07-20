import { describe, expect, it } from 'vitest'
import { normalizeBackendProblem } from './backend-problem'

describe('BackendProblem 规范化', () => {
  it('保留结构化中文错误、错误码、阶段和重试状态', () => {
    expect(normalizeBackendProblem({
      code: 'VALIDATION_ERROR',
      stage: 'validation',
      message: '地区和时间范围不能作为“小红书 · 搜索用户”端点的请求参数',
      retryable: false,
      safe_details: { endpoint_key: 'xiaohongshu.user_search' },
    })).toEqual({
      code: 'VALIDATION_ERROR',
      stage: 'validation',
      message: '地区和时间范围不能作为“小红书 · 搜索用户”端点的请求参数',
      retryable: false,
      safeDetails: { endpoint_key: 'xiaohongshu.user_search' },
    })
  })

  it('普通中文 Error 也保留真实消息，不因非 ASCII 字符变成未知错误', () => {
    const problem = normalizeBackendProblem(new Error('AI 服务请求超时'))

    expect(problem.message).toBe('AI 服务请求超时')
    expect(problem.code).toBe('UNCLASSIFIED_ERROR')
    expect(problem.stage).toBe('unknown')
  })

  it('只有完全没有错误详情时才使用未知兜底', () => {
    expect(normalizeBackendProblem({})).toMatchObject({
      code: 'UNKNOWN_ERROR',
      stage: 'unknown',
      message: '未能读取完整错误详情',
      retryable: false,
    })
  })

  it('过滤密钥、认证头、令牌和敏感 URL 查询参数', () => {
    const problem = normalizeBackendProblem({
      code: 'MODEL_AUTH_ERROR',
      stage: 'ai',
      message: 'Authorization: Bearer sk-secret-value，访问 https://ai.example/v1?api_key=top-secret&model=deepseek',
      safe_details: {
        api_key: 'top-secret',
        retry_after: '17',
        endpoint: 'https://ai.example/v1?token=private&model=deepseek',
      },
    })
    const serialized = JSON.stringify(problem)

    expect(serialized).not.toContain('sk-secret-value')
    expect(serialized).not.toContain('top-secret')
    expect(serialized).not.toContain('private')
    expect(problem.safeDetails).toEqual({
      retry_after: '17',
      endpoint: 'https://ai.example/v1?token=%E5%B7%B2%E9%9A%90%E8%97%8F&model=deepseek',
    })
  })
})
