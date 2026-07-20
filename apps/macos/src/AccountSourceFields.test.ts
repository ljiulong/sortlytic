import { describe, expect, it } from 'vitest'
import type {
  AccountCollectionCapabilityView,
  AccountSourceCapabilityView,
} from './backend-api'
import {
  accountSourceFilterCapabilities,
  reconcileAccountFields,
  sourceInputCopy,
} from './account-source-rules'

function source(
  key: string,
  inputKind: AccountSourceCapabilityView['input_kind'],
): AccountSourceCapabilityView {
  return {
    key,
    display_name: key,
    description: key,
    input_kind: inputKind,
    endpoint_key: `tiktok.${key}`,
    pagination_mode: 'cursor',
    max_page_size: 20,
    max_request_count: 100,
  }
}

const capability: AccountCollectionCapabilityView = {
  catalog_version: 1,
  platform: 'tiktok',
  display_name: 'TikTok',
  account_sources: [],
  field_groups: [],
  fields: [
    {
      key: 'avatar_url',
      group: 'profile',
      display_name: '头像',
      description: '头像',
      value_type: 'text',
      availability: 'direct',
      default_selected: true,
      required_operation_keys: [],
    },
    {
      key: 'country_region',
      group: 'profile',
      display_name: '国家或地区',
      description: '国家或地区',
      value_type: 'text',
      availability: 'enrichment',
      default_selected: true,
      required_operation_keys: ['enrich.account_country'],
    },
    {
      key: 'gender',
      group: 'demographics',
      display_name: '性别',
      description: '性别',
      value_type: 'text',
      availability: 'unsupported',
      default_selected: false,
      required_operation_keys: [],
    },
  ],
}

describe('AccountSourceFields state rules', () => {
  it('按来源输入类型提供明确标签', () => {
    expect(sourceInputCopy(source('user_search', 'keyword')).label).toBe('关键词')
    expect(sourceInputCopy(source('comment_authors', 'item')).label).toContain('作品')
    expect(sourceInputCopy(source('followers', 'account')).label).toBe('种子账号 ID/链接')
    expect(sourceInputCopy(source('direct_account', 'account')).label).toContain('用户名')
  })

  it('未自定义时应用核心预设且不选择不支持字段', () => {
    expect(reconcileAccountFields(capability, [], false)).toEqual({
      fields: ['avatar_url', 'country_region'],
      removedCount: 0,
    })
  })

  it('已自定义时只保留新平台支持的交集并返回移除数量', () => {
    expect(reconcileAccountFields(
      capability,
      ['avatar_url', 'gender', 'unknown_field'],
      true,
    )).toEqual({
      fields: ['avatar_url'],
      removedCount: 2,
    })
  })

  it('按当前账号来源返回筛选能力且未知能力默认关闭', () => {
    const sourceCapability = {
      ...source('user_search', 'keyword'),
      region_filter: 'unsupported' as const,
      time_range_filter: 'local' as const,
      time_ranges: ['1', '7', '30', '180'],
    }
    const withSource = {
      ...capability,
      platform: 'xiaohongshu',
      account_sources: [sourceCapability],
    }

    expect(accountSourceFilterCapabilities(withSource, 'user_search')).toEqual({
      regionFilter: 'unsupported',
      timeRangeFilter: 'local',
      timeRanges: ['1', '7', '30', '180'],
    })
    expect(accountSourceFilterCapabilities(withSource, 'direct_account')).toEqual({
      regionFilter: 'unsupported',
      timeRangeFilter: 'unsupported',
      timeRanges: [],
    })
  })
})
