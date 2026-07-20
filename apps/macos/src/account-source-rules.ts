import type {
  AccountCollectionCapabilityView,
  AccountSourceCapabilityView,
  FilterExecution,
} from './backend-api'

export type AccountSourceFilterCapabilities = {
  regionFilter: FilterExecution
  timeRangeFilter: FilterExecution
  timeRanges: string[]
}

export function accountSourceFilterCapabilities(
  capability?: AccountCollectionCapabilityView,
  sourceKey?: string,
): AccountSourceFilterCapabilities {
  const source = capability?.account_sources.find((candidate) => candidate.key === sourceKey)
  const regionFilter = normalizeFilterExecution(source?.region_filter)
  const timeRangeFilter = normalizeFilterExecution(source?.time_range_filter)
  const timeRanges = timeRangeFilter === 'unsupported'
    ? []
    : [...new Set(source?.time_ranges?.filter((value) => ['1', '7', '30', '180'].includes(value)) ?? [])]
  return { regionFilter, timeRangeFilter, timeRanges }
}

function normalizeFilterExecution(value?: string): FilterExecution {
  return value === 'provider' || value === 'local' ? value : 'unsupported'
}

export function sourceInputCopy(source?: AccountSourceCapabilityView) {
  if (!source) return { label: '来源参数', placeholder: '请先选择账号来源' }
  if (source.input_kind === 'keyword') {
    return { label: '关键词', placeholder: '输入用于搜索账号或内容的关键词' }
  }
  if (source.input_kind === 'item') {
    return {
      label: '作品、视频或笔记 ID/链接',
      placeholder: '输入公开作品、视频或笔记 ID/链接',
    }
  }
  if (['followers', 'followings', 'similar_accounts'].includes(source.key)) {
    return { label: '种子账号 ID/链接', placeholder: '输入公开种子账号 ID/链接' }
  }
  return {
    label: '账号 ID、用户名或主页链接',
    placeholder: '输入公开账号 ID、用户名或主页链接',
  }
}

export function reconcileAccountFields(
  capability: AccountCollectionCapabilityView,
  selectedFields: readonly string[],
  customized: boolean,
) {
  const available = capability.fields.filter((field) => field.availability !== 'unsupported')
  const next = customized
    ? available.filter((field) => selectedFields.includes(field.key)).map((field) => field.key)
    : available.filter((field) => field.default_selected).map((field) => field.key)
  return {
    fields: next,
    removedCount: selectedFields.filter((key) => !next.includes(key)).length,
  }
}
