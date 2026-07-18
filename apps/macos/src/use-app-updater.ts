import { useCallback, useEffect, useRef, useState } from 'react'
import {
  backendErrorMessage,
  checkForAppUpdate,
  getCurrentAppVersion,
  prepareAppUpdate,
  relaunchAfterAppUpdate,
  type AppUpdateInfo,
} from './backend-api'
import {
  readUpdatePreferences,
  saveUpdatePreferences,
  type UpdatePreferences,
} from './update-preferences'

export type AppUpdatePhase =
  | 'idle'
  | 'checking'
  | 'latest'
  | 'available'
  | 'preparing'
  | 'ready'
  | 'relaunching'
  | 'error'

type UpdateOperation = 'check' | 'prepare' | 'relaunch'

type ActiveUpdateOperation = {
  kind: UpdateOperation
  promise: Promise<unknown>
}

let automaticCheckStartedThisSession = false

export function useAppUpdater() {
  const [currentVersion, setCurrentVersion] = useState<string | null>(null)
  const [hasLoadedCurrentVersion, setHasLoadedCurrentVersion] = useState(false)
  const [update, setUpdate] = useState<AppUpdateInfo | null | undefined>(undefined)
  const [phase, setPhase] = useState<AppUpdatePhase>('idle')
  const [error, setError] = useState<string>()
  const [preferences, setPreferences] = useState<UpdatePreferences>(
    readUpdatePreferences,
  )
  const activeOperationRef = useRef<ActiveUpdateOperation | null>(null)
  const mountedRef = useRef(true)
  const preferencesRef = useRef(preferences)
  const updateRef = useRef<AppUpdateInfo | null | undefined>(undefined)
  const isPreparedRef = useRef(false)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  const transitionTo = useCallback((nextPhase: AppUpdatePhase) => {
    if (mountedRef.current) setPhase(nextPhase)
  }, [])

  const clearError = useCallback(() => {
    if (mountedRef.current) setError(undefined)
  }, [])

  const reportError = useCallback((cause: unknown) => {
    if (mountedRef.current) setError(backendErrorMessage(cause))
    transitionTo('error')
  }, [transitionTo])

  const setAvailableUpdate = useCallback((nextUpdate: AppUpdateInfo | null) => {
    updateRef.current = nextUpdate
    if (mountedRef.current) setUpdate(nextUpdate)
  }, [])

  const runSingleFlight = useCallback(function runSingleFlight<T>(
    kind: UpdateOperation,
    operation: () => Promise<T>,
  ): Promise<T> {
    const activeOperation = activeOperationRef.current
    if (activeOperation) {
      if (activeOperation.kind === kind) {
        return activeOperation.promise as Promise<T>
      }
      return Promise.reject(new Error('更新操作正在进行'))
    }

    let operationPromise: Promise<T>
    operationPromise = operation().finally(() => {
      if (activeOperationRef.current?.promise === operationPromise) {
        activeOperationRef.current = null
      }
    })
    activeOperationRef.current = { kind, promise: operationPromise }
    return operationPromise
  }, [])

  const performPrepare = useCallback(async (): Promise<void> => {
    if (!updateRef.current) throw new Error('请先检查更新')

    transitionTo('preparing')
    clearError()
    try {
      await prepareAppUpdate()
      isPreparedRef.current = true
      transitionTo('ready')
    } catch (cause) {
      reportError(cause)
      throw cause
    }
  }, [clearError, reportError, transitionTo])

  const performCheck = useCallback(async (
    prepareAutomatically: boolean,
  ): Promise<AppUpdateInfo | null> => {
    transitionTo('checking')
    clearError()
    try {
      const nextUpdate = await checkForAppUpdate()
      setAvailableUpdate(nextUpdate)
      isPreparedRef.current = false
      transitionTo(nextUpdate ? 'available' : 'latest')

      if (
        nextUpdate
        && prepareAutomatically
        && preferencesRef.current.autoDownload
      ) {
        await performPrepare()
      }
      return nextUpdate
    } catch (cause) {
      reportError(cause)
      throw cause
    }
  }, [clearError, performPrepare, reportError, setAvailableUpdate, transitionTo])

  const runCheck = useCallback((prepareAutomatically: boolean) => (
    runSingleFlight('check', () => performCheck(prepareAutomatically))
  ), [performCheck, runSingleFlight])

  const checkForUpdate = useCallback(
    () => runCheck(false),
    [runCheck],
  )

  const prepareUpdate = useCallback(
    () => runSingleFlight('prepare', performPrepare),
    [performPrepare, runSingleFlight],
  )

  const relaunchToUpdate = useCallback(() => runSingleFlight(
    'relaunch',
    async () => {
      if (!isPreparedRef.current) throw new Error('更新尚未准备完成')

      transitionTo('relaunching')
      clearError()
      try {
        await relaunchAfterAppUpdate()
        transitionTo('ready')
      } catch (cause) {
        reportError(cause)
        throw cause
      }
    },
  ), [clearError, reportError, runSingleFlight, transitionTo])

  const persistPreferences = useCallback((nextPreferences: UpdatePreferences) => {
    preferencesRef.current = nextPreferences
    if (mountedRef.current) setPreferences(nextPreferences)
    saveUpdatePreferences(nextPreferences)
  }, [])

  const setAutoCheck = useCallback((enabled: boolean) => {
    persistPreferences({
      autoCheck: enabled,
      autoDownload: enabled && preferencesRef.current.autoDownload,
    })
  }, [persistPreferences])

  const setAutoDownload = useCallback((enabled: boolean) => {
    persistPreferences({
      autoCheck: preferencesRef.current.autoCheck,
      autoDownload: preferencesRef.current.autoCheck && enabled,
    })
  }, [persistPreferences])

  useEffect(() => {
    let cancelled = false

    void getCurrentAppVersion()
      .then((version) => {
        if (cancelled || !mountedRef.current) return
        setCurrentVersion(version)
        setHasLoadedCurrentVersion(true)
      })
      .catch(() => {
        if (cancelled || !mountedRef.current) return
        setCurrentVersion(null)
        setHasLoadedCurrentVersion(true)
      })

    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    if (
      !hasLoadedCurrentVersion
      || currentVersion === null
      || !preferences.autoCheck
      || automaticCheckStartedThisSession
    ) {
      return
    }

    automaticCheckStartedThisSession = true
    void runCheck(true).catch(() => undefined)
  }, [currentVersion, hasLoadedCurrentVersion, preferences.autoCheck, runCheck])

  const isUpdateBusy = phase === 'checking'
    || phase === 'preparing'
    || phase === 'relaunching'

  return {
    currentVersion,
    update,
    phase,
    preferences,
    error,
    isUpdateBusy,
    setAutoCheck,
    setAutoDownload,
    checkForUpdate,
    prepareUpdate,
    relaunchToUpdate,
  }
}
