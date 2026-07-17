import { useState } from 'react'
import { Download, Pencil, Play, Save, Trash2, X } from 'lucide-react'
import { StatusPill } from './CollectionBuilder'
import type { TaskExportInput, WorkbenchRuntimeData } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'

type TaskRow = WorkbenchRuntimeData['tasks'][number]
type PendingAction = { taskId: string; type: 'cancel' | 'run' }

type TaskQueueProps = {
  tasks: TaskRow[]
  isBusy: boolean
  onUpdateTask: (input: { taskId: string; name: string }) => Promise<unknown>
  onCancelTask: (taskId: string) => Promise<unknown>
  onConfirmTask: (taskId: string) => Promise<unknown>
  onExportTask: (input: TaskExportInput) => Promise<unknown>
}

function TaskQueue({
  tasks,
  isBusy,
  onUpdateTask,
  onCancelTask,
  onConfirmTask,
  onExportTask,
}: TaskQueueProps) {
  const [editingTaskId, setEditingTaskId] = useState<string>()
  const [draftName, setDraftName] = useState('')
  const [pendingAction, setPendingAction] = useState<PendingAction>()
  const [exportFormats, setExportFormats] = useState<Record<string, TaskExportInput['format']>>({})

  const beginEditing = (task: TaskRow) => {
    setEditingTaskId(task.id)
    setDraftName(task.name)
    setPendingAction(undefined)
  }

  const saveTaskName = async (taskId: string) => {
    try {
      await onUpdateTask({ taskId, name: draftName })
      setEditingTaskId(undefined)
      setDraftName('')
    } catch {
      // 后端错误会显示在工作区状态栏，保留编辑态便于用户修正。
    }
  }

  const confirmAction = async (action: PendingAction) => {
    try {
      if (action.type === 'run') {
        await onConfirmTask(action.taskId)
      } else {
        await onCancelTask(action.taskId)
      }
      setPendingAction(undefined)
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
    <section className="glass-panel">
      <div className="section-heading">
        <div>
          <p className="eyebrow">任务</p>
          <h2>逐条确认、运行与导出</h2>
        </div>
      </div>
      <div className="task-list">
        {tasks.length === 0 ? (
          <p className="muted-text">暂无真实任务，请先前往“新建任务”创建计划。</p>
        ) : null}
        {tasks.map((task) => {
          const isEditing = editingTaskId === task.id
          const canEdit = task.status === '等待确认' || task.status === '待人工确认'
          const canCancel = ['等待确认', '待人工确认', '已排队', '运行中'].includes(task.status)
          const canConfirm = task.status === '等待确认'
          const canExport = task.status === '成功' || task.status === '部分成功'
          const confirmation = pendingAction?.taskId === task.id ? pendingAction : undefined

          return (
            <article className="task-row" key={task.id}>
              <div>
                {isEditing ? (
                  <div className="field">
                    <span>任务名称</span>
                    <input
                      aria-label={`${task.name} 新名称`}
                      autoFocus
                      maxLength={80}
                      value={draftName}
                      onChange={(event) => setDraftName(event.target.value)}
                    />
                    <div className="action-row">
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
                        onClick={() => setEditingTaskId(undefined)}
                      >
                        <X size={15} aria-hidden="true" />
                        放弃
                      </button>
                    </div>
                  </div>
                ) : (
                  <>
                    <h3>{task.name}</h3>
                    <p>{task.platform} · {task.source} · {task.records.toLocaleString()} 条</p>
                  </>
                )}
              </div>
              <StatusPill tone={toneForStatus(task.status)} label={task.status} />
              <div className="progress-cell">
                <div className="progress-bar" aria-label={`${task.name} 进度 ${task.progress}%`}>
                  <span style={{ width: `${task.progress}%` }} />
                </div>
                <strong>{task.cost}</strong>

                {confirmation ? (
                  <div aria-label={confirmation.type === 'run' ? '确认运行任务' : '确认取消任务'} className="field" role="group">
                    <span>
                      {confirmation.type === 'run'
                        ? '确认后任务将进入执行队列并可能产生费用。'
                        : '取消后当前任务不能继续运行。'}
                    </span>
                    <div className="action-row">
                      <button
                        className="primary-button"
                        disabled={isBusy}
                        type="button"
                        onClick={() => void confirmAction(confirmation)}
                      >
                        {confirmation.type === 'run' ? '确认运行' : '确认取消'}
                      </button>
                      <button
                        className="ghost-button"
                        disabled={isBusy}
                        type="button"
                        onClick={() => setPendingAction(undefined)}
                      >
                        返回
                      </button>
                    </div>
                  </div>
                ) : (
                  <div className="action-row">
                    <button
                      className="ghost-button"
                      disabled={isBusy || !canEdit}
                      title={canEdit ? '编辑任务名称' : '只有待确认任务可以编辑'}
                      type="button"
                      onClick={() => beginEditing(task)}
                    >
                      <Pencil size={15} aria-hidden="true" />
                      编辑
                    </button>
                    <button
                      className="ghost-button"
                      disabled={isBusy || !canCancel}
                      title={canCancel ? '取消任务' : '终态任务不能取消'}
                      type="button"
                      onClick={() => setPendingAction({ taskId: task.id, type: 'cancel' })}
                    >
                      <Trash2 size={15} aria-hidden="true" />
                      取消任务
                    </button>
                    {canConfirm ? (
                      <button
                        className="primary-button"
                        disabled={isBusy}
                        type="button"
                        onClick={() => setPendingAction({ taskId: task.id, type: 'run' })}
                      >
                        <Play size={15} aria-hidden="true" />
                        确认运行
                      </button>
                    ) : null}
                  </div>
                )}

                <div className="field">
                  <span>导出格式</span>
                  <select
                    aria-label={`${task.name} 导出格式`}
                    disabled={isBusy}
                    value={exportFormats[task.id] ?? 'xlsx'}
                    onChange={(event) => setExportFormats((formats) => ({
                      ...formats,
                      [task.id]: event.target.value as TaskExportInput['format'],
                    }))}
                  >
                    <option value="xlsx">Excel 工作簿</option>
                    <option value="pdf">PDF 报告</option>
                  </select>
                  <button
                    className="ghost-button"
                    disabled={isBusy || !canExport}
                    title={canExport ? '导出当前任务' : '任务完成或部分成功后可以导出'}
                    type="button"
                    onClick={() => void exportTask(task.id)}
                  >
                    <Download size={15} aria-hidden="true" />
                    导出
                  </button>
                </div>
              </div>
            </article>
          )
        })}
      </div>
    </section>
  )
}

function toneForStatus(status: TaskStatus) {
  if (status === '成功') return 'success'
  if (status === '失败') return 'danger'
  if (status === '待人工确认' || status === '等待确认') return 'warning'
  return 'info'
}

export default TaskQueue
