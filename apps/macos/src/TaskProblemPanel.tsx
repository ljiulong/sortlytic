import {
  ChevronDown,
  FilePenLine,
  HeartPulse,
  RefreshCcw,
  RotateCcw,
  Search,
  Settings,
} from 'lucide-react'
import {
  remediationForTaskProblem,
  type TaskRemediationAction,
} from './task-remediation'
import './TaskProblemPanel.css'

type TaskProblemPanelProps = {
  kind: 'natural_parse' | 'run'
  naturalState?: 'failed' | 'needs_review'
  code?: string | null
  message: string
  retryable: boolean
  attemptedAt?: string | null
  safeDetails?: Record<string, unknown>
  draftPreserved?: boolean
  isBusy?: boolean
  onAction?: (action: TaskRemediationAction) => void
}

const actionLabels: Record<TaskRemediationAction, string> = {
  edit_task: '编辑任务',
  open_ai_settings: '打开 AI 设置',
  open_tikhub_settings: '打开 TikHub 设置',
  retry: '重新尝试',
  reload: '重新读取',
  workspace_health: '工作区健康检查',
  view_diagnostics: '查看诊断',
}

const actionIcons: Record<TaskRemediationAction, typeof Search> = {
  edit_task: FilePenLine,
  open_ai_settings: Settings,
  open_tikhub_settings: Settings,
  retry: RotateCcw,
  reload: RefreshCcw,
  workspace_health: HeartPulse,
  view_diagnostics: Search,
}

export default function TaskProblemPanel({
  kind,
  naturalState = 'failed',
  code,
  message,
  retryable,
  attemptedAt,
  safeDetails = {},
  draftPreserved = true,
  isBusy = false,
  onAction,
}: TaskProblemPanelProps) {
  const needsReview = kind === 'natural_parse' && naturalState === 'needs_review'
  const displayCode = code || (needsReview ? 'NEEDS_REVIEW' : 'UNCLASSIFIED_ERROR')
  const remediation = needsReview
    ? {
        message: '点击“编辑任务”补齐缺失字段或移除不兼容条件，保存合格计划后才能确认运行。',
        primaryAction: 'edit_task' as const,
        secondaryAction: 'view_diagnostics' as const,
      }
    : remediationForTaskProblem(code, message, retryable, safeDetails)
  const actions = [remediation.primaryAction, remediation.secondaryAction]
    .filter((action): action is TaskRemediationAction => Boolean(action))

  return (
    <section
      aria-label={needsReview
        ? '自然语言解析待补充详情'
        : kind === 'natural_parse' ? '自然语言解析失败详情' : '任务运行失败详情'}
      aria-live={needsReview ? 'polite' : 'assertive'}
      aria-atomic="true"
      className="task-problem"
      data-kind={kind}
      data-tone={needsReview ? 'warning' : 'danger'}
      role={needsReview ? 'status' : 'alert'}
    >
      <div className="task-problem__message">
        <span>{needsReview ? '解析完成，需要补充信息' : '问题原因'}</span>
        <strong>{message}</strong>
      </div>
      <p className="task-problem__remediation">
        <span>修改方式</span>
        {remediation.message}
      </p>
      {onAction && (
        <div className="task-problem__actions">
          {actions.map((action) => {
            const Icon = actionIcons[action]
            const isPrimary = action === remediation.primaryAction
            return (
              <button
                className={isPrimary
                  ? 'ghost-button task-problem__action'
                  : 'task-problem__action task-problem__action--quiet'}
                disabled={isBusy}
                key={action}
                type="button"
                onClick={() => onAction(action)}
              >
                <Icon size={15} aria-hidden="true" />
                <span>
                  {isBusy && action === 'retry'
                    ? '正在重新尝试'
                    : kind === 'natural_parse' && action === 'view_diagnostics'
                      ? '查看解析记录'
                      : actionLabels[action]}
                </span>
              </button>
            )
          })}
        </div>
      )}
      <details className="task-problem__details">
        <summary>
          <span>技术详情</span>
          <ChevronDown size={15} aria-hidden="true" />
        </summary>
        <dl>
          <div>
            <dt>{needsReview ? '状态码' : '错误码'}</dt>
            <dd><code>{displayCode}</code></dd>
          </div>
          <div><dt>可重试</dt><dd>{retryable ? '是' : '否'}</dd></div>
          <div><dt>最近尝试</dt><dd>{formatAttemptTime(attemptedAt)}</dd></div>
          <div><dt>草稿与记录</dt><dd>{draftPreserved ? '已保留' : '状态未知'}</dd></div>
        </dl>
        {safeDetails.retry_after !== undefined && (
          <p className="task-problem__retry-after">
            Retry-After：{String(safeDetails.retry_after)}
          </p>
        )}
      </details>
    </section>
  )
}

function formatAttemptTime(value: string | null | undefined) {
  if (!value) return '时间不可用'
  const date = new Date(value)
  return Number.isNaN(date.getTime())
    ? value
    : new Intl.DateTimeFormat('zh-CN', {
        dateStyle: 'short',
        timeStyle: 'short',
      }).format(date)
}
