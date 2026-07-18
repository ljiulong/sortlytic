import { getApiProfileRegistry, testApiProfile } from './api-profiles'
import { quoteTikhubConnectorPrice } from './backend-api'

export type CollectionPricingPlan = {
  pricingEndpoints?: string[]
  requestCountEstimate?: number
  budgetMicros?: number
}

const endpointPaths: Record<string, string[]> = {
  'tiktok.keyword_search': ['/api/v1/tiktok/app/v3/fetch_video_search_result'],
  'tiktok.item_detail': ['/api/v1/tiktok/app/v3/fetch_one_video'],
  'tiktok.account_profile': ['/api/v1/tiktok/app/v3/handler_user_profile'],
  'tiktok.account_posts': ['/api/v1/tiktok/app/v3/fetch_user_post_videos'],
  'tiktok.comments': ['/api/v1/tiktok/app/v3/fetch_video_comments'],
  'douyin.keyword_search': ['/api/v1/douyin/search/fetch_video_search_v2'],
  'douyin.item_detail': ['/api/v1/douyin/app/v3/fetch_one_video'],
  'douyin.account_profile': ['/api/v1/douyin/app/v3/handler_user_profile'],
  'douyin.account_posts': ['/api/v1/douyin/app/v3/fetch_user_post_videos'],
  'douyin.comments': ['/api/v1/douyin/app/v3/fetch_video_comments'],
  'xiaohongshu.keyword_search': ['/api/v1/xiaohongshu/app_v2/search_notes'],
  'xiaohongshu.item_detail': [
    '/api/v1/xiaohongshu/app_v2/get_image_note_detail',
    '/api/v1/xiaohongshu/app_v2/get_video_note_detail',
  ],
  'xiaohongshu.account_profile': ['/api/v1/xiaohongshu/app_v2/get_user_info'],
  'xiaohongshu.account_posts': ['/api/v1/xiaohongshu/app_v2/get_user_posted_notes'],
  'xiaohongshu.comments': ['/api/v1/xiaohongshu/app_v2/get_note_comments'],
}

export function pricingEndpointsForPlan(planJson: Record<string, unknown>) {
  if (!Array.isArray(planJson.steps)) return []
  const endpoints = planJson.steps.flatMap((step) => {
    if (!step || typeof step !== 'object' || !('endpoint_key' in step)) return []
    const key = typeof step.endpoint_key === 'string' ? step.endpoint_key : ''
    return endpointPaths[key] ?? []
  })
  return [...new Set(endpoints)]
}

export async function preflightCollectionPlanPricing(plan: CollectionPricingPlan) {
  const requestCount = plan.requestCountEstimate
  const budgetMicros = plan.budgetMicros
  const endpoints = [...new Set(plan.pricingEndpoints ?? [])]
  if (!Number.isInteger(requestCount) || !requestCount || requestCount <= 0) {
    throw new Error('计划请求次数未知，无法确认运行')
  }
  if (!Number.isInteger(budgetMicros) || !budgetMicros || budgetMicros <= 0) {
    throw new Error('计划预算未知，无法确认运行')
  }
  if (!endpoints.length) throw new Error('TikHub 计价端点未知，无法确认运行')

  let registry
  try {
    registry = await getApiProfileRegistry()
  } catch {
    throw new Error('TikHub API 配置读取失败，无法确认运行')
  }

  const activeProfileId = registry.activeProfileIds.tikhub
  if (!activeProfileId) {
    throw new Error('当前未选择 TikHub API 配置，无法确认运行')
  }
  const activeProfile = registry.tikhubProfiles.find(({ id }) => id === activeProfileId)
  if (!activeProfile) {
    throw new Error('当前 TikHub API 配置不存在，无法确认运行')
  }
  if (activeProfile.status !== 'success') {
    throw new Error('当前 TikHub API 配置未通过验证，无法确认运行')
  }

  let testResult
  try {
    testResult = await testApiProfile('tikhub', activeProfileId)
  } catch {
    throw new Error('当前 TikHub API 配置测试失败，无法确认运行')
  }
  if (!testResult.success) {
    throw new Error('当前 TikHub API 配置测试失败，无法确认运行')
  }
  if (testResult.registry.activeProfileIds.tikhub !== activeProfileId) {
    throw new Error('当前 TikHub API 配置在测试期间已变更，无法确认运行')
  }
  const testedProfile = testResult.registry.tikhubProfiles.find(
    ({ id }) => id === activeProfileId,
  )
  if (!testedProfile || testedProfile.status !== 'success' || !testedProfile.testSummary) {
    throw new Error('当前 TikHub API 配置测试结果不完整，无法确认运行')
  }

  const balance = requiredMoney(testedProfile.testSummary.balance, 'TikHub 充值余额')
  const freeCredit = requiredMoney(testedProfile.testSummary.freeCredit, 'TikHub 免费额度')
  const availableCredit = requiredMoney(
    testedProfile.testSummary.availableCredit,
    'TikHub 可用额度合计',
  )
  if (Math.abs(balance + freeCredit - availableCredit) > 0.000001) {
    throw new Error('TikHub 额度合计与免费额度、充值余额不一致')
  }
  const quotes = []
  for (const endpoint of endpoints) {
    quotes.push(await quoteTikhubConnectorPrice(endpoint, 1))
  }
  const unitPrice = Math.max(...quotes.map((quote) => requiredMoney(quote.total_price, 'TikHub 实时报价')))
  const quotedTotalMicros = Math.round(unitPrice * requestCount * 1_000_000)
  if (quotedTotalMicros > budgetMicros) {
    throw new Error('TikHub 实时报价超过计划预算上限')
  }
  if (quotedTotalMicros > Math.round(availableCredit * 1_000_000)) {
    throw new Error('TikHub 免费额度与充值余额合计不足')
  }
  return { balance, freeCredit, availableCredit, quotedTotalMicros }
}

function requiredMoney(value: number | null | undefined, label: string) {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
    throw new Error(`${label}未知，无法确认运行`)
  }
  return value
}
