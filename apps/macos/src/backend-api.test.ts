import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  checkForAppUpdate,
  getAiRun,
  getCurrentAppVersion,
  getTask,
  listLatestTaskIntents,
  listAiRuns,
  prepareAppUpdate,
  relaunchAfterAppUpdate,
  reviseCollectionTask,
} from './backend-api'

const invokeMock = vi.hoisted(() => vi.fn())
const getVersionMock = vi.hoisted(() => vi.fn())
const updaterCheckMock = vi.hoisted(() => vi.fn())
const updaterInstallMock = vi.hoisted(() => vi.fn())
const relaunchMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/api/app', () => ({ getVersion: getVersionMock }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))
vi.mock('@tauri-apps/plugin-updater', () => ({ check: updaterCheckMock }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: relaunchMock }))

beforeEach(() => {
  vi.unstubAllGlobals()
  getVersionMock.mockReset()
  updaterCheckMock.mockReset()
  updaterInstallMock.mockReset()
  relaunchMock.mockReset()
  invokeMock.mockReset()
})

describe('natural parse attempt API boundary', () => {
  it('loads all latest attempts with one active-workspace command', async () => {
    invokeMock.mockResolvedValue([])

    await expect(listLatestTaskIntents()).resolves.toEqual([])

    expect(invokeMock).toHaveBeenCalledWith('list_latest_task_intents', { rootPath: null })
  })
})

describe('task revision API boundary', () => {
  it('loads the complete persisted task before editing', async () => {
    invokeMock.mockResolvedValue({ id: 'task-1' })

    await getTask('task-1')

    expect(invokeMock).toHaveBeenCalledWith('get_task', {
      taskId: 'task-1',
      rootPath: null,
    })
  })

  it('passes the complete user-edited plan to the active workspace command', async () => {
    invokeMock.mockResolvedValue({ task: { id: 'task-1' }, collection_plan: { id: 'plan-2' } })
    const input = {
      task_id: 'task-1',
      name: '英国宠物账号',
      platforms: ['tiktok'],
      data_types: ['account'],
      source: 'user_edited' as const,
      plan_json: {
        schema_version: 4,
        account_source: 'user_search',
        region: 'GB',
        query_locale: 'en-GB',
      },
    }

    await reviseCollectionTask(input)

    expect(invokeMock).toHaveBeenCalledWith('revise_collection_task', {
      input,
      rootPath: null,
    })
  })
})

describe('AI run diagnostics API boundary', () => {
  it('loads one parsed intent by its persisted run id', async () => {
    invokeMock.mockResolvedValue({ id: 'ai-run-1' })

    await getAiRun('ai-run-1')

    expect(invokeMock).toHaveBeenCalledWith('get_ai_run', {
      aiRunId: 'ai-run-1',
      rootPath: null,
    })
  })

  it('lists all natural-language intent runs for a task', async () => {
    invokeMock.mockResolvedValue([])

    await listAiRuns('task-1', 'collection_intent_generation')

    expect(invokeMock).toHaveBeenCalledWith('list_ai_runs', {
      taskId: 'task-1',
      runType: 'collection_intent_generation',
      rootPath: null,
    })
  })
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
