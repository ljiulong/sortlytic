import { useEffect, useMemo, useState } from 'react'
import { ArrowLeft, History, RefreshCcw, Save } from 'lucide-react'
import AppSelect from './AppSelect'
import {
  backendErrorMessage,
  generateAccountCollectionPlan,
  getAccountCollectionCapabilities,
  getAiRun,
  getLatestCollectionPlan,
  getTask,
  listTaskIntents,
  reviseCollectionTask,
  type AccountCollectionCapabilityView,
  type CollectionIntentV1,
  type NaturalParseAttemptView,
  type RevisedCollectionTaskView,
} from './backend-api'
import { accountSourceFilterCapabilities, sourceInputCopy } from './account-source-rules'
import { countryRegionSelectOptions } from './collection-select-options'
import { createTaskEditDraft, type TaskEditDraft } from './task-edit-draft'
import TaskRevisionPreview from './TaskRevisionPreview'

type TaskEditorProps = {
  taskId: string
  naturalParseAttempt?: NaturalParseAttemptView
  isBusy: boolean
  onCancel: () => void
  onRetryNaturalTask?: (taskId: string, intentText: string) => Promise<unknown>
  onSaved: (result: RevisedCollectionTaskView) => void
}

const platformOptions = [
  { value: 'tiktok', label: 'TikTok' },
  { value: 'douyin', label: '抖音' },
  { value: 'xiaohongshu', label: '小红书' },
]
const genderOptions = [
  { value: 'female', label: '女性' },
  { value: 'male', label: '男性' },
  { value: 'other', label: '其他' },
] as const

function TaskEditor({
  taskId,
  naturalParseAttempt,
  isBusy,
  onCancel,
  onRetryNaturalTask,
  onSaved,
}: TaskEditorProps) {
  const [draft, setDraft] = useState<TaskEditDraft>()
  const [capability, setCapability] = useState<AccountCollectionCapabilityView>()
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [history, setHistory] = useState<NaturalParseAttemptView[]>()

  useEffect(() => {
    let current = true
    setLoading(true)
    setError('')
    void loadDraft(taskId, naturalParseAttempt)
      .then((nextDraft) => {
        if (!current) return
        setDraft(nextDraft)
        setLoading(false)
      })
      .catch((loadError) => {
        if (!current) return
        setError(backendErrorMessage(loadError))
        setLoading(false)
      })
    return () => {
      current = false
    }
  }, [naturalParseAttempt, taskId])

  useEffect(() => {
    let current = true
    setCapability(undefined)
    if (!draft?.platform) return () => {
      current = false
    }
    void getAccountCollectionCapabilities(draft.platform)
      .then((nextCapability) => {
        if (current) setCapability(nextCapability)
      })
      .catch((capabilityError) => {
        if (current) setError(backendErrorMessage(capabilityError))
      })
    return () => {
      current = false
    }
  }, [draft?.platform])

  const selectedSource = capability?.account_sources.find(
    (source) => source.key === draft?.accountSource,
  )
  const filterCapabilities = useMemo(
    () => accountSourceFilterCapabilities(capability, draft?.accountSource),
    [capability, draft?.accountSource],
  )
  const regionUnsupported = filterCapabilities.regionFilter === 'unsupported'
  const timeUnsupported = filterCapabilities.timeRangeFilter === 'unsupported'
  const timeOptions = filterCapabilities.timeRanges.map((days) => ({
    value: days,
    label: `近 ${days} 天`,
    meta: `${days}d`,
  }))
  const supportedFieldKeys = new Set(
    capability?.fields
      .filter((field) => field.availability !== 'unsupported')
      .map((field) => field.key) ?? [],
  )
  const unsupportedSelectedFields = draft?.selectedFields.filter(
    (field) => !supportedFieldKeys.has(field),
  ) ?? []
  const updateDraft = (patch: Partial<TaskEditDraft>) => {
    setDraft((current) => current ? { ...current, ...patch } : current)
    setError('')
    setNotice('')
  }

  const saveRevision = async () => {
    if (!draft || !capability || !selectedSource) return
    const validationError = validateDraft(
      draft,
      regionUnsupported,
      timeUnsupported,
      unsupportedSelectedFields,
    )
    if (validationError) {
      setError(validationError)
      return
    }
    setSaving(true)
    setError('')
    setNotice('')
    try {
      const single = selectedSource.pagination_mode === 'single'
      const recordLimit = single ? 1 : draft.recordLimit as number
      const selectedFields = new Set(draft.selectedFields)
      if (draft.ageRange) selectedFields.add('age')
      if (draft.genderFilter.length > 0) selectedFields.add('gender')
      const params: Record<string, unknown> = {
        [sourceInputParam(selectedSource.input_kind)]: draft.sourceInput.trim(),
      }
      if (draft.regionCode) params.region = draft.regionCode
      if (draft.timeRangeDays) params.time_range = draft.timeRangeDays
      const generated = await generateAccountCollectionPlan({
        platform: draft.platform,
        account_source: draft.accountSource,
        selected_fields: [...selectedFields],
        enrichment_policy: 'auto_costed',
        params,
        age_range: draft.ageRange ?? null,
        gender_filter: draft.genderFilter.length > 0 ? draft.genderFilter : null,
        request_limit: single
          ? 1
          : Math.min(
              selectedSource.max_request_count,
              Math.max(1, Math.ceil(recordLimit / selectedSource.max_page_size)),
            ),
        record_limit: recordLimit,
        budget_limit_micros: draft.budgetLimitMicros,
      })
      const planJson = {
        ...generated.plan_json,
        ...(draft.queryLocale.trim() ? { query_locale: draft.queryLocale.trim() } : {}),
      }
      const result = await reviseCollectionTask({
        task_id: draft.taskId,
        name: draft.name.trim(),
        platforms: [draft.platform],
        data_types: ['account'],
        source: 'user_edited',
        plan_json: planJson,
      })
      setDraft(createTaskEditDraft(
        result.task,
        result.collection_plan,
        naturalParseAttempt,
        intentFromDraft(draft),
      ))
      setNotice(result.copied_from_task_id
        ? '已复制成功任务并保存为新的待确认任务，原任务和运行记录保持不变。'
        : '已保存新的计划版本，旧计划、运行和错误日志保持不变。')
      onSaved(result)
    } catch (saveError) {
      setError(backendErrorMessage(saveError))
    } finally {
      setSaving(false)
    }
  }

  const retryNatural = async () => {
    if (!draft?.originalIntent.trim() || !onRetryNaturalTask) return
    setSaving(true)
    setError('')
    try {
      await onRetryNaturalTask(draft.taskId, draft.originalIntent.trim())
      setNotice('重新解析已完成，原失败记录继续保留。')
    } catch (retryError) {
      setError(backendErrorMessage(retryError))
    } finally {
      setSaving(false)
    }
  }

  const loadHistory = async () => {
    try {
      setHistory(await listTaskIntents(taskId))
    } catch (historyError) {
      setError(backendErrorMessage(historyError))
    }
  }

  if (loading) return <section className="task-editor" role="status">正在读取任务和最新计划…</section>
  if (!draft) {
    return (
      <section className="task-editor" role="alert">
        <h2>任务无法编辑</h2>
        <p>{error || '未能读取任务详情'}</p>
        <button className="ghost-button" type="button" onClick={onCancel}>返回任务</button>
      </section>
    )
  }

  return (
    <section className="task-editor" aria-labelledby="task-editor-heading">
      <header className="task-editor__heading">
        <button className="ghost-button" type="button" onClick={onCancel}>
          <ArrowLeft size={15} aria-hidden="true" />
          返回任务
        </button>
        <div>
          <p className="eyebrow">任务计划修订</p>
          <h2 id="task-editor-heading">编辑任务</h2>
          <p>保存会生成新的计划版本；旧运行、错误、AI 快照和导出不会删除。</p>
        </div>
        <span className="status-pill" data-tone="warning">
          {draft.copyOnSave ? '复制并编辑' : '版本化编辑'}
        </span>
      </header>

      {draft.sourceType === 'natural_language' ? (
        <div className="task-editor__mode-bar" role="tablist" aria-label="编辑方式">
          <button
            aria-selected={draft.editorMode === 'natural_language'}
            className="ghost-button"
            role="tab"
            type="button"
            onClick={() => updateDraft({ editorMode: 'natural_language' })}
          >自然语言</button>
          <button
            aria-selected={draft.editorMode === 'form'}
            className="ghost-button"
            role="tab"
            type="button"
            onClick={() => updateDraft({ editorMode: 'form' })}
          >结构化表单</button>
        </div>
      ) : null}

      {draft.editorMode === 'natural_language' ? (
        <div className="task-editor__natural-panel">
          <label htmlFor="task-editor-natural-input">原始自然语言需求</label>
          <textarea
            id="task-editor-natural-input"
            rows={6}
            value={draft.originalIntent}
            onChange={(event) => updateDraft({ originalIntent: event.target.value })}
          />
          {draft.parseProblem ? (
            <div className="task-editor__problem" role="alert">
              <strong>上次解析失败</strong>
              <p>{draft.parseProblem.message ?? '未能读取完整错误详情'}</p>
              <code>{draft.parseProblem.code ?? 'UNKNOWN_PARSE_ERROR'}</code>
              <p>修改方式：修正上方需求后重新解析，或切换到结构化表单补齐字段。</p>
            </div>
          ) : null}
          <div className="task-editor__actions">
            <button
              className="primary-button"
              disabled={isBusy || saving || !draft.originalIntent.trim()}
              type="button"
              onClick={() => void retryNatural()}
            >
              <RefreshCcw size={15} aria-hidden="true" />
              重新解析
            </button>
            <button className="ghost-button" type="button" onClick={() => updateDraft({ editorMode: 'form' })}>
              切换到表单修正
            </button>
          </div>
        </div>
      ) : (
        <div className="task-editor__form">
          <EditorField label="任务名称" htmlFor="task-editor-name">
            <input
              id="task-editor-name"
              maxLength={80}
              value={draft.name}
              onChange={(event) => updateDraft({ name: event.target.value })}
            />
          </EditorField>

          <div className="task-editor__grid">
            <EditorField label="平台" htmlFor="task-editor-platform">
              <AppSelect
                id="task-editor-platform"
                value={draft.platform}
                options={platformOptions}
                placeholder="选择平台"
                onChange={(platform) => updateDraft({
                  platform,
                  accountSource: '',
                  selectedFields: [],
                  regionCode: '',
                  timeRangeDays: '',
                })}
              />
            </EditorField>
            <EditorField label="账号来源" htmlFor="task-editor-account-source">
              <AppSelect
                id="task-editor-account-source"
                value={draft.accountSource}
                options={capability?.account_sources.map((source) => ({
                  value: source.key,
                  label: source.display_name,
                  description: source.description,
                })) ?? []}
                placeholder={capability ? '选择账号来源' : '正在读取来源能力'}
                disabled={!capability}
                onChange={(accountSource) => updateDraft({
                  accountSource,
                  sourceInput: '',
                  queryLocale: '',
                  regionCode: '',
                  timeRangeDays: '',
                })}
              />
            </EditorField>
          </div>

          <div className="task-editor__grid">
            <EditorField
              label={sourceInputCopy(selectedSource).label}
              htmlFor="task-editor-source-input"
            >
              <input
                id="task-editor-source-input"
                value={draft.sourceInput}
                placeholder={sourceInputCopy(selectedSource).placeholder}
                onChange={(event) => updateDraft({ sourceInput: event.target.value })}
              />
            </EditorField>
            <EditorField
              label="目标检索语言"
              htmlFor="task-editor-query-locale"
              hint={selectedSource?.input_kind === 'keyword'
                ? '关键词来源使用 language-REGION，例如 en-GB；实际检索词可在左侧修改。'
                : '账号 ID、作品 ID、URL 和分享链接不会翻译。'}
            >
              <input
                id="task-editor-query-locale"
                value={draft.queryLocale}
                disabled={Boolean(selectedSource && selectedSource.input_kind !== 'keyword')}
                placeholder="例如 en-GB"
                onChange={(event) => updateDraft({ queryLocale: event.target.value })}
              />
            </EditorField>
          </div>

          <div className="task-editor__grid">
            <EditorField
              label="国家地区"
              htmlFor="task-editor-region"
              hint={filterHint(filterCapabilities.regionFilter, '地区')}
            >
              <AppSelect
                id="task-editor-region"
                value={draft.regionCode}
                options={countryRegionSelectOptions}
                placeholder={regionUnsupported ? '当前来源不支持' : '选择国家地区'}
                disabled={!selectedSource || regionUnsupported}
                searchable
                searchPlaceholder="按中文名、英文名或代码搜索"
                onChange={(regionCode) => updateDraft({ regionCode })}
              />
              {regionUnsupported && draft.regionCode ? (
                <UnsupportedField
                  message="当前平台或来源无法可靠筛选地区，旧条件不能进入新计划。"
                  action="移除地区条件"
                  onRemove={() => updateDraft({ regionCode: '' })}
                />
              ) : null}
            </EditorField>
            <EditorField
              label="时间范围"
              htmlFor="task-editor-time-range"
              hint={filterHint(filterCapabilities.timeRangeFilter, '时间')}
            >
              <AppSelect
                id="task-editor-time-range"
                value={draft.timeRangeDays}
                options={timeOptions}
                placeholder={timeUnsupported ? '当前来源不支持' : '选择时间范围'}
                disabled={!selectedSource || timeUnsupported}
                onChange={(timeRangeDays) => updateDraft({ timeRangeDays })}
              />
              {timeUnsupported && draft.timeRangeDays ? (
                <UnsupportedField
                  message="当前平台或来源无法可靠筛选时间，旧条件不能进入新计划。"
                  action="移除时间条件"
                  onRemove={() => updateDraft({ timeRangeDays: '' })}
                />
              ) : null}
            </EditorField>
          </div>

          <div className="task-editor__grid">
            <EditorField label="最大记录数" htmlFor="task-editor-record-limit">
              <input
                id="task-editor-record-limit"
                min={1}
                type="number"
                value={draft.recordLimit ?? ''}
                onChange={(event) => updateDraft({ recordLimit: numberOrUndefined(event.target.value) })}
              />
            </EditorField>
            <EditorField label="预算上限（美元）" htmlFor="task-editor-budget">
              <input
                id="task-editor-budget"
                min={0.1}
                step={0.1}
                type="number"
                value={draft.budgetLimitMicros ? draft.budgetLimitMicros / 1_000_000 : ''}
                onChange={(event) => updateDraft({
                  budgetLimitMicros: dollarsToMicros(event.target.value),
                })}
              />
            </EditorField>
          </div>

          <section className="task-editor__field-picker" aria-labelledby="task-editor-fields">
            <h3 id="task-editor-fields">结果字段</h3>
            <div>
              {unsupportedSelectedFields.map((fieldKey) => {
                const field = capability?.fields.find((candidate) => candidate.key === fieldKey)
                const label = field ? `${field.display_name}（${fieldKey}）` : fieldKey
                const reason = field?.missing_reason ?? '该字段已从当前平台能力目录移除'
                return (
                  <UnsupportedField
                    key={fieldKey}
                    message={`旧结果字段 ${label} 当前不可用：${reason}`}
                    action={`移除字段 ${fieldKey}`}
                    onRemove={() => updateDraft({
                      selectedFields: draft.selectedFields.filter((value) => value !== fieldKey),
                    })}
                  />
                )
              })}
              {capability?.fields.filter((field) => field.availability !== 'unsupported').map((field) => (
                <label key={field.key}>
                  <input
                    type="checkbox"
                    checked={draft.selectedFields.includes(field.key)}
                    onChange={() => updateDraft({
                      selectedFields: toggleValue(draft.selectedFields, field.key),
                    })}
                  />
                  <span>{field.display_name}</span>
                </label>
              ))}
            </div>
          </section>

          <div className="task-editor__grid">
            <fieldset>
              <legend>年龄范围</legend>
              <label>
                <input
                  type="checkbox"
                  checked={Boolean(draft.ageRange)}
                  onChange={(event) => updateDraft({
                    ageRange: event.target.checked ? { min: 18, max: 65 } : undefined,
                  })}
                />
                启用明确年龄闭区间
              </label>
              {draft.ageRange ? (
                <div className="task-editor__inline-inputs">
                  <input
                    aria-label="最小年龄"
                    min={0}
                    max={130}
                    type="number"
                    value={draft.ageRange.min}
                    onChange={(event) => updateDraft({
                      ageRange: { ...draft.ageRange!, min: Number(event.target.value) },
                    })}
                  />
                  <span>至</span>
                  <input
                    aria-label="最大年龄"
                    min={0}
                    max={130}
                    type="number"
                    value={draft.ageRange.max}
                    onChange={(event) => updateDraft({
                      ageRange: { ...draft.ageRange!, max: Number(event.target.value) },
                    })}
                  />
                </div>
              ) : null}
            </fieldset>
            <fieldset>
              <legend>性别筛选</legend>
              {genderOptions.map((gender) => (
                <label key={gender.value}>
                  <input
                    type="checkbox"
                    checked={draft.genderFilter.includes(gender.value)}
                    onChange={() => updateDraft({
                      genderFilter: toggleValue(draft.genderFilter, gender.value),
                    })}
                  />
                  {gender.label}
                </label>
              ))}
            </fieldset>
          </div>

          {draft.validationIssues.length > 0 || draft.missingFields.length > 0 ? (
            <div className="task-editor__problem" role="alert">
              <strong>旧计划需要修正</strong>
              <ul>
                {[...draft.validationIssues, ...draft.missingFields.map((field) => `缺少字段：${field}`)]
                  .map((issue) => <li key={issue}>{issue}</li>)}
              </ul>
            </div>
          ) : null}
        </div>
      )}

      <TaskRevisionPreview draft={draft} attempt={naturalParseAttempt} />

      <section className="task-editor__audit" aria-labelledby="task-editor-audit-heading">
        <div>
          <h3 id="task-editor-audit-heading">计划与解析审计</h3>
          <button className="ghost-button" type="button" onClick={() => void loadHistory()}>
            <History size={15} aria-hidden="true" />
            查看解析记录
          </button>
        </div>
        <dl>
          <div><dt>AI 模型</dt><dd>{naturalParseAttempt?.model_id ?? '不适用'}</dd></div>
          <div><dt>提示词版本</dt><dd>{naturalParseAttempt?.prompt_version_id ?? '不适用'}</dd></div>
          <div><dt>意图 Schema</dt><dd>{naturalParseAttempt ? 'collection_intent_v1' : '不适用'}</dd></div>
          <div><dt>最终计划 Schema</dt><dd>{draft.schemaVersion ? `collection_plan_v${draft.schemaVersion}` : '尚未生成'}</dd></div>
        </dl>
        {history ? (
          history.length > 0 ? (
            <ol className="task-editor__history">
              {history.map((run) => (
                <li key={run.id}>
                  <strong>{run.parse_status} · {run.error_code ?? 'NO_ERROR'}</strong>
                  <span>{run.created_at}</span>
                  <span>{run.error_message ?? '结构化意图已保存'}</span>
                </li>
              ))}
            </ol>
          ) : <p role="status">当前任务没有可显示的 AI 解析运行。</p>
        ) : null}
      </section>

      {error ? <p className="task-editor__error" role="alert">{error}</p> : null}
      {notice ? <p className="task-editor__notice" role="status">{notice}</p> : null}
      {draft.editorMode === 'form' ? (
        <footer className="task-editor__footer">
          <p>保存前由后端重新生成步骤、校验端点白名单并重算请求数和成本。</p>
          <div>
            <button className="ghost-button" disabled={saving} type="button" onClick={onCancel}>
              取消编辑
            </button>
            <button
              className="primary-button"
              disabled={isBusy || saving || !capability}
              type="button"
              onClick={() => void saveRevision()}
            >
              <Save size={15} aria-hidden="true" />
              保存新计划版本
            </button>
          </div>
        </footer>
      ) : null}
    </section>
  )
}

async function loadDraft(taskId: string, attempt?: NaturalParseAttemptView) {
  const task = await getTask(taskId)
  let plan
  try {
    plan = await getLatestCollectionPlan(taskId)
  } catch (error) {
    if (task.source_type !== 'natural_language') throw error
  }
  let intent: CollectionIntentV1 | undefined
  if (attempt?.ai_run_id) {
    const aiRun = await getAiRun(attempt.ai_run_id).catch(() => undefined)
    intent = collectionIntentFromJson(aiRun?.output_json)
  }
  return createTaskEditDraft(task, plan, attempt, intent)
}

function validateDraft(
  draft: TaskEditDraft,
  regionUnsupported: boolean,
  timeUnsupported: boolean,
  unsupportedSelectedFields: string[],
) {
  if (draft.name.trim().length < 2) return '任务名称至少需要 2 个字符'
  if (!draft.platform) return '请选择平台'
  if (!draft.accountSource) return '请选择账号来源'
  if (!draft.sourceInput.trim()) return '请填写当前账号来源需要的检索词、账号或作品信息'
  if (regionUnsupported && draft.regionCode) return '请先移除当前来源不支持的地区条件，或更换平台和来源'
  if (timeUnsupported && draft.timeRangeDays) return '请先移除当前来源不支持的时间条件，或更换平台和来源'
  if (unsupportedSelectedFields.length > 0) {
    return `请先移除当前不支持的旧结果字段：${unsupportedSelectedFields.join('、')}`
  }
  if (!regionUnsupported && !draft.regionCode) return '请选择国家地区'
  if (!draft.recordLimit || draft.recordLimit < 1) return '最大记录数必须大于 0'
  if (!draft.budgetLimitMicros || draft.budgetLimitMicros < 100_000) return '预算至少为 0.1 美元'
  if (draft.ageRange && (draft.ageRange.min < 0
    || draft.ageRange.max > 130
    || draft.ageRange.min > draft.ageRange.max)) return '年龄范围必须是 0–130 之间的有效闭区间'
  return ''
}

function collectionIntentFromJson(value: unknown): CollectionIntentV1 | undefined {
  if (!isRecord(value) || value.schema_version !== 1 || !Array.isArray(value.selected_fields)) {
    return undefined
  }
  return value as CollectionIntentV1
}

function intentFromDraft(draft: TaskEditDraft): CollectionIntentV1 {
  return {
    schema_version: 1,
    platform: platformValue(draft.platform),
    account_source: draft.accountSource || null,
    source_input: draft.sourceInput || null,
    query_locale: draft.queryLocale || null,
    region_code: draft.regionCode || null,
    selected_fields: draft.selectedFields,
    time_range_days: timeRangeValue(draft.timeRangeDays),
    age_range: draft.ageRange ?? null,
    gender_filter: draft.genderFilter.length > 0 ? draft.genderFilter : null,
    record_limit: draft.recordLimit ?? null,
    budget_limit_micros: draft.budgetLimitMicros ?? null,
    missing_fields: [],
    confidence: 1,
  }
}

function sourceInputParam(inputKind: 'keyword' | 'account' | 'item') {
  if (inputKind === 'keyword') return 'keyword'
  if (inputKind === 'item') return 'item_id'
  return 'account_id'
}

function filterHint(execution: string, field: string) {
  if (execution === 'provider') return `由平台接口直接筛选${field}`
  if (execution === 'local') return `采集并合并账号后按明确公开值筛选${field}`
  return `当前平台或来源无法可靠筛选${field}`
}

function numberOrUndefined(value: string) {
  const number = Number(value)
  return Number.isFinite(number) && number > 0 ? Math.floor(number) : undefined
}

function dollarsToMicros(value: string) {
  const number = Number(value)
  return Number.isFinite(number) && number > 0 ? Math.round(number * 1_000_000) : undefined
}

function toggleValue<T extends string>(values: T[], value: T) {
  return values.includes(value) ? values.filter((item) => item !== value) : [...values, value]
}

function platformValue(value: string): CollectionIntentV1['platform'] {
  return ['tiktok', 'douyin', 'xiaohongshu'].includes(value)
    ? value as CollectionIntentV1['platform']
    : null
}

function timeRangeValue(value: string): CollectionIntentV1['time_range_days'] {
  const days = Number(value)
  return [1, 7, 30, 180].includes(days) ? days as 1 | 7 | 30 | 180 : null
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

function EditorField({
  children,
  htmlFor,
  label,
  hint,
}: {
  children: React.ReactNode
  htmlFor: string
  label: string
  hint?: string
}) {
  return (
    <div className="task-editor__field">
      <label htmlFor={htmlFor}>{label}</label>
      {children}
      {hint ? <small>{hint}</small> : null}
    </div>
  )
}

function UnsupportedField({
  action,
  message,
  onRemove,
}: {
  action: string
  message: string
  onRemove: () => void
}) {
  return (
    <div className="task-editor__unsupported" role="alert">
      <p>{message}</p>
      <button className="ghost-button" type="button" onClick={onRemove}>{action}</button>
    </div>
  )
}

export default TaskEditor
