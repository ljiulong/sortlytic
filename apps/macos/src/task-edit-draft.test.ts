import { describe, expect, it } from 'vitest'
import type {
  CollectionIntentV1,
  CollectionPlanView,
  CollectionTaskView,
  NaturalParseAttemptView,
} from './backend-api'
import { collectionIntentFromJson, createTaskEditDraft } from './task-edit-draft'

describe('task edit draft mapping', () => {
  it.each([
    { ...intent(), source_input: { keyword: 'object' } },
    { ...intent(), source_input: 42 },
    { ...intent(), source_input: ['array'] },
    { ...intent(), selected_fields: ['country_region', 42] },
    { ...intent(), missing_fields: 'region_code' },
    { ...intent(), confidence: Number.NaN },
    { ...intent(), age_range: { min: 45, max: 21 } },
    { ...intent(), gender_filter: ['female', 'inferred'] },
  ])('rejects malformed legacy AI intent output without unsafe casts', (value) => {
    expect(collectionIntentFromJson(value)).toBeUndefined()
  })

  it('restores every editable field from a v4 plan and parsed intent', () => {
    const draft = createTaskEditDraft(
      task({ source_type: 'natural_language' }),
      plan({
        schema_version: 4,
        platforms: ['tiktok'],
        account_source: 'user_search',
        selected_fields: ['country_region', 'last_posted_at', 'age', 'gender'],
        region: 'GB',
        time_range: '30',
        age_range: { min: 21, max: 45 },
        gender_filter: ['female'],
        record_limit: 10,
        budget_limit: { currency: 'USD', amount_micros: 100_000 },
        steps: [{ params: { keyword: 'pet supplies' } }],
      }),
      attempt({ parse_status: 'valid', intent_text: '用中文查找英国宠物用品账号' }),
      intent({ source_input: 'pet supplies', query_locale: 'en-GB' }),
    )

    expect(draft).toMatchObject({
      taskId: 'task-1',
      name: '宠物园区',
      editorMode: 'form',
      originalIntent: '用中文查找英国宠物用品账号',
      platform: 'tiktok',
      accountSource: 'user_search',
      sourceInput: 'pet supplies',
      queryLocale: 'en-GB',
      regionCode: 'GB',
      timeRangeDays: '30',
      recordLimit: 10,
      budgetLimitMicros: 100_000,
      selectedFields: ['country_region', 'last_posted_at', 'age', 'gender'],
      ageRange: { min: 21, max: 45 },
      genderFilter: ['female'],
      schemaVersion: 4,
    })
  })

  it('opens a failed natural-language task in preserved text mode without requiring a plan', () => {
    const draft = createTaskEditDraft(
      task({ source_type: 'natural_language', status: 'failed' }),
      undefined,
      attempt({
        parse_status: 'failed',
        intent_text: '查找英国 TikTok 宠物用品账号',
        error_code: 'MODEL_AUTH_ERROR',
        error_message: 'AI 配置鉴权失败',
      }),
    )

    expect(draft.editorMode).toBe('natural_language')
    expect(draft.originalIntent).toBe('查找英国 TikTok 宠物用品账号')
    expect(draft.planId).toBeUndefined()
    expect(draft.parseProblem).toEqual({
      code: 'MODEL_AUTH_ERROR',
      message: 'AI 配置鉴权失败',
    })
  })

  it('keeps stale failure history without reopening a successfully revised task as failed', () => {
    const staleFailure = attempt({
      parse_status: 'failed',
      intent_text: '查找英国 TikTok 宠物用品账号',
      error_code: 'MODEL_AUTH_ERROR',
      error_message: 'AI 配置鉴权失败',
      updated_at: '2026-07-20T00:01:00Z',
    })
    const draft = createTaskEditDraft(
      task({
        source_type: 'natural_language',
        status: 'waiting_confirmation',
        updated_at: '2026-07-20T00:02:00Z',
      }),
      plan({ platforms: ['tiktok'], account_source: 'user_search' }),
      staleFailure,
    )

    expect(draft.editorMode).toBe('form')
    expect(draft.originalIntent).toBe('查找英国 TikTok 宠物用品账号')
    expect(draft.parseProblem).toBeUndefined()
  })

  it('keeps a user plan authoritative when a superseded AI candidate arrives later', () => {
    const draft = createTaskEditDraft(
      task({ source_type: 'natural_language', status: 'waiting_confirmation' }),
      plan({
        platforms: ['tiktok'],
        account_source: 'user_search',
        query_locale: 'en-GB',
        region: 'GB',
        time_range: '30',
        record_limit: 10,
        budget_limit: { amount_micros: 100_000 },
        steps: [{ params: { keyword: 'user saved pets' } }],
      }),
      attempt({
        parse_status: 'needs_review',
        updated_at: '2026-07-20T00:03:00Z',
        error_safe_details_json: {
          superseded_by_user_edit: true,
          issues: ['迟到候选不得覆盖用户计划'],
        },
      }),
      intent({
        platform: 'xiaohongshu',
        account_source: 'keyword_search',
        source_input: '迟到模型检索词',
        query_locale: 'zh-CN',
        region_code: 'CN',
        time_range_days: 7,
        record_limit: 99,
        budget_limit_micros: 900_000,
      }),
    )

    expect(draft).toMatchObject({
      platform: 'tiktok',
      accountSource: 'user_search',
      sourceInput: 'user saved pets',
      queryLocale: 'en-GB',
      regionCode: 'GB',
      timeRangeDays: '30',
      recordLimit: 10,
      budgetLimitMicros: 100_000,
    })
    expect(draft.validationIssues).not.toContain('迟到候选不得覆盖用户计划')
    expect(draft.parseProblem).toBeUndefined()
  })

  it('keeps direct identifiers unchanged and supports legacy plan fields', () => {
    const directUrl = 'https://www.tiktok.com/@PetBrandUK'
    const draft = createTaskEditDraft(
      task(),
      plan({
        platforms: ['tiktok'],
        data_types: ['account_profile'],
        region: { value: 'GB' },
        time_range: '7',
        record_limit: 1,
        budget_limit: { amount_micros: 200_000 },
        steps: [{ params: { account_id: directUrl } }],
      }, 2),
    )

    expect(draft.sourceInput).toBe(directUrl)
    expect(draft.regionCode).toBe('GB')
    expect(draft.timeRangeDays).toBe('7')
    expect(draft.schemaVersion).toBe(2)
  })

  it('marks successful tasks for copy editing and preserves validation suggestions', () => {
    const draft = createTaskEditDraft(
      task({ status: 'success' }),
      {
        ...plan({ platforms: ['xiaohongshu'] }),
        validation_status: 'needs_review',
        validation_errors_json: ['参数 region 不在 endpoint 白名单内'],
      },
    )

    expect(draft.copyOnSave).toBe(true)
    expect(draft.validationIssues).toEqual(['参数 region 不在 endpoint 白名单内'])
  })
})

function task(overrides: Partial<CollectionTaskView> = {}): CollectionTaskView {
  return {
    id: 'task-1',
    name: '宠物园区',
    source_type: 'form',
    status: 'failed',
    platforms_json: ['tiktok'],
    data_types_json: ['account'],
    created_at: '2026-07-20T00:00:00Z',
    updated_at: '2026-07-20T00:00:00Z',
    cost_estimate_json: {},
    actual_cost_json: {},
    ...overrides,
  }
}

function plan(planJson: Record<string, unknown>, schemaVersion = 4): CollectionPlanView {
  return {
    id: 'plan-1',
    task_id: 'task-1',
    source: 'user_edited',
    schema_version: schemaVersion,
    plan_json: planJson,
    validation_status: 'valid',
    validation_errors_json: [],
    cost_estimate_json: {},
    confirmed_by_user: false,
    created_at: '2026-07-20T00:00:00Z',
    updated_at: '2026-07-20T00:00:00Z',
  }
}

function attempt(overrides: Partial<NaturalParseAttemptView> = {}): NaturalParseAttemptView {
  return {
    id: 'attempt-1',
    task_id: 'task-1',
    intent_text: '原始需求',
    parse_status: 'failed',
    error_safe_details_json: {},
    created_at: '2026-07-20T00:00:00Z',
    updated_at: '2026-07-20T00:00:00Z',
    ...overrides,
  }
}

function intent(overrides: Partial<CollectionIntentV1> = {}): CollectionIntentV1 {
  return {
    schema_version: 1,
    platform: 'tiktok',
    account_source: 'user_search',
    source_input: 'pet supplies',
    query_locale: 'en-GB',
    region_code: 'GB',
    selected_fields: [],
    time_range_days: 30,
    age_range: null,
    gender_filter: null,
    record_limit: 10,
    budget_limit_micros: 100_000,
    missing_fields: [],
    confidence: 0.9,
    ...overrides,
  }
}
