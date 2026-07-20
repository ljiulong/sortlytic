import { describe, expect, it } from 'vitest'
import { mapTaskRow, toUiTaskStatus } from './workbench-task-mapper'

describe('workbench task status mapping', () => {
  it('将 Schema v4 account 任务显示为账号数据而不是评论用户', () => {
    expect(mapTaskRow({
      id: 'task-account-v4',
      name: '账号任务',
      source_type: 'form',
      status: 'waiting_confirmation',
      platforms_json: ['tiktok'],
      data_types_json: ['account'],
      created_at: '2026-07-20T00:00:00Z',
      updated_at: '2026-07-20T00:01:00Z',
      cancelled_at: null,
      cost_estimate_json: { request_count_estimate: 2 },
      actual_cost_json: {},
    })).toMatchObject({
      cost: '预计 2 次请求 · 账号数据',
      dataTypeCode: 'account',
    })
  })

  it('将已取消任务保留为独立终态，不误报为失败', () => {
    expect(toUiTaskStatus('cancelled')).toBe('已取消')
    expect(mapTaskRow({
      id: 'task-cancelled',
      name: '已取消任务',
      source_type: 'form',
      status: 'cancelled',
      platforms_json: ['tiktok'],
      data_types_json: ['keyword_search'],
      created_at: '2026-07-17T00:00:00Z',
      updated_at: '2026-07-17T00:01:00Z',
      cancelled_at: '2026-07-17T00:01:00Z',
      cost_estimate_json: {},
      actual_cost_json: {},
    })).toMatchObject({
      id: 'task-cancelled',
      status: '已取消',
      sourceType: 'form',
      requestCount: 0,
      dataTypeCode: 'keyword_search',
    })
  })
})
