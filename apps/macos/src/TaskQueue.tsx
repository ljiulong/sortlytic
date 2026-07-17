import { useState } from 'react'
import { Download, ListTodo, Pencil, Play, Save, Trash2, X } from 'lucide-react'
import { StatusPill } from './CollectionBuilder'
import type { TaskExportInput, WorkbenchRuntimeData } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'
import './TaskQueue.css'

type TaskRow = WorkbenchRuntimeData['tasks'][number]
type ActiveTaskMode =
  | { taskId: string; type: 'edit' }
  | { taskId: string; type: 'confirm-run' }
  | { taskId: string; type: 'confirm-cancel' }
type ConfirmationMode = Exclude<ActiveTaskMode, { type: 'edit' }>

type TaskQueueProps = {
  tasks: TaskRow[]
  isBusy: boolean
  onUpdateTask: (input: { taskId: string; name: string }) => Promise<unknown>
  onCancelTask: (taskId: string) => Promise<unknown>
  onConfirmTask: (taskId: string) => Promise<unknown>
  onExportTask: (input: TaskExportInput) => Promise<unknown>
}

// oxlint-disable-next-line react/only-export-components
export function capabilitiesForStatus(status: TaskStatus) {
  return {
    canEdit: status === '等待确认' || status === '待人工确认',
    canCancel: ['等待确认', '待人工确认', '已排队', '运行中'].includes(status),
    canConfirm: status === '等待确认',
    canExport: status === '成功' || status === '部分成功',
  }
}

function TaskQueue({
  tasks,
  isBusy,
  onUpdateTask,
  onCancelTask,
  onConfirmTask,
  onExportTask,
}: TaskQueueProps) {
  const [activeMode, setActiveMode] = useState<ActiveTaskMode>()
  const [draftName, setDraftName] = useState('')
  const [exportFormats, setExportFormats] = useState<Record<string, TaskExportInput['format']>>({})

  const beginEditing = (task: TaskRow) => {
    setActiveMode({ taskId: task.id, type: 'edit' })
    setDraftName(task.name)
  }

  const stopEditing = () => {
    setActiveMode(undefined)
    setDraftName('')
  }

  const saveTaskName = async (taskId: string) => {
    try {
      await onUpdateTask({ taskId, name: draftName })
      stopEditing()
    } catch {
      // 后端错误会显示在工作区状态栏，保留编辑态便于用户修正。
    }
  }

  const confirmAction = async (action: ConfirmationMode) => {
    try {
      if (action.type === 'confirm-run') {
        await onConfirmTask(action.taskId)
      } else {
        await onCancelTask(action.taskId)
      }
      setActiveMode(undefined)
    } catch {
      // 后端错误会显示在工作区状态栏，保留确认态供用户检查。
    }
  }

  const exportTask = async (taskId: string) => {
    try {
      await onExportTask({ taskId, format: exportFormats[taskId] ?? 'xlsx' })
    } catch {
      // 后端错误会显示在工作区状态栏。
    }
  }

  return (
    <section className="task-queue" aria-labelledby="task-queue-heading">
      <header className="task-queue__heading">
        <div>
          <p className="eyebrow">任务队列</p>
          <h2 id="task-queue-heading">逐条确认、运行与导出</h2>
          <p className="task-queue__intro">每条任务独立管理，完成后可按需要导出 Excel 工作簿或 PDF 报告。</p>
        </div>
        <span className="task-queue__count">{tasks.length} 条任务</span>
      </header>

      {tasks.length === 0 ? (
        <div className="task-queue__empty" role="status">
          <ListTodo size={24} strokeWidth={1.7} aria-hidden="true" />
          <div>
            <h3>还没有可运行的任务</h3>
            <p>前往“新建任务”定义采集条件并生成计划，确认后会显示在这里。</p>
          </div>
        </div>
      ) : (
        <div className="task-queue__list" role="list">
          {tasks.map((task) => {
            const taskMode = activeMode?.taskId === task.id ? activeMode : undefined
            const isEditing = taskMode?.type === 'edit'
            const confirmation = taskMode?.type === 'confirm-run' || taskMode?.type === 'confirm-cancel'
              ? taskMode
              : undefined
            const isConfirming = Boolean(confirmation)
            const capabilities = capabilitiesForStatus(task.status)
            const progress = Math.min(100, Math.max(0, task.progress))
            const titleId = `task-title-${task.id}`
            const confirmationIsRun = confirmation?.type === 'confirm-run'

            return (
              <article
                aria-labelledby={titleId}
                className="task-card"
                data-mode={taskMode?.type ?? 'default'}
                data-status={task.status}
                key={task.id}
                role="listitem"
              >
                <header className="task-card__header">
                  <div className="task-card__identity">
                    {isEditing ? (
                      <div className="task-card__edit-form" role="group" aria-label="编辑任务名称">
                        <label id={titleId} htmlFor={`task-name-${task.id}`}>任务名称</label>
                        <div className="task-card__edit-row">
                          <input
                            id={`task-name-${task.id}`}
                            aria-label={`${task.name} 新名称`}
                            autoFocus
                            maxLength={80}
                            value={draftName}
                            onChange={(event) => setDraftName(event.target.value)}
                          />
                          <button
                            className="primary-button"
                            disabled={isBusy || draftName.trim().length < 2}
                            type="button"
                            onClick={() => void saveTaskName(task.id)}
                          >
                            <Save size={15} aria-hidden="true" />
                            保存
                          </button>
                          <button
                            className="ghost-button"
                            disabled={isBusy}
                            type="button"
                            onClick={stopEditing}
                          >
                            <X size={15} aria-hidden="true" />
                            放弃
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div className="task-card__title-view">
                        <h3 id={titleId}>{task.name}</h3>
                        <p className="task-card__meta">{task.platform} · {task.source}</p>
                      </div>
                    )}
                  </div>
                  <StatusPill tone={toneForStatus(task.status)} label={task.status} />
                </header>

                <div className="task-card__summary">
                  <dl className="task-card__stats">
                    <div>
                      <dt>结果记录</dt>
                      <dd>{task.records.toLocaleString()} 条</dd>
                    </div>
                    <div>
                      <dt>请求 / 费用</dt>
                      <dd>{task.cost}</dd>
                    </div>
                  </dl>
                  <div className="task-card__progress">
                    <div className="task-card__progress-label">
                      <span>执行进度</span>
                      <strong>{progress}%</strong>
                    </div>
                    <div
                      aria-label={`${task.name} 进度`}
                      aria-valuemax={100}
                      aria-valuemin={0}
                      aria-valuenow={progress}
                      className="task-card__progress-track"
                      role="progressbar"
                    >
                      <span className="task-card__progress-fill" style={{ width: `${progress}%` }} />
                    </div>
                  </div>
                </div>

                <footer className="task-card__footer">
                  <div className="task-card__action-slot">
                    <div
                      aria-hidden={isConfirming}
                      className="task-card__actions"
                      data-visible={!isConfirming}
                    >
                      <div className="task-card__secondary-actions">
                        <button
                          className="ghost-button"
                          disabled={isBusy || isEditing || !capabilities.canEdit || isConfirming}
                          title={capabilities.canEdit ? '编辑任务名称' : '只有待确认任务可以编辑'}
                          type="button"
                          onClick={() => beginEditing(task)}
                        >
                          <Pencil size={15} aria-hidden="true" />
                          编辑
                        </button>
                        <button
                          className="ghost-button task-card__danger-button"
                          disabled={isBusy || isEditing || !capabilities.canCancel || isConfirming}
                          title={capabilities.canCancel ? '取消任务' : '终态任务不能取消'}
                          type="button"
                          onClick={() => setActiveMode({ taskId: task.id, type: 'confirm-cancel' })}
                        >
                          <Trash2 size={15} aria-hidden="true" />
                          取消
                        </button>
                      </div>
                      <div className="task-card__run-action">
                        {capabilities.canConfirm ? (
                          <button
                            className="primary-button"
                            disabled={isBusy || isEditing || isConfirming}
                            type="button"
                            onClick={() => setActiveMode({ taskId: task.id, type: 'confirm-run' })}
                          >
                            <Play size={15} aria-hidden="true" />
                            确认运行
                          </button>
                        ) : null}
                      </div>
                      <div className="task-card__export">
                        <label>
                          <span>导出格式</span>
                          <select
                            aria-label={`${task.name} 导出格式`}
                            disabled={isBusy || isEditing || isConfirming}
                            value={exportFormats[task.id] ?? 'xlsx'}
                            onChange={(event) => setExportFormats((formats) => ({
                              ...formats,
                              [task.id]: event.target.value as TaskExportInput['format'],
                            }))}
                          >
                            <option value="xlsx">Excel 工作簿</option>
                            <option value="pdf">PDF 报告</option>
                          </select>
                        </label>
                        <button
                          className="ghost-button"
                          disabled={isBusy || isEditing || isConfirming || !capabilities.canExport}
                          title={capabilities.canExport ? '导出当前任务' : '任务完成或部分成功后可以导出'}
                          type="button"
                          onClick={() => void exportTask(task.id)}
                        >
                          <Download size={15} aria-hidden="true" />
                          导出
                        </button>
                      </div>
                    </div>

                    <div
                      aria-hidden={!isConfirming}
                      aria-label={confirmation
                        ? confirmationIsRun ? '确认运行任务' : '确认取消任务'
                        : '任务操作确认'}
                      aria-live="polite"
                      className="task-card__confirmation"
                      data-visible={isConfirming}
                      role="group"
                    >
                      <p>
                        {confirmation
                          ? confirmationIsRun
                            ? '确认后任务将进入执行队列并可能产生费用。'
                            : '取消后当前任务不能继续运行。'
                          : '请确认当前任务操作。'}
                      </p>
                      <div className="task-card__confirmation-actions">
                        <button
                          className={confirmationIsRun ? 'primary-button' : 'task-card__confirm-danger'}
                          disabled={isBusy || !confirmation}
                          tabIndex={confirmation ? 0 : -1}
                          type="button"
                          onClick={() => confirmation && void confirmAction(confirmation)}
                        >
                          {confirmation ? (confirmationIsRun ? '确认运行' : '确认取消') : '确认操作'}
                        </button>
                        <button
                          className="ghost-button"
                          disabled={isBusy || !confirmation}
                          tabIndex={confirmation ? 0 : -1}
                          type="button"
                          onClick={() => setActiveMode(undefined)}
                        >
                          返回
                        </button>
                      </div>
                    </div>
                  </div>
                </footer>
              </article>
            )
          })}
        </div>
      )}
    </section>
  )
}

function toneForStatus(status: TaskStatus) {
  if (status === '成功') return 'success'
  if (status === '失败') return 'danger'
  if (status === '待人工确认' || status === '等待确认' || status === '部分成功') return 'warning'
  return 'info'
}

export default TaskQueue
