import { describe, expect, it } from 'vitest'
import { backendErrorMessage } from './backend-api'

describe('backendErrorMessage', () => {
  it('保留标准错误的可读消息', () => {
    expect(backendErrorMessage(new Error('后端连接失败'))).toBe('后端连接失败')
  })
})
