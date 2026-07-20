import {
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
  code?: string | null
  message: string
  retryable: boolean
  attemptedAt?: string | null
  safeDetails?: Record<string, unknown>
  draftPreserved?: boolean
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
  code,
  message,
  retryable,
  attemptedAt,
  safeDetails = {},
  draftPreserved = true,
  onAction,
}: TaskProblemPanelProps) {
  const remediation = remediationForTaskProblem(code, message)
  const actions = [remediation.primaryAction, remediation.secondaryAction]
    .filter((action): action is TaskRemediationAction => Boolean(action))

  return (
    <section
      aria-label={kind === 'natural_parse' ? '自然语言解析失败详情' : '任务运行失败详情'}
      className="task-problem"
      data-kind={kind}
    >
      <header>
        <div>
          <span>{kind === 'natural_parse' ? '解析失败' : '运行失败'}</span>
          <strong>{message}</strong>
        </div>
        <code>{code || 'UNCLASSIFIED_ERROR'}</code>
      </header>
      <dl>
        <div><dt>错误码</dt><dd>{code || 'UNCLASSIFIED_ERROR'}</dd></div>
        <div><dt>可重试</dt><dd>{retryable ? '是' : '否'}</dd></div>
        <div><dt>最近尝试</dt><dd>{formatAttemptTime(attemptedAt)}</dd></div>
        <div><dt>草稿与记录</dt><dd>{draftPreserved ? '已保留' : '状态未知'}</dd></div>
      </dl>
      {safeDetails.retry_after !== undefined && (
        <p className="task-problem__retry-after">
          Retry-After：{String(safeDetails.retry_after)}
        </p>
      )}
      <p className="task-problem__remediation">
        <strong>修改方式：</strong>{remediation.message}
      </p>
      {onAction && (
        <div className="task-problem__actions">
          {actions.map((action) => {
            const Icon = actionIcons[action]
            return (
              <button
                className={action === remediation.primaryAction ? 'ghost-button' : 'text-button'}
                key={action}
                type="button"
                onClick={() => onAction(action)}
              >
                <Icon size={14} aria-hidden="true" />
                {actionLabels[action]}
              </button>
            )
          })}
        </div>
      )}
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
