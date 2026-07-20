import {
  AlertTriangle,
  CheckCircle2,
  Clock3,
  FilePenLine,
  RefreshCcw,
  Settings,
  ShieldCheck,
} from 'lucide-react'
import type { BackendProblem } from './backend-problem'
import type { NaturalParseState } from './natural-parse-state'
import './NaturalParseFeedback.css'

type NaturalParseFeedbackProps = {
  state: NaturalParseState
  onRetry?: () => void
  onOpenAiSettings?: () => void
  onSwitchToForm?: () => void
  onViewDiagnostics?: () => void
}

const phaseLabels: Record<NaturalParseState['phase'], string> = {
  idle: '等待输入',
  preparing: '检查 AI 配置',
  requesting_ai: '等待模型响应',
  validating_intent: '校验结构化意图',
  building_plan: '生成安全计划',
  needs_review: '解析完成，需要补充信息',
  success: '安全计划已生成',
  failed: '自然语言解析失败',
}

export default function NaturalParseFeedback({
  state,
  onRetry,
  onOpenAiSettings,
  onSwitchToForm,
  onViewDiagnostics,
}: NaturalParseFeedbackProps) {
  if (state.phase === 'idle') return null
  const running = ['preparing', 'requesting_ai', 'validating_intent', 'building_plan']
    .includes(state.phase)
  const failed = state.phase === 'failed'
  const needsReview = state.phase === 'needs_review'
  const success = state.phase === 'success'
  const remediation = state.problem ? remediationForProblem(state.problem) : undefined
  const retryAfter = state.problem?.safeDetails.retry_after

  return (
    <section
      className="natural-parse-feedback"
      data-phase={state.phase}
      role={failed ? 'alert' : 'status'}
      aria-live={failed ? 'assertive' : 'polite'}
      aria-atomic="true"
    >
      <div className="natural-parse-feedback__heading">
        <span className="natural-parse-feedback__icon" aria-hidden="true">
          {failed || needsReview
            ? <AlertTriangle size={17} />
            : success
              ? <CheckCircle2 size={17} />
              : <Clock3 size={17} />}
        </span>
        <div>
          <strong>{running ? '正在解析自然语言需求' : phaseLabels[state.phase]}</strong>
          {running && <span>{phaseLabels[state.phase]}</span>}
        </div>
        {running && <span className="natural-parse-feedback__elapsed">已等待 {elapsedSeconds(state)} 秒</span>}
      </div>

      {running && (
        <div className="natural-parse-feedback__body">
          <p>
            {state.modelId
              ? `AI 配置：${state.providerId ?? '当前配置'} · ${state.modelId}`
              : '正在读取当前已测试的 AI 配置与模型'}
          </p>
          <p className="natural-parse-feedback__safety">
            <ShieldCheck size={15} aria-hidden="true" />
            解析不会自动调用 TikHub；确认运行前不会产生采集请求。
          </p>
        </div>
      )}

      {(failed || needsReview) && state.problem && (
        <div className="natural-parse-feedback__body">
          <p className="natural-parse-feedback__message">{state.problem.message}</p>
          <dl className="natural-parse-feedback__facts">
            <div><dt>错误码</dt><dd>{state.problem.code}</dd></div>
            <div><dt>失败阶段</dt><dd>{phaseLabels[state.phase] ?? state.problem.stage}</dd></div>
            <div><dt>最近尝试</dt><dd>{formatAttemptTime(state.finishedAt)}</dd></div>
            <div><dt>可重试</dt><dd>{state.problem.retryable ? '是' : '否'}</dd></div>
            <div><dt>草稿与原始输入</dt><dd>{state.draftPreserved ? '已保留' : '状态未知'}</dd></div>
          </dl>
          {retryAfter !== undefined && retryAfter !== null && (
            <p>建议等待：{String(retryAfter)}</p>
          )}
          <p className="natural-parse-feedback__remediation">
            <strong>修改方式：</strong>{remediation?.message}
          </p>
        </div>
      )}

      {success && (
        <p className="natural-parse-feedback__success">
          意图已通过 Schema 和能力校验；实际检索词与后端生成步骤可在下方计划预览中确认。
        </p>
      )}

      {(failed || needsReview) && (
        <div className="natural-parse-feedback__actions">
          {remediation?.showRetry && onRetry && (
            <button type="button" className="button button--secondary" onClick={onRetry}>
              <RefreshCcw size={15} aria-hidden="true" />重新解析
            </button>
          )}
          {remediation?.showSettings && onOpenAiSettings && (
            <button type="button" className="button button--secondary" onClick={onOpenAiSettings}>
              <Settings size={15} aria-hidden="true" />打开 AI 设置
            </button>
          )}
          {remediation?.showForm && onSwitchToForm && (
            <button type="button" className="button button--secondary" onClick={onSwitchToForm}>
              <FilePenLine size={15} aria-hidden="true" />切换到表单修正
            </button>
          )}
          {onViewDiagnostics && (
            <button type="button" className="button button--ghost" onClick={onViewDiagnostics}>
              查看诊断
            </button>
          )}
        </div>
      )}
    </section>
  )
}

function remediationForProblem(problem: BackendProblem) {
  const configurationError = [
    'MODEL_CONFIG_ERROR',
    'MODEL_AUTH_ERROR',
    'MODEL_PROTOCOL_ERROR',
    'MODEL_NOT_FOUND',
  ].includes(problem.code)
  const legacyConfigurationError = problem.code === 'VALIDATION_ERROR'
    && /(?:AI 配置|API Key|真实连通性测试)/.test(problem.message)
  const reviewError = [
    'VALIDATION_ERROR',
    'MODEL_SCHEMA_ERROR',
    'COST_LIMIT_ERROR',
  ].includes(problem.code)
  if (configurationError || legacyConfigurationError) {
    return {
      message: '打开 AI 设置，检查 Base URL、API Key 和模型 ID，完成真实连通性测试后重新解析。',
      showRetry: false,
      showSettings: true,
      showForm: false,
    }
  }
  if (reviewError) {
    return {
      message: '切换到结构化表单，补齐缺失字段或移除当前平台、来源不支持的条件。',
      showRetry: false,
      showSettings: false,
      showForm: true,
    }
  }
  if (problem.retryable) {
    return {
      message: '保留当前输入；按建议等待后重新解析，不会自动重复发送可能已计费的模型请求。',
      showRetry: true,
      showSettings: false,
      showForm: false,
    }
  }
  return {
    message: '查看诊断并编辑任务；原始输入、失败记录和运行快照会继续保留。',
    showRetry: true,
    showSettings: false,
    showForm: true,
  }
}

function elapsedSeconds(state: NaturalParseState) {
  const startedAt = Date.parse(state.startedAt ?? '')
  if (!Number.isFinite(startedAt)) return 0
  return Math.max(0, Math.floor((Date.now() - startedAt) / 1_000))
}

function formatAttemptTime(value: string | undefined) {
  if (!value) return '刚刚'
  const date = new Date(value)
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat('zh-CN', {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      }).format(date)
}
