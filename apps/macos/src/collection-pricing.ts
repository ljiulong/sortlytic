import { getApiProfileRegistry, testApiProfile } from './api-profiles'
import {
  quoteTikhubConnectorPrice,
  type TikhubPriceQuote,
} from './backend-api'

export type CollectionPricingPlan = {
  pricingEndpoints?: string[]
  requestCountEstimate?: number
  budgetMicros?: number
}

type PricingProfileSnapshot = {
  balance: number
  freeCredit: number
  availableCredit: number
}

type CacheEntry<T> = {
  expiresAt: number
  value: T
}

const pricingCacheTtlMs = 60_000
const minimumQuoteIntervalMs = 250
const profileCache = new Map<string, CacheEntry<PricingProfileSnapshot>>()
const quoteCache = new Map<string, CacheEntry<TikhubPriceQuote>>()
const profileRequests = new Map<string, Promise<PricingProfileSnapshot>>()
const quoteRequests = new Map<string, Promise<TikhubPriceQuote>>()
let quoteQueue = Promise.resolve()
let lastQuoteStartedAt = Number.NEGATIVE_INFINITY

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

  const profileKey = `${activeProfile.id}:${activeProfile.revision}`
  const profileSnapshot = await cachedProfileSnapshot(profileKey, activeProfileId)
  const quotes = []
  for (const endpoint of endpoints) {
    quotes.push(await cachedQuote(profileKey, endpoint))
  }
  const unitPrice = Math.max(...quotes.map((quote) => requiredMoney(quote.total_price, 'TikHub 实时报价')))
  const quotedTotalMicros = Math.round(unitPrice * requestCount * 1_000_000)
  return { ...profileSnapshot, quotedTotalMicros }
}

async function cachedProfileSnapshot(profileKey: string, activeProfileId: string) {
  const cached = readCache(profileCache, profileKey)
  if (cached) return cached
  const inFlight = profileRequests.get(profileKey)
  if (inFlight) return inFlight

  const request = loadProfileSnapshot(activeProfileId)
    .then((snapshot) => {
      writeCache(profileCache, profileKey, snapshot)
      return snapshot
    })
    .finally(() => profileRequests.delete(profileKey))
  profileRequests.set(profileKey, request)
  return request
}

async function loadProfileSnapshot(activeProfileId: string): Promise<PricingProfileSnapshot> {

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
  return { balance, freeCredit, availableCredit }
}

async function cachedQuote(profileKey: string, endpoint: string) {
  const cacheKey = `${profileKey}:${endpoint}:1`
  const cached = readCache(quoteCache, cacheKey)
  if (cached) return cached
  const inFlight = quoteRequests.get(cacheKey)
  if (inFlight) return inFlight

  const request = scheduleQuote(() => quoteTikhubConnectorPrice(endpoint, 1))
    .then((quote) => {
      writeCache(quoteCache, cacheKey, quote)
      return quote
    })
    .finally(() => quoteRequests.delete(cacheKey))
  quoteRequests.set(cacheKey, request)
  return request
}

async function scheduleQuote<T>(operation: () => Promise<T>) {
  const previous = quoteQueue
  let releaseQueue: () => void = () => {}
  quoteQueue = new Promise<void>((resolve) => {
    releaseQueue = resolve
  })
  await previous
  try {
    const waitMs = Math.max(0, lastQuoteStartedAt + minimumQuoteIntervalMs - Date.now())
    if (waitMs > 0) await delay(waitMs)
    lastQuoteStartedAt = Date.now()
    return await operation()
  } finally {
    releaseQueue()
  }
}

function readCache<T>(cache: Map<string, CacheEntry<T>>, key: string) {
  const entry = cache.get(key)
  if (!entry) return undefined
  if (entry.expiresAt <= Date.now()) {
    cache.delete(key)
    return undefined
  }
  return entry.value
}

function writeCache<T>(cache: Map<string, CacheEntry<T>>, key: string, value: T) {
  cache.set(key, { expiresAt: Date.now() + pricingCacheTtlMs, value })
}

function delay(milliseconds: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, milliseconds))
}

export function resetCollectionPricingStateForTests() {
  profileCache.clear()
  quoteCache.clear()
  profileRequests.clear()
  quoteRequests.clear()
  quoteQueue = Promise.resolve()
  lastQuoteStartedAt = Number.NEGATIVE_INFINITY
}

function requiredMoney(value: number | null | undefined, label: string) {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < 0) {
    throw new Error(`${label}未知，无法确认运行`)
  }
  return value
}
