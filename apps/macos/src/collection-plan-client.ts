export type PlanParamValues = {
  regionCode: string
  keyword: string
  range: string
  maxRecords: number
  genderFilterEnabled?: boolean
  genders?: Array<'male' | 'female' | 'other'>
}

type BackendPlatform = 'tiktok' | 'douyin' | 'xiaohongshu'

type BackendDataType =
  | 'keyword_search'
  | 'comments'
  | 'account_profile'
  | 'item_detail'
  | 'account_posts'

function supportsRegionFilter(platform: BackendPlatform, dataType: BackendDataType) {
  if (platform === 'tiktok') {
    return ['keyword_search', 'account_posts', 'comments'].includes(dataType)
  }

  return ['keyword_search', 'comments'].includes(dataType)
}

function supportsProviderTimeRange(dataType: BackendDataType) {
  return dataType === 'keyword_search'
}

function supportsPageSize(platform: BackendPlatform, dataType: BackendDataType) {
  return (
    (platform === 'tiktok' && dataType === 'keyword_search') ||
    ((platform === 'tiktok' || platform === 'douyin') && dataType === 'comments')
  )
}

export function buildPlanParams(
  values: PlanParamValues,
  platform: BackendPlatform,
  dataType: BackendDataType,
) {
  const keyword = values.keyword.trim()
  const params: Record<string, string | number | string[]> = {}

  if (dataType === 'keyword_search') {
    params.keyword = keyword
  } else if (dataType === 'account_profile' || dataType === 'account_posts') {
    params.account_id = keyword
  } else {
    params.item_id = keyword
  }

  const region = values.regionCode.trim().toUpperCase()
  if (region && supportsRegionFilter(platform, dataType)) {
    params.region = region
  }

  const timeRange = values.range.trim()
  if (timeRange && supportsProviderTimeRange(dataType)) {
    params.time_range = timeRange
  }

  if (supportsPageSize(platform, dataType)) {
    params.page_size = Math.min(Math.max(values.maxRecords, 1), 50)
  }

  if (values.genderFilterEnabled && values.genders?.length) {
    params.genders = [...new Set(values.genders)]
  }

  return params
}
