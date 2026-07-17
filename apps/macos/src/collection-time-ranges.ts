import { useEffect, useState } from 'react'
import {
  listPlatformDataTypes,
  type CollectionDataTypeCapabilityView,
} from './backend-api'
import type { Platform } from './workbench-data'

type TimeRangeCapability = Pick<
  CollectionDataTypeCapabilityView,
  'data_type' | 'provider_time_ranges'
>

export type TimeRangeCapabilityLoader = (
  platform: string,
) => Promise<TimeRangeCapability[]>

const backendPlatformCodes: Record<Platform, string> = {
  TikTok: 'tiktok',
  抖音: 'douyin',
  小红书: 'xiaohongshu',
}

export function extractProviderTimeRanges(
  capabilities: readonly TimeRangeCapability[],
) {
  const values = capabilities
    .find(({ data_type: dataType }) => dataType === 'keyword_search')
    ?.provider_time_ranges ?? []

  return [...new Set(values)]
    .filter((value) => /^\d+$/u.test(value) && Number(value) > 0)
    .sort((left, right) => Number(left) - Number(right))
}

export async function loadPlatformTimeRanges(
  platform: Platform,
  loader: TimeRangeCapabilityLoader = listPlatformDataTypes,
) {
  const values = extractProviderTimeRanges(
    await loader(backendPlatformCodes[platform]),
  )
  if (values.length === 0) {
    throw new Error('TIME_RANGE_CAPABILITY_UNAVAILABLE')
  }
  return values
}

export function useCollectionTimeRanges(
  platform?: Platform,
  loader: TimeRangeCapabilityLoader = listPlatformDataTypes,
) {
  const [values, setValues] = useState<string[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string>()

  useEffect(() => {
    let isCurrent = true
    setValues([])
    setError(undefined)
    if (!platform) {
      setIsLoading(false)
      return () => {
        isCurrent = false
      }
    }

    setIsLoading(true)
    void loadPlatformTimeRanges(platform, loader)
      .then((nextValues) => {
        if (!isCurrent) return
        setValues(nextValues)
        setIsLoading(false)
      })
      .catch(() => {
        if (!isCurrent) return
        setError('TIME_RANGE_CAPABILITY_UNAVAILABLE')
        setIsLoading(false)
      })

    return () => {
      isCurrent = false
    }
  }, [loader, platform])

  return { error, isLoading, values }
}
