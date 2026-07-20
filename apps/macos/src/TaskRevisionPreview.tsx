import type { NaturalParseAttemptView } from './backend-api'
import type { TaskEditDraft } from './task-edit-draft'

function TaskRevisionPreview({
  draft,
  attempt,
}: {
  draft: TaskEditDraft
  attempt?: NaturalParseAttemptView
}) {
  const steps = planSteps(draft.planJson?.steps)
  const hasPlan = Boolean(draft.planId && draft.planJson)

  return (
    <section className="task-editor__audit task-editor__preview" aria-labelledby="task-plan-preview-heading">
      <div>
        <div>
          <p className="eyebrow">确认前可见</p>
          <h3 id="task-plan-preview-heading">安全计划预览</h3>
        </div>
        <span className="status-pill" data-tone={hasPlan ? 'success' : 'warning'}>
          {hasPlan ? '后端已生成' : '等待生成'}
        </span>
      </div>
      {!hasPlan ? (
        <p role="status">尚未生成可预览的安全计划。原始需求和失败诊断已保留，可重新解析或使用表单修正。</p>
      ) : (
        <>
          <dl>
            <div><dt>目标地区</dt><dd>{draft.regionCode || '未设置'}</dd></div>
            <div><dt>目标检索语言</dt><dd>{draft.queryLocale || '不适用'}</dd></div>
            <div><dt>实际检索词或标识</dt><dd>{draft.sourceInput || '未设置'}</dd></div>
            <div><dt>提示词版本</dt><dd>{attempt?.prompt_version_id ?? '表单编辑，不适用'}</dd></div>
          </dl>
          <dl>
            <div><dt>意图 Schema</dt><dd>{attempt ? 'collection_intent_v1' : '不适用'}</dd></div>
            <div><dt>最终计划 Schema</dt><dd>{`collection_plan_v${draft.schemaVersion ?? 4}`}</dd></div>
            <div>
              <dt>地区证据</dt>
              <dd>{draft.selectedFields.includes('country_region')
                ? 'country_region · 明确值复核'
                : '未启用'}</dd>
            </div>
            <div>
              <dt>时间证据</dt>
              <dd>{draft.selectedFields.includes('last_posted_at')
                ? 'last_posted_at · UTC 边界复核'
                : '未启用'}</dd>
            </div>
          </dl>
          <ol className="task-editor__history" aria-label="后端生成的调用步骤">
            {steps.length > 0 ? steps.map((step, index) => (
              <li key={`${step.endpoint}-${index}`}>
                <strong>{String(index + 1).padStart(2, '0')} · {step.operation}</strong>
                <code>{step.endpoint}</code>
              </li>
            )) : <li>当前计划没有可执行步骤，必须修正后才能确认运行。</li>}
          </ol>
          <p>只有点击“确认运行”后才会调用 TikHub；检索语言不能作为账号地区证据。</p>
        </>
      )}
    </section>
  )
}

function planSteps(value: unknown) {
  if (!Array.isArray(value)) return []
  return value.flatMap((step) => {
    if (!isRecord(step)) return []
    const operation = text(step.operation_key)
    const endpoint = text(step.endpoint_key)
    return operation && endpoint ? [{ operation, endpoint }] : []
  })
}

function text(value: unknown) {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}

export default TaskRevisionPreview
