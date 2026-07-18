// @vitest-environment happy-dom

import { act, createElement } from 'react'
import { createRoot, type Root } from 'react-dom/client'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { useAppUpdater } from './use-app-updater'

const backendMocks = vi.hoisted(() => ({
  checkForAppUpdate: vi.fn(),
  getCurrentAppVersion: vi.fn(),
  prepareAppUpdate: vi.fn(),
  relaunchAfterAppUpdate: vi.fn(),
}))
const preferenceMocks = vi.hoisted(() => ({
  current: { autoCheck: false, autoDownload: false },
  readUpdatePreferences: vi.fn(),
  saveUpdatePreferences: vi.fn(),
}))

vi.mock('./backend-api', () => ({
  backendErrorMessage: (error: unknown) => error instanceof Error
    ? error.message
    : String(error),
  ...backendMocks,
}))

vi.mock('./update-preferences', () => ({
  readUpdatePreferences: preferenceMocks.readUpdatePreferences,
  saveUpdatePreferences: preferenceMocks.saveUpdatePreferences,
}))

type HookValue = ReturnType<typeof useAppUpdater>

const mountedRoots = new Set<{ container: HTMLDivElement; root: Root }>()

function createDeferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, reject, resolve }
}

async function flushUpdates() {
  await act(async () => {
    for (let index = 0; index < 8; index += 1) {
      await Promise.resolve()
    }
  })
}

async function mountUpdater() {
  let current: HookValue | undefined

  function Probe() {
    current = useAppUpdater()
    return null
  }

  const container = document.createElement('div')
  const root = createRoot(container)
  mountedRoots.add({ container, root })
  document.body.append(container)

  await act(async () => {
    root.render(createElement(Probe))
  })
  await flushUpdates()

  return {
    get current() {
      if (!current) throw new Error('Updater Hook 未完成渲染')
      return current
    },
  }
}

beforeEach(() => {
  ;(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean })
    .IS_REACT_ACT_ENVIRONMENT = true
  preferenceMocks.current = { autoCheck: false, autoDownload: false }
  preferenceMocks.readUpdatePreferences.mockReset()
  preferenceMocks.readUpdatePreferences.mockImplementation(
    () => ({ ...preferenceMocks.current }),
  )
  preferenceMocks.saveUpdatePreferences.mockReset()
  backendMocks.checkForAppUpdate.mockReset()
  backendMocks.getCurrentAppVersion.mockReset()
  backendMocks.getCurrentAppVersion.mockResolvedValue('0.3.0')
  backendMocks.prepareAppUpdate.mockReset()
  backendMocks.prepareAppUpdate.mockResolvedValue(undefined)
  backendMocks.relaunchAfterAppUpdate.mockReset()
  backendMocks.relaunchAfterAppUpdate.mockResolvedValue(undefined)
})

afterEach(() => {
  for (const mounted of mountedRoots) {
    act(() => mounted.root.unmount())
    mounted.container.remove()
  }
  mountedRoots.clear()
})

describe.sequential('useAppUpdater', () => {
  it('checks once per application session, downloads automatically, and never relaunches', async () => {
    preferenceMocks.current = { autoCheck: true, autoDownload: true }
    backendMocks.getCurrentAppVersion.mockResolvedValueOnce(null)
    backendMocks.getCurrentAppVersion.mockResolvedValue('0.3.0')
    backendMocks.checkForAppUpdate.mockResolvedValue({
      version: '0.3.1',
      body: 'Stability improvements',
    })

    const browserHook = await mountUpdater()
    expect(browserHook.current.currentVersion).toBeNull()
    expect(backendMocks.checkForAppUpdate).not.toHaveBeenCalled()

    const firstAppHook = await mountUpdater()
    expect(firstAppHook.current.currentVersion).toBe('0.3.0')
    expect(firstAppHook.current.phase).toBe('ready')
    expect(backendMocks.checkForAppUpdate).toHaveBeenCalledOnce()
    expect(backendMocks.prepareAppUpdate).toHaveBeenCalledOnce()
    expect(backendMocks.relaunchAfterAppUpdate).not.toHaveBeenCalled()

    await mountUpdater()
    expect(backendMocks.checkForAppUpdate).toHaveBeenCalledOnce()
    expect(backendMocks.prepareAppUpdate).toHaveBeenCalledOnce()
  })

  it('loads the current version and supports a manual update check', async () => {
    backendMocks.checkForAppUpdate.mockResolvedValue({ version: '0.3.2' })
    const mounted = await mountUpdater()

    let result
    await act(async () => {
      result = await mounted.current.checkForUpdate()
    })

    expect(result).toEqual({ version: '0.3.2' })
    expect(mounted.current.currentVersion).toBe('0.3.0')
    expect(mounted.current.phase).toBe('available')
    expect(mounted.current.update).not.toBeUndefined()
    expect(mounted.current.update).toEqual({ version: '0.3.2' })
  })

  it('runs concurrent manual checks as a single operation', async () => {
    const deferred = createDeferred<null>()
    backendMocks.checkForAppUpdate.mockReturnValue(deferred.promise)
    const mounted = await mountUpdater()
    let first!: Promise<unknown>
    let second!: Promise<unknown>

    act(() => {
      first = mounted.current.checkForUpdate()
      second = mounted.current.checkForUpdate()
    })

    expect(first).toBe(second)
    expect(backendMocks.checkForAppUpdate).toHaveBeenCalledOnce()

    await act(async () => {
      deferred.resolve(null)
      await Promise.all([first, second])
    })
    expect(mounted.current.phase).toBe('latest')
  })

  it('prepares concurrent update requests once without relaunching', async () => {
    backendMocks.checkForAppUpdate.mockResolvedValue({ version: '0.3.2' })
    const prepareDeferred = createDeferred<void>()
    backendMocks.prepareAppUpdate.mockReturnValue(prepareDeferred.promise)
    const mounted = await mountUpdater()

    await act(async () => {
      await mounted.current.checkForUpdate()
    })

    let first!: Promise<void>
    let second!: Promise<void>
    act(() => {
      first = mounted.current.prepareUpdate()
      second = mounted.current.prepareUpdate()
    })

    expect(first).toBe(second)
    expect(backendMocks.prepareAppUpdate).toHaveBeenCalledOnce()
    expect(backendMocks.relaunchAfterAppUpdate).not.toHaveBeenCalled()

    await act(async () => {
      prepareDeferred.resolve()
      await Promise.all([first, second])
    })
    expect(mounted.current.phase).toBe('ready')
  })

  it('runs relaunch as a single operation and allows retry after failure', async () => {
    backendMocks.checkForAppUpdate.mockResolvedValue({ version: '0.3.2' })
    const mounted = await mountUpdater()
    await act(async () => {
      await mounted.current.checkForUpdate()
      await mounted.current.prepareUpdate()
    })

    const relaunchDeferred = createDeferred<void>()
    backendMocks.relaunchAfterAppUpdate.mockReturnValueOnce(relaunchDeferred.promise)
    let first!: Promise<void>
    let second!: Promise<void>
    act(() => {
      first = mounted.current.relaunchToUpdate()
      second = mounted.current.relaunchToUpdate()
    })

    expect(first).toBe(second)
    expect(backendMocks.relaunchAfterAppUpdate).toHaveBeenCalledOnce()

    await act(async () => {
      relaunchDeferred.reject(new Error('relaunch failed'))
      await expect(first).rejects.toThrow('relaunch failed')
    })
    expect(mounted.current.phase).toBe('error')

    await act(async () => {
      await mounted.current.relaunchToUpdate()
    })
    expect(backendMocks.relaunchAfterAppUpdate).toHaveBeenCalledTimes(2)
  })

  it('allows check and preparation retries after failures', async () => {
    backendMocks.checkForAppUpdate
      .mockRejectedValueOnce(new Error('check failed'))
      .mockResolvedValueOnce({ version: '0.3.2' })
    backendMocks.prepareAppUpdate
      .mockRejectedValueOnce(new Error('download failed'))
      .mockResolvedValueOnce(undefined)
    const mounted = await mountUpdater()

    await act(async () => {
      await expect(mounted.current.checkForUpdate()).rejects.toThrow('check failed')
    })
    expect(mounted.current.phase).toBe('error')

    await act(async () => {
      await mounted.current.checkForUpdate()
    })
    await act(async () => {
      await expect(mounted.current.prepareUpdate()).rejects.toThrow('download failed')
    })
    expect(mounted.current.phase).toBe('error')

    await act(async () => {
      await mounted.current.prepareUpdate()
    })
    expect(mounted.current.phase).toBe('ready')
    expect(backendMocks.checkForAppUpdate).toHaveBeenCalledTimes(2)
    expect(backendMocks.prepareAppUpdate).toHaveBeenCalledTimes(2)
  })

  it('persists dependent automatic update preferences safely', async () => {
    preferenceMocks.current = { autoCheck: true, autoDownload: true }
    const mounted = await mountUpdater()

    act(() => mounted.current.setAutoCheck(false))
    expect(mounted.current.preferences).toEqual({
      autoCheck: false,
      autoDownload: false,
    })
    expect(preferenceMocks.saveUpdatePreferences).toHaveBeenLastCalledWith({
      autoCheck: false,
      autoDownload: false,
    })

    act(() => mounted.current.setAutoDownload(true))
    expect(mounted.current.preferences.autoDownload).toBe(false)

    act(() => {
      mounted.current.setAutoCheck(true)
      mounted.current.setAutoDownload(true)
    })
    expect(mounted.current.preferences).toEqual({
      autoCheck: true,
      autoDownload: true,
    })
  })
})
