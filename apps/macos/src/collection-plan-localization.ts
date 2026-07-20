import type { CollectionTranslator } from './collection-options'
import type { RuntimeCollectionPlan } from './use-workbench-backend'

const planStatusTranslationKeys: Record<RuntimeCollectionPlan['status'], string> = {
  '已排队': 'status.queued',
  '运行中': 'status.running',
  '等待确认': 'status.awaitingConfirmation',
  '部分成功': 'status.partialSuccess',
  '成功': 'status.success',
  '待人工确认': 'status.manualConfirmation',
  '失败': 'status.failed',
  '已取消': 'status.cancelled',
}

const planMessageTranslationKeys: Record<string, string> = {
  '年龄范围必须填写上下限': 'message.ageRangeBoundsRequired',
  '价格未知': 'message.priceUnknown',
  'TikHub 免费额度与充值余额合计不足': 'message.tikhubCreditsInsufficient',
  'TikHub 额度合计与免费额度、充值余额不一致': 'message.tikhubCreditsMismatch',
  'TikHub 实时报价超过计划预算上限': 'message.pricingExceedsBudget',
  '未提供时间范围': 'message.rangeMissing',
  '计划校验未通过': 'message.planValidationFailed',
  '计划校验未通过，无法确认运行': 'message.planValidationFailedCannotRun',
}

const actionMessageTranslationKeys: Record<string, string> = {
  '后端正在初始化本地工作区': 'action.initializing',
  '等待生成': 'action.waitingForPlan',
  '等待确认': 'status.awaitingConfirmation',
  '采集计划已保存到本地 SQLite，等待确认运行': 'action.formPlanSaved',
  '自然语言计划已生成，并保存了提示词运行快照': 'action.naturalPlanSaved',
  '任务已确认并加入本地队列': 'action.confirmed',
  '任务名称已更新': 'action.renamed',
  '任务已取消': 'action.cancelled',
  '任务已删除': 'action.deleted',
  '当前未连接本地后端，不展示预览数据；请打开打包后的 macOS 应用': 'action.backendUnavailable',
  '本地工作区已打开，后端可用': 'action.workspaceReady',
  '计划需要修正': 'action.planNeedsRevision',
  '后端调用失败': 'action.backendCallFailed',
}

const dynamicPlanMessageTranslationKeys: Array<[RegExp, string]> = [
  [/^region 尚未验证$/, 'message.regionNotVerified'],
  [/^time_range 不能为空$/, 'message.timeRangeRequired'],
]

const dynamicPricingMessageTranslationKeys: Array<[RegExp, string]> = [
  [/(?:HTTP|code) 429|请求过于频繁/, 'message.pricingRateLimited'],
]

export function localizedPlanStatus(
  t: CollectionTranslator,
  status: RuntimeCollectionPlan['status'],
) {
  return t(planStatusTranslationKeys[status] ?? 'status.unknown', { defaultValue: status })
}

export function localizePlanMessage(t: CollectionTranslator, message: string | undefined) {
  if (!message) return ''
  const key = planMessageTranslationKeys[message]
  if (key) return t(key)
  const dynamicKey = dynamicPlanMessageTranslationKeys.find(([pattern]) => pattern.test(message))?.[1]
  if (dynamicKey) return t(dynamicKey)
  return /[^\p{ASCII}]/u.test(message) ? t('message.unknown') : message
}

export function localizePricingMessage(t: CollectionTranslator, message: string | undefined) {
  if (!message) return ''
  const key = planMessageTranslationKeys[message]
  if (key) return t(key)
  const dynamicKey = dynamicPricingMessageTranslationKeys
    .find(([pattern]) => pattern.test(message))?.[1]
  return dynamicKey ? t(dynamicKey) : t('message.pricingFailed')
}

export function localizeActionMessage(t: CollectionTranslator, message: string) {
  const exportMatch = /^(Excel|PDF) 已导出到本地工作区$/.exec(message)
  if (exportMatch) return t('action.exported', { format: exportMatch[1] })
  const key = actionMessageTranslationKeys[message]
  if (key) return t(key)
  return /[^\p{ASCII}]/u.test(message) ? t('action.unknown') : message
}

export function formatNumber(value: number, language: string) {
  return value.toLocaleString(language)
}

export function localizedCostEstimate(
  t: CollectionTranslator,
  costEstimate: string | undefined,
  language: string,
) {
  if (!costEstimate) return t('preview.noEstimate')
  const requestEstimate = /^(\d+) 次请求$/.exec(costEstimate)
  if (requestEstimate) {
    return t('preview.requestEstimate', {
      count: formatNumber(Number(requestEstimate[1]), language),
    })
  }
  const requestQuote = /^(\d+) 次请求，实时报价上限 \$([\d.]+)$/.exec(costEstimate)
  if (requestQuote) {
    return t('preview.requestQuote', {
      amount: requestQuote[2],
      count: formatNumber(Number(requestQuote[1]), language),
    })
  }
  return /[^\p{ASCII}]/u.test(costEstimate) ? t('preview.estimateUnavailable') : costEstimate
}

export function confirmationBlocker(
  plan: RuntimeCollectionPlan,
  isBusy: boolean,
  t: CollectionTranslator,
) {
  if (isBusy) return t('blocker.busy')
  if (!plan.taskId || !plan.planId) return t('blocker.unsaved')
  if (plan.validationStatus !== 'valid') {
    return localizePlanMessage(t, plan.missing[0]) || t('blocker.validationFailed')
  }
  if (plan.status === '已排队' || plan.status === '运行中') return undefined
  if (plan.status !== '等待确认') {
    return t('blocker.status', { status: localizedPlanStatus(t, plan.status) })
  }
  return undefined
}

export function toneForPlanStatus(status: RuntimeCollectionPlan['status']) {
  if (status === '成功') return 'success'
  if (status === '失败') return 'danger'
  if (status === '等待确认' || status === '待人工确认' || status === '部分成功') return 'warning'
  return 'info'
}
