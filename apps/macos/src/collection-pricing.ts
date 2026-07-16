import { quoteTikhubConnectorPrice, testTikhubConnector } from './backend-api'

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

  const quota = await testTikhubConnector()
  const balance = requiredMoney(quota.balance, 'TikHub 充值余额')
  const freeCredit = requiredMoney(quota.free_credit, 'TikHub 免费额度')
  const availableCredit = requiredMoney(quota.available_credit, 'TikHub 可用额度合计')
  if (Math.abs(balance + freeCredit - availableCredit) > 0.000001) {
    throw new Error('TikHub 额度合计与免费额度、充值余额不一致')
  }
  const quotes = await Promise.all(
    endpoints.map((endpoint) => quoteTikhubConnectorPrice(endpoint, 1)),
  )
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
