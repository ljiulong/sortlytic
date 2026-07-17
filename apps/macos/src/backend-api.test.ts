import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  checkForAppUpdate,
  getCurrentAppVersion,
  prepareAppUpdate,
  relaunchAfterAppUpdate,
} from './backend-api'

const getVersionMock = vi.hoisted(() => vi.fn())
const updaterCheckMock = vi.hoisted(() => vi.fn())
const updaterInstallMock = vi.hoisted(() => vi.fn())
const relaunchMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/app', () => ({ getVersion: getVersionMock }))
vi.mock('@tauri-apps/plugin-updater', () => ({ check: updaterCheckMock }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: relaunchMock }))

beforeEach(() => {
  vi.unstubAllGlobals()
  getVersionMock.mockReset()
  updaterCheckMock.mockReset()
  updaterInstallMock.mockReset()
  relaunchMock.mockReset()
})

describe('application update API boundaries', () => {
  it('returns null for the current version outside a Tauri runtime', async () => {
    await expect(getCurrentAppVersion()).resolves.toBeNull()

    expect(getVersionMock).not.toHaveBeenCalled()
  })

  it('loads the configured application version inside a Tauri runtime', async () => {
    vi.stubGlobal('window', { __TAURI_INTERNALS__: {} })
    getVersionMock.mockResolvedValue('0.3.0')

    await expect(getCurrentAppVersion()).resolves.toBe('0.3.0')

    expect(getVersionMock).toHaveBeenCalledOnce()
  })

  it('keeps update metadata while preparing without relaunching', async () => {
    updaterCheckMock.mockResolvedValue({
      version: '0.3.1',
      date: '2026-07-18T08:00:00Z',
      body: 'Stability improvements',
      downloadAndInstall: updaterInstallMock,
    })
    updaterInstallMock.mockResolvedValue(undefined)

    await expect(checkForAppUpdate()).resolves.toEqual({
      version: '0.3.1',
      date: '2026-07-18T08:00:00Z',
      body: 'Stability improvements',
    })
    await prepareAppUpdate()

    expect(updaterInstallMock).toHaveBeenCalledOnce()
    expect(relaunchMock).not.toHaveBeenCalled()
  })

  it('relaunches only through the explicit relaunch operation', async () => {
    relaunchMock.mockResolvedValue(undefined)

    await relaunchAfterAppUpdate()

    expect(relaunchMock).toHaveBeenCalledOnce()
    expect(updaterInstallMock).not.toHaveBeenCalled()
  })

  it('rejects update preparation when no update is pending', async () => {
    updaterCheckMock.mockResolvedValue(null)

    await expect(checkForAppUpdate()).resolves.toBeNull()
    await expect(prepareAppUpdate()).rejects.toThrow('请先检查更新')
    expect(updaterInstallMock).not.toHaveBeenCalled()
    expect(relaunchMock).not.toHaveBeenCalled()
  })
})
