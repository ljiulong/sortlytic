import { useEffect, useState } from 'react'
import {
  getAccountCollectionCapabilities,
  type AccountCollectionCapabilityView,
} from './backend-api'
import type { Platform } from './workbench-data'

export type AccountCapabilityLoader = (
  platform: string,
) => Promise<AccountCollectionCapabilityView>

const backendPlatformCodes: Record<Platform, string> = {
  TikTok: 'tiktok',
  抖音: 'douyin',
  小红书: 'xiaohongshu',
}

export function loadAccountCapabilities(
  platform: Platform,
  loader: AccountCapabilityLoader = getAccountCollectionCapabilities,
) {
  return loader(backendPlatformCodes[platform])
}

export function useAccountCapabilities(
  platform?: Platform,
  loader: AccountCapabilityLoader = getAccountCollectionCapabilities,
) {
  const [capability, setCapability] = useState<AccountCollectionCapabilityView>()
  const [isLoading, setIsLoading] = useState(false)
  const [error, setError] = useState<string>()

  useEffect(() => {
    let isCurrent = true
    setCapability(undefined)
    setError(undefined)
    if (!platform) {
      setIsLoading(false)
      return () => {
        isCurrent = false
      }
    }

    setIsLoading(true)
    void loadAccountCapabilities(platform, loader)
      .then((nextCapability) => {
        if (!isCurrent) return
        setCapability(nextCapability)
        setIsLoading(false)
      })
      .catch(() => {
        if (!isCurrent) return
        setError('ACCOUNT_CAPABILITY_UNAVAILABLE')
        setIsLoading(false)
      })

    return () => {
      isCurrent = false
    }
  }, [loader, platform])

  return {
    capability,
    error,
    isEmpty: Boolean(
      capability
      && (capability.account_sources.length === 0 || capability.fields.length === 0),
    ),
    isLoading,
  }
}
