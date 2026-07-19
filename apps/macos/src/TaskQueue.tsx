import { useState } from 'react'
import { Ban, Download, ListChecks, ListTodo, Pencil, Play, Save, Table2, Trash2, X } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import AppSelect from './AppSelect'
import { backendErrorMessage, type ExportJobView } from './backend-api'
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
import TaskRunLogPanel from './TaskRunLogPanel'
import type { TaskExportInput, WorkbenchRuntimeData } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'
import './TaskQueue.css'
import './TaskQueueActions.css'

type TaskRow = WorkbenchRuntimeData['tasks'][number]
type ActiveTaskMode =
  | { taskId: string; type: 'edit' }
  | { taskId: string; type: 'confirm-run' }
  | { taskId: string; type: 'confirm-cancel' }
  | { taskId: string; type: 'confirm-delete' }
type ConfirmationMode = Exclude<ActiveTaskMode, { type: 'edit' }>
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

type TaskQueueProps = {
  tasks: TaskRow[]
  isBusy: boolean
  onUpdateTask: (input: { taskId: string; name: string }) => Promise<unknown>
  onCancelTask: (taskId: string) => Promise<unknown>
  onConfirmTask: (taskId: string) => Promise<unknown>
  onDeleteTask: (taskId: string) => Promise<unknown>
  onExportTask: (input: TaskExportInput) => Promise<ExportJobView>
}

function TaskQueue({
  tasks,
  isBusy,
  onUpdateTask,
  onCancelTask,
  onConfirmTask,
  onDeleteTask,
  onExportTask,
}: TaskQueueProps) {
  const { t, i18n } = useTranslation('tasks')
  const [activeMode, setActiveMode] = useState<ActiveTaskMode>()
  const [previewTaskId, setPreviewTaskId] = useState<string>()
  const [resultsTaskId, setResultsTaskId] = useState<string>()
  const [draftName, setDraftName] = useState('')
  const [exportFormats, setExportFormats] = useState<Record<string, TaskExportInput['format']>>({})
  const [exportFeedback, setExportFeedback] = useState<Record<string, TaskExportFeedback>>({})
  const [actionErrors, setActionErrors] = useState<Record<string, string>>({})
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
  const runTimeFormatter = new Intl.DateTimeFormat(numberLocale, {
    dateStyle: 'medium',
    timeStyle: 'medium',
  })
  const localizeRunStage = (code?: string, raw?: string | null) => {
    const fallback = showRawDiagnostics && raw
      ? raw
      : String(t('taskQueue.diagnostics.unknownStage', { code: code ?? 'UNKNOWN_STAGE' }))
    return code && code !== 'UNKNOWN_STAGE'
      ? String(t(`taskQueue.diagnostics.stage.${code}`, { defaultValue: fallback }))
      : fallback
  }
  const localizeRunError = (code: string | null | undefined, raw: string) => {
    if (showRawDiagnostics) return raw
    const fallback = String(t('taskQueue.diagnostics.unknownError', { code: code ?? 'UNKNOWN_ERROR' }))
    return code
      ? String(t(`taskQueue.diagnostics.error.${code}`, { defaultValue: fallback }))
      : fallback
  }

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
            const taskMode = activeMode?.taskId === task.id ? activeMode : undefined
            const isEditing = taskMode?.type === 'edit'
            const confirmation = taskMode?.type === 'confirm-run'
              || taskMode?.type === 'confirm-cancel'
              || taskMode?.type === 'confirm-delete'
              ? taskMode
              : undefined
            const isConfirming = Boolean(confirmation)
            const actionError = actionErrors[task.id]
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
                    {isEditing ? (
                      <div className="task-card__edit-form" role="group" aria-label={t('taskQueue.editFormAriaLabel')}>
                        <label id={titleId} htmlFor={`task-name-${task.id}`}>{t('taskQueue.taskNameLabel')}</label>
                        <div className="task-card__edit-row">
                          <input
                            id={`task-name-${task.id}`}
                            aria-label={t('taskQueue.newNameAriaLabel', { taskName: task.name })}
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
                            {t('taskQueue.save')}
                          </button>
                          <button
                            className="ghost-button"
                            disabled={isBusy}
                            type="button"
                            aria-label={t('taskQueue.discard')}
                            onClick={stopEditing}
                          >
                            <X size={15} aria-hidden="true" />
                            {t('taskQueue.discard')}
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div className="task-card__title-view">
                        <h3 id={titleId}>{task.name}</h3>
                        <p className="task-card__meta">{task.platform} · {sourceLabel}</p>
                      </div>
                    )}
                  </div>
                  <StatusPill tone={toneForStatus(task.status)} label={t(taskStatusTranslationKeys[task.status])} />
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
                      aria-label={t('taskQueue.progressAriaLabel', { taskName: task.name })}
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

                {resultsTaskId === task.id ? (
                  <TaskResultsPanel taskId={task.id} taskName={task.name} />
                ) : null}

                {task.latestRun ? (
                  <section
                    aria-label={t('taskQueue.runDetails')}
                    className="task-card__run-details"
                  >
                    <header className="task-card__run-heading">
                      <h4>{t('taskQueue.runDetails')}</h4>
                      <span>{t('taskQueue.attempt', { count: task.latestRun.attemptNumber })}</span>
                    </header>
                    <dl className="task-card__run-facts">
                      <div>
                        <dt>{t('taskQueue.currentStage')}</dt>
                        <dd>{localizeRunStage(
                          task.latestRun.currentStageCode,
                          task.latestRun.currentStage,
                        )}</dd>
                      </div>
                      <div>
                        <dt>{t('taskQueue.startedAt')}</dt>
                        <dd>
                          <time dateTime={task.latestRun.startedAt}>
                            {runTimeFormatter.format(new Date(task.latestRun.startedAt))}
                          </time>
                        </dd>
                      </div>
                      <div>
                        <dt>{t('taskQueue.endedAt')}</dt>
                        <dd>
                          {task.latestRun.endedAt ? (
                            <time dateTime={task.latestRun.endedAt}>
                              {runTimeFormatter.format(new Date(task.latestRun.endedAt))}
                            </time>
                          ) : t('taskQueue.inProgress')}
                        </dd>
                      </div>
                      {task.latestRun.errorCode ? (
                        <div className="task-card__run-fact--error">
                          <dt>{t('taskQueue.errorCode')}</dt>
                          <dd>{task.latestRun.errorCode}</dd>
                        </div>
                      ) : null}
                      {task.latestRun.errorMessage ? (
                        <div className="task-card__run-fact--error">
                          <dt>{t('taskQueue.errorMessage')}</dt>
                          <dd>{localizeRunError(
                            task.latestRun.errorCode,
                            task.latestRun.errorMessage,
                          )}</dd>
                        </div>
                      ) : null}
                      <div>
                        <dt>{t('taskQueue.retryable')}</dt>
                        <dd>{t(task.latestRun.retryable
                          ? 'taskQueue.retryableYes'
                          : 'taskQueue.retryableNo')}</dd>
                      </div>
                    </dl>
                    <TaskRunLogPanel key={task.latestRun.id} runId={task.latestRun.id} />
                  </section>
                ) : null}

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
                          aria-label={t('taskQueue.edit')}
                          title={capabilities.canEdit ? t('taskQueue.editTitle') : t('taskQueue.editDisabledTitle')}
                          type="button"
                          onClick={() => beginEditing(task)}
                        >
                          <Pencil size={15} aria-hidden="true" />
                          {t('taskQueue.edit')}
                        </button>
                        <button
                          className="ghost-button"
                          disabled={isBusy || isEditing || !capabilities.canCancel || isConfirming}
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
                          disabled={isBusy || isEditing || !capabilities.canDelete || isConfirming}
                          aria-label={t('taskQueue.delete')}
                          title={capabilities.canDelete ? t('taskQueue.deleteTitle') : t('taskQueue.deleteDisabledTitle')}
                          type="button"
                          onClick={() => setActiveMode({ taskId: task.id, type: 'confirm-delete' })}
                        >
                          <Trash2 size={15} aria-hidden="true" />
                          {t('taskQueue.delete')}
                        </button>
                      </div>
                      <div className="task-card__run-action">
                        {capabilities.canExport && resultsTaskId !== task.id ? (
                          <button
                            aria-label={t('taskQueue.viewResults')}
                            className="primary-button"
                            disabled={isBusy || isEditing || isConfirming}
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
                            disabled={isBusy || isEditing || isConfirming}
                            aria-label={t('taskQueue.confirmRun')}
                            type="button"
                            onClick={() => setActiveMode({ taskId: task.id, type: 'confirm-run' })}
                          >
                            <Play size={15} aria-hidden="true" />
                            {t('taskQueue.confirmRun')}
                          </button>
                        ) : null}
                      </div>
                      <div className="task-card__export">
                        <div className="task-card__export-field">
                          <label htmlFor={`task-export-format-${task.id}`}>{t('taskQueue.exportFormat')}</label>
                          <AppSelect
                            id={`task-export-format-${task.id}`}
                            disabled={isBusy || isEditing || isConfirming}
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
                          disabled={isBusy || isEditing || isConfirming || !capabilities.canExport}
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
