import { i18n as appI18n } from './i18n'
import type { TaskExportInput } from './use-workbench-backend'
import type { TaskStatus } from './workbench-data'

export const taskStatusTranslationKeys: Record<TaskStatus, string> = {
  已排队: 'taskStatus.queued',
  运行中: 'taskStatus.running',
  等待确认: 'taskStatus.waitingConfirmation',
  部分成功: 'taskStatus.partialSuccess',
  成功: 'taskStatus.success',
  待人工确认: 'taskStatus.manualConfirmation',
  失败: 'taskStatus.failed',
  已取消: 'taskStatus.cancelled',
}

export const taskSourceTranslationKeys = {
  natural_language: 'taskQueue.source.naturalLanguage',
  form: 'taskQueue.source.form',
} as const

export const taskDataTypeTranslationKeys: Record<string, string> = {
  keyword_search: 'taskQueue.dataType.keywordSearch',
  account_profile: 'taskQueue.dataType.accountProfile',
  item_detail: 'taskQueue.dataType.itemDetail',
  account_posts: 'taskQueue.dataType.accountPosts',
  comments: 'taskQueue.dataType.comments',
}

const taskExportFormatOptionKeys = [
  { value: 'xlsx', key: 'export.formats.xlsx' },
  { value: 'pdf', key: 'export.formats.pdf' },
] as const

export const taskExportFormatOptions = taskExportFormatOptionKeys.map(({ value, key }) => ({
  value,
  get label() {
    return appI18n.t(key, { ns: 'tasks' })
  },
})) satisfies Array<{ value: TaskExportInput['format']; label: string }>

export function capabilitiesForStatus(status: TaskStatus) {
  return {
    canEdit: status === '等待确认' || status === '待人工确认',
    canCancel: ['等待确认', '待人工确认', '已排队', '运行中'].includes(status),
    canConfirm: status === '等待确认',
    canDelete: status !== '已排队' && status !== '运行中',
    canExport: status === '成功' || status === '部分成功',
  }
}

export function confirmationForTaskAction(
  type: 'confirm-run' | 'confirm-cancel' | 'confirm-delete',
) {
  if (type === 'confirm-run') {
    return {
      ariaLabel: appI18n.t('confirmation.confirmRun.ariaLabel', { ns: 'tasks' }),
      buttonLabel: appI18n.t('confirmation.confirmRun.button', { ns: 'tasks' }),
      message: appI18n.t('confirmation.confirmRun.message', { ns: 'tasks' }),
      tone: 'primary' as const,
    }
  }
  if (type === 'confirm-cancel') {
    return {
      ariaLabel: appI18n.t('confirmation.confirmCancel.ariaLabel', { ns: 'tasks' }),
      buttonLabel: appI18n.t('confirmation.confirmCancel.button', { ns: 'tasks' }),
      message: appI18n.t('confirmation.confirmCancel.message', { ns: 'tasks' }),
      tone: 'danger' as const,
    }
  }
  return {
    ariaLabel: appI18n.t('confirmation.confirmDelete.ariaLabel', { ns: 'tasks' }),
    buttonLabel: appI18n.t('confirmation.confirmDelete.button', { ns: 'tasks' }),
    message: appI18n.t('confirmation.confirmDelete.message', { ns: 'tasks' }),
    tone: 'danger' as const,
  }
}
