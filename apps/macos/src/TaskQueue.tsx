import { useRef, useState } from 'react'
import { Ban, Download, ListChecks, ListTodo, Pencil, Play, Table2, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import AppSelect from './AppSelect'
import {
  backendErrorMessage,
  type ExportJobView,
  type WorkspaceHealthCheckView,
} from './backend-api'
import { StatusPill } from './CollectionBuilder'
import {
  capabilitiesForStatus,
  confirmationForTaskAction,
  taskDataTypeTranslationKeys,
  taskExportFormatOptions,
  taskSourceTranslationKeys,
  taskStatusTranslationKeys,
} from './task-queue-config'
import TaskResultsPanel from './TaskResultsPanel'
import TaskProblemPanel from './TaskProblemPanel'
import TaskRunDetails from './TaskRunDetails'
import {
  isNaturalParsePlaceholder,
  isNaturalParseProvenanceOnly,
} from './natural-parse-state'
import type { TaskRemediationAction } from './task-remediation'
import type { TaskExportInput, WorkbenchRuntimeData } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'
import { describeWorkspaceHealth } from './workspace-health'
import './TaskQueue.css'
import './TaskQueueActions.css'

type TaskRow = WorkbenchRuntimeData['tasks'][number]
type ConfirmationMode =
  | { taskId: string; type: 'confirm-run' }
  | { taskId: string; type: 'confirm-cancel' }
  | { taskId: string; type: 'confirm-delete' }
type TaskExportFeedback = {
  errorReason?: string
  errorType?: 'export' | 'missing-path' | 'open'
  filePath?: string
}
type BulkDeleteFailure = {
  failedCount: number
  reason: string
  totalCount: number
}
type TaskActionNotice = { message: string; passed: boolean }

type TaskQueueProps = {
  tasks: TaskRow[]
  isBusy: boolean
  onEditTask: (taskId: string) => void
  onCancelTask: (taskId: string) => Promise<unknown>
  onConfirmTask: (taskId: string) => Promise<unknown>
  onDeleteTask: (taskId: string) => Promise<unknown>
  onExportTask: (input: TaskExportInput) => Promise<ExportJobView>
  onRetryNaturalTask?: (taskId: string, intentText: string) => Promise<unknown>
  onRetryTask?: (taskId: string) => Promise<unknown>
  onOpenSettings?: () => void
  onRefresh?: () => void
  onWorkspaceHealthCheck?: () => Promise<WorkspaceHealthCheckView>
}

function TaskQueue({
  tasks,
  isBusy,
  onEditTask,
  onCancelTask,
  onConfirmTask,
  onDeleteTask,
  onExportTask,
  onRetryNaturalTask,
  onRetryTask,
  onOpenSettings,
  onRefresh,
  onWorkspaceHealthCheck,
}: TaskQueueProps) {
  const { t, i18n } = useTranslation('tasks')
  const [activeMode, setActiveMode] = useState<ConfirmationMode>()
  const [previewTaskId, setPreviewTaskId] = useState<string>()
  const [resultsTaskId, setResultsTaskId] = useState<string>()
  const [exportFormats, setExportFormats] = useState<Record<string, TaskExportInput['format']>>({})
  const [exportFeedback, setExportFeedback] = useState<Record<string, TaskExportFeedback>>({})
  const [actionErrors, setActionErrors] = useState<Record<string, string>>({})
  const [actionNotices, setActionNotices] = useState<Record<string, TaskActionNotice>>({})
  const [pendingProblemTaskIds, setPendingProblemTaskIds] = useState<string[]>([])
  const problemActionsInFlightRef = useRef(new Set<string>())
  const [bulkMode, setBulkMode] = useState(false)
  const [selectedTaskIds, setSelectedTaskIds] = useState<string[]>([])
  const [bulkDeleteConfirmationOpen, setBulkDeleteConfirmationOpen] = useState(false)
  const [bulkDeleteFailure, setBulkDeleteFailure] = useState<BulkDeleteFailure>()
  const [isBulkDeleting, setIsBulkDeleting] = useState(false)
  const previewTask = previewTaskId
    ? tasks.find((task) => task.id === previewTaskId)
    : undefined
  const visibleTasks = previewTask ? [previewTask] : tasks
  const deletableTaskIds = tasks
    .filter((task) => capabilitiesForStatus(task.status).canDelete)
    .map((task) => task.id)
  const deletableTaskIdSet = new Set(deletableTaskIds)
  const validSelectedTaskIds = selectedTaskIds.filter((taskId) => deletableTaskIdSet.has(taskId))
  const allDeletableTasksSelected = deletableTaskIds.length > 0
    && validSelectedTaskIds.length === deletableTaskIds.length
  const someDeletableTasksSelected = validSelectedTaskIds.length > 0
    && !allDeletableTasksSelected
  const numberLocale = i18n.resolvedLanguage ?? i18n.language
  const showRawDiagnostics = numberLocale.toLowerCase().startsWith('zh')
  const handleProblemAction = async (
    task: TaskRow,
    kind: 'natural_parse' | 'run',
    action: TaskRemediationAction,
  ) => {
    if (action === 'edit_task') onEditTask(task.id)
    else if (action === 'view_diagnostics') {
      if (kind === 'natural_parse') onEditTask(task.id)
      else setPreviewTaskId(task.id)
    }
    else if (action === 'open_ai_settings' || action === 'open_tikhub_settings') onOpenSettings?.()
    else if (action === 'reload') onRefresh?.()
    else if (action === 'workspace_health') {
      const actionKey = `${task.id}:${kind}:workspace_health`
      if (problemActionsInFlightRef.current.has(actionKey)) return
      problemActionsInFlightRef.current.add(actionKey)
      setPendingProblemTaskIds((taskIds) => taskIds.includes(task.id)
        ? taskIds
        : [...taskIds, task.id])
      setActionErrors((errors) => ({ ...errors, [task.id]: '' }))
      setActionNotices((notices) => ({ ...notices, [task.id]: { message: '', passed: false } }))
      try {
        const health = await onWorkspaceHealthCheck?.()
        if (!health) throw new Error('工作区健康检查未连接')
        setActionNotices((notices) => ({
          ...notices,
          [task.id]: describeWorkspaceHealth(health),
        }))
      } catch (error) {
        setActionErrors((errors) => ({ ...errors, [task.id]: backendErrorMessage(error) }))
      } finally {
        problemActionsInFlightRef.current.delete(actionKey)
        setPendingProblemTaskIds((taskIds) => taskIds.filter((taskId) => taskId !== task.id))
      }
    }
    else if (action === 'retry') {
      const actionKey = `${task.id}:${kind}:retry`
      if (problemActionsInFlightRef.current.has(actionKey)) return
      problemActionsInFlightRef.current.add(actionKey)
      setPendingProblemTaskIds((taskIds) => taskIds.includes(task.id)
        ? taskIds
        : [...taskIds, task.id])
      setActionErrors((errors) => ({ ...errors, [task.id]: '' }))
      try {
        if (kind === 'natural_parse' && task.naturalParseAttempt) {
          await onRetryNaturalTask?.(task.id, task.naturalParseAttempt.intent_text)
        } else {
          await onRetryTask?.(task.id)
        }
      } catch (error) {
        setActionErrors((errors) => ({
          ...errors,
          [task.id]: backendErrorMessage(error),
        }))
      } finally {
        problemActionsInFlightRef.current.delete(actionKey)
        setPendingProblemTaskIds((taskIds) => taskIds.filter((taskId) => taskId !== task.id))
      }
    }
  }

  const confirmAction = async (action: ConfirmationMode) => {
    setActionErrors((errors) => ({ ...errors, [action.taskId]: '' }))
    try {
      if (action.type === 'confirm-run') {
        await onConfirmTask(action.taskId)
        setResultsTaskId(undefined)
        setPreviewTaskId(action.taskId)
      } else if (action.type === 'confirm-cancel') {
        await onCancelTask(action.taskId)
      } else {
        await onDeleteTask(action.taskId)
        setSelectedTaskIds((taskIds) => taskIds.filter((taskId) => taskId !== action.taskId))
      }
      setActiveMode(undefined)
    } catch (error) {
      if (action.type === 'confirm-run') {
        setActionErrors((errors) => ({
          ...errors,
          [action.taskId]: backendErrorMessage(error),
        }))
      }
      // 保留确认态，让用户看到未入队原因并可以直接重试。
    }
  }

  const openExportFile = async (taskId: string, filePath: string) => {
    setExportFeedback((feedback) => ({
      ...feedback,
      [taskId]: { filePath },
    }))
    try {
      const { openPath } = await import('@tauri-apps/plugin-opener')
      await openPath(filePath)
    } catch (error) {
      setExportFeedback((feedback) => ({
        ...feedback,
        [taskId]: {
          errorReason: backendErrorMessage(error),
          errorType: 'open',
          filePath,
        },
      }))
    }
  }

  const exportTask = async (taskId: string) => {
    setExportFeedback((feedback) => {
      const next = { ...feedback }
      delete next[taskId]
      return next
    })
    try {
      const exportJob = await onExportTask({ taskId, format: exportFormats[taskId] ?? 'xlsx' })
      const filePath = exportJob.file_path?.trim()
      if (!filePath) {
        setExportFeedback((feedback) => ({
          ...feedback,
          [taskId]: { errorType: 'missing-path' },
        }))
        return
      }
      await openExportFile(taskId, filePath)
    } catch (error) {
      setExportFeedback((feedback) => ({
        ...feedback,
        [taskId]: {
          errorReason: backendErrorMessage(error),
          errorType: 'export',
        },
      }))
    }
  }

  const toggleTaskSelection = (taskId: string) => {
    setBulkDeleteFailure(undefined)
    setSelectedTaskIds((taskIds) => taskIds.includes(taskId)
      ? taskIds.filter((selectedTaskId) => selectedTaskId !== taskId)
      : [...taskIds, taskId])
  }

  const exitBulkMode = () => {
    setBulkMode(false)
    setSelectedTaskIds([])
    setBulkDeleteConfirmationOpen(false)
    setBulkDeleteFailure(undefined)
  }

  const toggleAllDeletableTasks = () => {
    setBulkDeleteFailure(undefined)
    setSelectedTaskIds(allDeletableTasksSelected ? [] : deletableTaskIds)
  }

  const confirmBulkDelete = async () => {
    const taskIds = [...validSelectedTaskIds]
    if (taskIds.length === 0) {
      setBulkDeleteConfirmationOpen(false)
      return
    }

    setIsBulkDeleting(true)
    setBulkDeleteFailure(undefined)
    const failedTaskIds: string[] = []
    let firstFailureReason = ''
    for (const taskId of taskIds) {
      try {
        await onDeleteTask(taskId)
      } catch (error) {
        failedTaskIds.push(taskId)
        firstFailureReason ||= backendErrorMessage(error)
      }
    }
    setSelectedTaskIds(failedTaskIds)
    if (failedTaskIds.length > 0) {
      setBulkDeleteFailure({
        failedCount: failedTaskIds.length,
        reason: firstFailureReason,
        totalCount: taskIds.length,
      })
    } else {
      setBulkDeleteConfirmationOpen(false)
      setBulkMode(false)
    }
    setIsBulkDeleting(false)
  }

  return (
    <section className="task-queue" aria-labelledby="task-queue-heading">
      <header className="task-queue__heading">
        <div>
          <p className="eyebrow">{t('taskQueue.eyebrow')}</p>
          <h2 id="task-queue-heading">{previewTask?.name ?? t('taskQueue.title')}</h2>
          <p className="task-queue__intro">{t('taskQueue.intro')}</p>
        </div>
        {previewTask ? (
          <button
            aria-label={t('taskQueue.backToList')}
            className="ghost-button"
            type="button"
            onClick={() => {
              setPreviewTaskId(undefined)
              setResultsTaskId(undefined)
            }}
          >
            {t('taskQueue.backToList')}
          </button>
        ) : (
          <div className="task-queue__heading-actions">
            <span className="task-queue__count">
              {t('taskQueue.taskCount', {
                count: tasks.length,
                formattedCount: tasks.length.toLocaleString(numberLocale),
              })}
            </span>
            {tasks.length > 0 ? (
              <button
                aria-label={t(bulkMode ? 'taskQueue.exitBulkMode' : 'taskQueue.enterBulkMode')}
                aria-pressed={bulkMode}
                className="ghost-button"
                type="button"
                onClick={() => {
                  if (bulkMode) exitBulkMode()
                  else setBulkMode(true)
                }}
              >
                <ListChecks size={15} aria-hidden="true" />
                {t(bulkMode ? 'taskQueue.exitBulkMode' : 'taskQueue.enterBulkMode')}
              </button>
            ) : null}
          </div>
        )}
      </header>

      {tasks.length > 0 && !previewTask && bulkMode ? (
        <div
          aria-label={t('taskQueue.bulkToolbarAriaLabel')}
          className="task-queue__bulk-toolbar"
          role="toolbar"
        >
          {bulkDeleteConfirmationOpen ? (
            <div className="task-queue__bulk-confirmation" role="group">
              <p>{t('taskQueue.bulkDeleteConfirmation', {
                count: validSelectedTaskIds.length,
              })}</p>
              {bulkDeleteFailure ? (
                <p className="task-card__confirmation-error" role="alert">
                  {t('taskQueue.bulkDeleteFailed', bulkDeleteFailure)}
                </p>
              ) : null}
              <div>
                <button
                  aria-label={t('taskQueue.confirmBulkDelete')}
                  className="task-card__confirm-danger"
                  disabled={isBusy || isBulkDeleting || validSelectedTaskIds.length === 0}
                  type="button"
                  onClick={() => void confirmBulkDelete()}
                >
                  {t('taskQueue.confirmBulkDelete')}
                </button>
                <button
                  aria-label={t('taskQueue.back')}
                  className="ghost-button"
                  disabled={isBusy || isBulkDeleting}
                  type="button"
                  onClick={() => {
                    setBulkDeleteConfirmationOpen(false)
                    setBulkDeleteFailure(undefined)
                  }}
                >
                  {t('taskQueue.back')}
                </button>
              </div>
            </div>
          ) : (
            <>
              <label>
                <input
                  aria-checked={someDeletableTasksSelected ? 'mixed' : allDeletableTasksSelected}
                  aria-label={t('taskQueue.selectAllDeletable')}
                  checked={allDeletableTasksSelected}
                  disabled={isBusy || isBulkDeleting || deletableTaskIds.length === 0}
                  type="checkbox"
                  onChange={toggleAllDeletableTasks}
                />
                <span>{t('taskQueue.selectAllDeletable')}</span>
              </label>
              <span>{t('taskQueue.selectionCount', { count: validSelectedTaskIds.length })}</span>
              <button
                aria-label={t('taskQueue.bulkDelete')}
                className="ghost-button task-card__danger-button"
                disabled={isBusy || isBulkDeleting || validSelectedTaskIds.length === 0}
                type="button"
                onClick={() => {
                  setActiveMode(undefined)
                  setBulkDeleteFailure(undefined)
                  setBulkDeleteConfirmationOpen(true)
                }}
              >
                <Trash2 size={15} aria-hidden="true" />
                {t('taskQueue.bulkDelete')}
              </button>
            </>
          )}
        </div>
      ) : null}

      {tasks.length === 0 ? (
        <div className="task-queue__empty" role="status">
          <ListTodo size={24} strokeWidth={1.7} aria-hidden="true" />
          <div>
            <h3>{t('taskQueue.emptyTitle')}</h3>
            <p>{t('taskQueue.emptyDescription')}</p>
          </div>
        </div>
      ) : (
        <div className="task-queue__list" role="list">
          {visibleTasks.map((task) => {
            const confirmation = activeMode?.taskId === task.id ? activeMode : undefined
            const isConfirming = Boolean(confirmation)
            const actionError = actionErrors[task.id]
            const actionNotice = actionNotices[task.id]
            const taskExportFeedback = exportFeedback[task.id]
            const exportedFilePath = taskExportFeedback?.filePath
            const exportError = taskExportFeedback?.errorType === 'missing-path'
              ? t('taskQueue.exportMissingPath')
              : taskExportFeedback?.errorType === 'open'
                ? t('taskQueue.openExportFailed', { reason: taskExportFeedback.errorReason })
                : taskExportFeedback?.errorType === 'export'
                  ? t('taskQueue.exportFailed', { reason: taskExportFeedback.errorReason })
                  : undefined
            const confirmationContent = confirmation
              ? confirmationForTaskAction(confirmation.type)
              : {
                  ariaLabel: t('taskQueue.confirmationFallbackAriaLabel'),
                  buttonLabel: t('taskQueue.confirmationFallbackButton'),
                  message: t('taskQueue.confirmationFallbackMessage'),
                  tone: 'danger' as const,
                }
            const capabilities = capabilitiesForStatus(task.status)
            const progress = Math.min(100, Math.max(0, task.progress))
            const titleId = `task-title-${task.id}`
            const formattedRecords = task.records.toLocaleString(numberLocale)
            const sourceLabel = task.sourceType
              ? t(taskSourceTranslationKeys[task.sourceType])
              : task.source
            const dataTypeKey = task.dataTypeCode
              ? taskDataTypeTranslationKeys[task.dataTypeCode] ?? taskDataTypeTranslationKeys.comments
              : undefined
            const costLabel = dataTypeKey
              ? `${task.requestCount
                ? t('taskQueue.requestEstimate', { count: task.requestCount })
                : t('taskQueue.requestEstimateUnknown')} · ${t(dataTypeKey)}`
              : task.cost
            const parseAttempt = task.naturalParseAttempt
            const parseNeedsAttention = parseAttempt
              && !isNaturalParseProvenanceOnly(parseAttempt)
              && !isNaturalParsePlaceholder(parseAttempt)
              && ['failed', 'interrupted', 'needs_review'].includes(parseAttempt.parse_status)
            const parseFailed = parseNeedsAttention && parseAttempt
              && ['failed', 'interrupted'].includes(parseAttempt.parse_status)
            const parseNeedsReview = parseNeedsAttention
              && parseAttempt.parse_status === 'needs_review'
            const displayTaskName = parseNeedsAttention && parseAttempt
              ? parseAttempt.intent_text
              : task.name

            return (
              <article
                aria-labelledby={titleId}
                className="task-card"
                data-mode={confirmation?.type ?? 'default'}
                data-status={task.status}
                key={task.id}
                role="listitem"
              >
                <header className="task-card__header">
                  {!previewTask && bulkMode ? (
                    <label
                      className="task-card__selection"
                      title={capabilities.canDelete
                        ? t('taskQueue.selectTask', { taskName: task.name })
                        : t('taskQueue.selectTaskDisabled')}
                    >
                      <input
                        aria-label={t('taskQueue.selectTask', { taskName: task.name })}
                        checked={validSelectedTaskIds.includes(task.id)}
                        disabled={isBusy || isBulkDeleting || !capabilities.canDelete}
                        type="checkbox"
                        onChange={() => toggleTaskSelection(task.id)}
                      />
                    </label>
                  ) : null}
                  <div className="task-card__identity">
                    <div className="task-card__title-view">
                      <h3 id={titleId}>{displayTaskName}</h3>
                      <p className="task-card__meta">{task.platform} · {sourceLabel}</p>
                    </div>
                  </div>
                  <StatusPill
                    tone={parseFailed ? 'danger' : parseNeedsReview ? 'warning' : toneForStatus(task.status)}
                    label={parseFailed
                      ? '解析失败'
                      : parseNeedsReview ? '待修正' : t(taskStatusTranslationKeys[task.status])}
                  />
                </header>

                <div className="task-card__summary">
                  <dl className="task-card__stats">
                    <div>
                      <dt>{t('taskQueue.resultRecords')}</dt>
                      <dd>{t('taskQueue.recordCount', { count: task.records, formattedCount: formattedRecords })}</dd>
                    </div>
                    <div>
                      <dt>{t('taskQueue.requestCost')}</dt>
                      <dd>{costLabel}</dd>
                    </div>
                  </dl>
                  <div className="task-card__progress">
                    <div className="task-card__progress-label">
                      <span>{t('taskQueue.executionProgress')}</span>
                      <strong>{progress}%</strong>
                    </div>
                    <div
                      aria-label={t('taskQueue.progressAriaLabel', { taskName: displayTaskName })}
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

                {parseNeedsAttention ? (
                  <div className="task-card__parse-problem">
                    <TaskProblemPanel
                      kind="natural_parse"
                      naturalState={parseAttempt.parse_status === 'needs_review'
                        ? 'needs_review'
                        : 'failed'}
                      code={parseAttempt.error_code}
                      message={parseAttempt.error_message
                        ?? (parseAttempt.parse_status === 'needs_review'
                          ? '解析完成，需要补充信息后才能生成安全计划'
                          : '上次自然语言解析被中断，请重新解析')}
                      retryable={parseAttempt.retryable ?? parseAttempt.parse_status === 'interrupted'}
                      attemptedAt={parseAttempt.updated_at}
                      safeDetails={parseAttempt.error_safe_details_json}
                      isBusy={pendingProblemTaskIds.includes(task.id)}
                      onAction={(action) => void handleProblemAction(task, 'natural_parse', action)}
                    />
                  </div>
                ) : null}

                {resultsTaskId === task.id ? (
                  <TaskResultsPanel taskId={task.id} taskName={task.name} />
                ) : null}

                {task.latestRun ? (
                  <TaskRunDetails
                    run={task.latestRun}
                    isBusy={pendingProblemTaskIds.includes(task.id)}
                    onProblemAction={(action) => void handleProblemAction(task, 'run', action)}
                  />
                ) : null}

                {actionError && !isConfirming ? (
                  <p className="task-card__confirmation-error" role="alert">
                    操作失败：{actionError}
                  </p>
                ) : null}

                {actionNotice?.message && !actionError && !isConfirming ? (
                  <p
                    className="task-card__health-result"
                    data-passed={actionNotice.passed}
                    role={actionNotice.passed ? 'status' : 'alert'}
                  >
                    {actionNotice.message}
                  </p>
                ) : null}

                <footer className="task-card__footer">
                  <div className="task-card__action-slot">
                    <div
                      aria-hidden={isConfirming}
                      className="task-card__actions"
                      data-visible={!isConfirming}
                    >
                      <div className="task-card__run-action">
                        {capabilities.canExport && resultsTaskId !== task.id ? (
                          <button
                            aria-label={t('taskQueue.viewResults')}
                            className="primary-button"
                            disabled={isBusy || isConfirming}
                            type="button"
                            onClick={() => {
                              setActiveMode(undefined)
                              exitBulkMode()
                              setResultsTaskId(task.id)
                              setPreviewTaskId(task.id)
                            }}
                          >
                            <Table2 size={15} aria-hidden="true" />
                            {t('taskQueue.viewResults')}
                          </button>
                        ) : null}
                        {capabilities.canConfirm ? (
                          <button
                            className="primary-button"
                            disabled={isBusy || isConfirming}
                            aria-label={t('taskQueue.confirmRun')}
                            type="button"
                            onClick={() => setActiveMode({ taskId: task.id, type: 'confirm-run' })}
                          >
                            <Play size={15} aria-hidden="true" />
                            {t('taskQueue.confirmRun')}
                          </button>
                        ) : null}
                      </div>
                      <div className="task-card__secondary-actions">
                        <button
                          className="ghost-button"
                          disabled={isBusy || !capabilities.canEdit || isConfirming}
                          aria-label={t('taskQueue.edit')}
                          title={capabilities.canEdit ? t('taskQueue.editTitle') : t('taskQueue.editDisabledTitle')}
                          type="button"
                          onClick={() => onEditTask(task.id)}
                        >
                          <Pencil size={15} aria-hidden="true" />
                          {t('taskQueue.edit')}
                        </button>
                        <button
                          className="ghost-button"
                          disabled={isBusy || !capabilities.canCancel || isConfirming}
                          aria-label={t('taskQueue.cancel')}
                          title={capabilities.canCancel ? t('taskQueue.cancelTitle') : t('taskQueue.cancelDisabledTitle')}
                          type="button"
                          onClick={() => setActiveMode({ taskId: task.id, type: 'confirm-cancel' })}
                        >
                          <Ban size={15} aria-hidden="true" />
                          {t('taskQueue.cancel')}
                        </button>
                        <button
                          className="ghost-button task-card__danger-button"
                          disabled={isBusy || !capabilities.canDelete || isConfirming}
                          aria-label={t('taskQueue.delete')}
                          title={capabilities.canDelete ? t('taskQueue.deleteTitle') : t('taskQueue.deleteDisabledTitle')}
                          type="button"
                          onClick={() => setActiveMode({ taskId: task.id, type: 'confirm-delete' })}
                        >
                          <Trash2 size={15} aria-hidden="true" />
                          {t('taskQueue.delete')}
                        </button>
                      </div>
                      <div className="task-card__export">
                        <div className="task-card__export-field">
                          <label htmlFor={`task-export-format-${task.id}`}>{t('taskQueue.exportFormat')}</label>
                          <AppSelect
                            id={`task-export-format-${task.id}`}
                            disabled={isBusy || isConfirming}
                            onChange={(format) => setExportFormats((formats) => ({
                              ...formats,
                              [task.id]: format as TaskExportInput['format'],
                            }))}
                            options={taskExportFormatOptions}
                            placeholder={t('taskQueue.exportFormatPlaceholder')}
                            value={exportFormats[task.id] ?? 'xlsx'}
                          />
                        </div>
                        <button
                          className="ghost-button"
                          disabled={isBusy || isConfirming || !capabilities.canExport}
                          aria-label={t('taskQueue.export')}
                          title={capabilities.canExport ? t('taskQueue.exportTitle') : t('taskQueue.exportDisabledTitle')}
                          type="button"
                          onClick={() => void exportTask(task.id)}
                        >
                          <Download size={15} aria-hidden="true" />
                          {t('taskQueue.export')}
                        </button>
                      </div>
                    </div>

                    <div
                      aria-hidden={!isConfirming}
                      aria-label={confirmationContent.ariaLabel}
                      aria-live="polite"
                      className="task-card__confirmation"
                      data-visible={isConfirming}
                      role="group"
                    >
                      <p>{confirmationContent.message}</p>
                      {confirmation?.type === 'confirm-run' && actionError ? (
                        <p className="task-card__confirmation-error" role="alert">
                          {t('taskQueue.confirmRunFailed', {
                            reason: showRawDiagnostics
                              ? actionError
                              : t('taskQueue.confirmRunFailedFallback'),
                          })}
                        </p>
                      ) : null}
                      <div className="task-card__confirmation-actions">
                        <button
                          className={confirmationContent.tone === 'primary'
                            ? 'primary-button'
                            : 'task-card__confirm-danger'}
                          disabled={isBusy || !confirmation}
                          tabIndex={confirmation ? 0 : -1}
                          type="button"
                          aria-label={confirmationContent.buttonLabel}
                          onClick={() => confirmation && void confirmAction(confirmation)}
                        >
                          {confirmationContent.buttonLabel}
                        </button>
                        <button
                          className="ghost-button"
                          disabled={isBusy || !confirmation}
                          tabIndex={confirmation ? 0 : -1}
                          type="button"
                          aria-label={t('taskQueue.back')}
                          onClick={() => setActiveMode(undefined)}
                        >
                          {t('taskQueue.back')}
                        </button>
                      </div>
                    </div>
                  </div>
                  {taskExportFeedback ? (
                    <div aria-live="polite" className="task-card__export-result">
                      {exportedFilePath ? (
                        <div>
                          <p><strong>{t('taskQueue.exportReady')}</strong></p>
                          <p>
                            <span>{t('taskQueue.exportPath')}</span>
                            <code>{exportedFilePath}</code>
                          </p>
                          <button
                            aria-label={t('taskQueue.openExportFile')}
                            className="ghost-button"
                            disabled={isBusy}
                            type="button"
                            onClick={() => void openExportFile(task.id, exportedFilePath)}
                          >
                            {t('taskQueue.openExportFile')}
                          </button>
                        </div>
                      ) : null}
                      {exportError ? (
                        <p className="task-card__confirmation-error" role="alert">
                          {exportError}
                        </p>
                      ) : null}
                    </div>
                  ) : null}
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
