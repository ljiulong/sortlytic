import type { CollectionTaskView } from './backend-api'
import type { DataType, Platform, TaskStatus } from './workbench-data'

export function mapTaskRow(task: CollectionTaskView) {
  const platforms = stringArrayFromJson(task.platforms_json)
  const dataTypes = stringArrayFromJson(task.data_types_json)
  const requestCount = numberFromJson(task.cost_estimate_json)

  return {
    id: task.id,
    name: task.name,
    platform: toUiPlatform(platforms[0] ?? 'xiaohongshu'),
    status: toUiTaskStatus(task.status),
    source: task.source_type === 'natural_language' ? '自然语言' : '表单式',
    progress: progressForTaskStatus(task.status),
    records: 0,
    cost: `${requestCount ? `预计 ${requestCount} 次请求` : '尚无请求估算'} · ${toUiDataType(dataTypes[0] ?? 'comments')}`,
  } as const
}

export function toUiPlatform(platform: string): Platform {
  if (platform === 'tiktok') return 'TikTok'
  if (platform === 'douyin') return '抖音'
  return '小红书'
}

export function toUiDataType(dataType: string): DataType {
  if (dataType === 'keyword_search') return '搜索结果账号'
  if (dataType === 'account_profile') return '账号公开信息'
  if (dataType === 'item_detail') return '作品/笔记作者'
  if (dataType === 'account_posts') return '账号作品所属账号'
  return '评论用户'
}

export function toUiTaskStatus(status: string): TaskStatus {
  if (status === 'success') return '成功'
  if (status === 'partial_success') return '部分成功'
  if (status === 'failed' || status === 'cancelled') return '失败'
  if (status === 'queued') return '已排队'
  if (status === 'waiting_confirmation') return '等待确认'
  if (status === 'draft') return '待人工确认'
  return '运行中'
}

export function numberFromJson(value: Record<string, unknown>) {
  const estimate = value.request_count_estimate
  return typeof estimate === 'number' ? estimate : 0
}

export function stringArrayFromJson(value: unknown) {
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === 'string')
  }
  return []
}

function progressForTaskStatus(status: string) {
  return ['success', 'partial_success'].includes(status) ? 100 : 0
}
