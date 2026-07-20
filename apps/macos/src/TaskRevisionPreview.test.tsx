import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it } from 'vitest'
import type { NaturalParseAttemptView } from './backend-api'
import TaskRevisionPreview from './TaskRevisionPreview'
import type { TaskEditDraft } from './task-edit-draft'

describe('task revision preview', () => {
  it('shows translated query, deterministic endpoints, schemas and evidence requirements', () => {
    const markup = renderToStaticMarkup(
      <TaskRevisionPreview
        attempt={{
          id: 'attempt-1',
          task_id: 'task-1',
          intent_text: '用中文查找英国 TikTok 宠物用品账号',
          parse_status: 'valid',
          prompt_version_id: 'prompt-v3',
          error_safe_details_json: {},
          created_at: '2026-07-20T00:00:00Z',
          updated_at: '2026-07-20T00:00:00Z',
        } satisfies NaturalParseAttemptView}
        draft={{
          taskId: 'task-1',
          planId: 'plan-2',
          name: '英国宠物账号',
          sourceType: 'natural_language',
          editorMode: 'form',
          originalIntent: '用中文查找英国 TikTok 宠物用品账号',
          platform: 'tiktok',
          accountSource: 'user_search',
          sourceInput: 'pet supplies',
          queryLocale: 'en-GB',
          regionCode: 'GB',
          timeRangeDays: '30',
          recordLimit: 10,
          budgetLimitMicros: 100_000,
          selectedFields: ['country_region', 'last_posted_at'],
          genderFilter: [],
          schemaVersion: 4,
          planJson: {
            schema_version: 4,
            steps: [
              { operation_key: 'discover.user_search', endpoint_key: 'tiktok.user_search' },
              { operation_key: 'enrich.account_country', endpoint_key: 'tiktok.account_country' },
            ],
          },
          validationIssues: [],
          missingFields: [],
          copyOnSave: false,
        } satisfies TaskEditDraft}
      />,
    )

    for (const expected of [
      'GB',
      'en-GB',
      'pet supplies',
      'tiktok.user_search',
      'tiktok.account_country',
      'prompt-v3',
      'collection_intent_v1',
      'collection_plan_v4',
      'country_region',
      'last_posted_at',
    ]) expect(markup).toContain(expected)
  })

  it('does not invent endpoints when a failed parse has no plan', () => {
    const markup = renderToStaticMarkup(<TaskRevisionPreview draft={{
      taskId: 'task-1',
      name: '失败草稿',
      sourceType: 'natural_language',
      editorMode: 'natural_language',
      originalIntent: '原始需求',
      platform: '',
      accountSource: '',
      sourceInput: '',
      queryLocale: '',
      regionCode: '',
      timeRangeDays: '',
      selectedFields: [],
      genderFilter: [],
      validationIssues: [],
      missingFields: [],
      copyOnSave: false,
    }} />)

    expect(markup).toContain('尚未生成可预览的安全计划')
    expect(markup).not.toContain('endpoint_key')
  })
})
