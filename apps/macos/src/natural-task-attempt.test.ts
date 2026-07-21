import { beforeEach, describe, expect, it, vi } from 'vitest'

const generateCollectionPlanFromText = vi.hoisted(() => vi.fn())

vi.mock('./backend-api', async (importOriginal) => ({
  ...await importOriginal<typeof import('./backend-api')>(),
  generateCollectionPlanFromText,
}))

import {
  describeNaturalParseFailure,
  parseNaturalTaskAttempt,
} from './natural-task-attempt'

describe('自然语言待修正结果', () => {
  beforeEach(() => generateCollectionPlanFromText.mockReset())

  it('保留已识别意图和缺失字段供输入区立即展示', async () => {
    generateCollectionPlanFromText.mockResolvedValue({
      parsed_intent: {
        schema_version: 1,
        platform: 'tiktok',
        account_source: 'user_search',
        source_input: 'pet supplies',
        query_locale: 'en-GB',
        region_code: 'GB',
        selected_fields: [],
        time_range_days: null,
        age_range: null,
        gender_filter: null,
        record_limit: 10,
        budget_limit_micros: null,
        missing_fields: ['budget_limit_micros'],
        confidence: 0.91,
      },
      issues: ['缺少预算上限'],
      collection_plan: null,
    })

    const error = await parseNaturalTaskAttempt(
      'task-needs-review',
      '用中文查找英国 TikTok 宠物用品账号',
    ).catch((failure: unknown) => failure)
    const failure = describeNaturalParseFailure(error)

    expect(failure.phase).toBe('needs_review')
    expect(failure.taskId).toBe('task-needs-review')
    expect(failure.problem.safeDetails).toMatchObject({
      issues: ['缺少预算上限'],
      missing_fields: ['budget_limit_micros'],
      intent: {
        platform: 'tiktok',
        region_code: 'GB',
        query_locale: 'en-GB',
        source_input: 'pet supplies',
      },
    })
  })
})
