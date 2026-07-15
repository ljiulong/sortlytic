import { useState } from 'react'
import {
  backendErrorMessage,
  checkForAppUpdate,
  installAppUpdate,
  type AppUpdateInfo,
} from './backend-api'

export function useAppUpdater() {
  const [update, setUpdate] = useState<AppUpdateInfo | null | undefined>(undefined)
  const [isChecking, setIsChecking] = useState(false)
  const [isInstalling, setIsInstalling] = useState(false)
  const [error, setError] = useState<string>()

  const checkForUpdate = async () => {
    setIsChecking(true)
    setError(undefined)
    try {
      const nextUpdate = await checkForAppUpdate()
      setUpdate(nextUpdate)
      return nextUpdate
    } catch (error) {
      const message = backendErrorMessage(error)
      setError(message)
      throw error
    } finally {
      setIsChecking(false)
    }
  }

  const installUpdate = async () => {
    if (!update) {
      throw new Error('请先检查更新')
    }
    setIsInstalling(true)
    setError(undefined)
    try {
      await installAppUpdate()
    } catch (error) {
      const message = backendErrorMessage(error)
      setError(message)
      throw error
    } finally {
      setIsInstalling(false)
    }
  }

  return {
    update,
    hasCheckedForUpdate: update !== undefined,
    updateError: error,
    isCheckingForUpdate: isChecking,
    isInstallingUpdate: isInstalling,
    isUpdateBusy: isChecking || isInstalling,
    checkForUpdate,
    installUpdate,
  }
}
