import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  defaultUpdatePreferences,
  readUpdatePreferences,
  saveUpdatePreferences,
  updatePreferencesStorageKey,
} from './update-preferences'

const originalStorageDescriptor = Object.getOwnPropertyDescriptor(
  globalThis,
  'localStorage',
)
const storedValues = new Map<string, string>()

function installWorkingStorage() {
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      clear: () => storedValues.clear(),
      getItem: (key: string) => storedValues.get(key) ?? null,
      setItem: (key: string, value: string) => storedValues.set(key, value),
    },
  })
}

function restoreOriginalStorage() {
  if (originalStorageDescriptor) {
    Object.defineProperty(globalThis, 'localStorage', originalStorageDescriptor)
  } else {
    Reflect.deleteProperty(globalThis, 'localStorage')
  }
}

describe('update preferences', () => {
  beforeEach(() => {
    storedValues.clear()
    installWorkingStorage()
  })

  afterEach(() => {
    restoreOriginalStorage()
  })

  it('defaults to automatic checks enabled and automatic downloads disabled', () => {
    expect(updatePreferencesStorageKey).toBe('sortlytic-update-preferences-v1')
    expect(defaultUpdatePreferences).toEqual({
      autoCheck: true,
      autoDownload: false,
    })
    expect(readUpdatePreferences()).toEqual(defaultUpdatePreferences)
  })

  it('persists and restores valid preferences', () => {
    saveUpdatePreferences({ autoCheck: true, autoDownload: true })

    expect(storedValues.get(updatePreferencesStorageKey)).toBe(
      JSON.stringify({ autoCheck: true, autoDownload: true }),
    )
    expect(readUpdatePreferences()).toEqual({
      autoCheck: true,
      autoDownload: true,
    })
  })

  it.each([
    ['invalid JSON', '{invalid'],
    ['non-object JSON', 'null'],
    ['missing field', JSON.stringify({ autoCheck: true })],
    ['invalid field type', JSON.stringify({ autoCheck: 'yes', autoDownload: false })],
    ['invalid dependency', JSON.stringify({ autoCheck: false, autoDownload: true })],
  ])('restores defaults for %s', (_label, storedValue) => {
    storedValues.set(updatePreferencesStorageKey, storedValue)

    expect(readUpdatePreferences()).toEqual(defaultUpdatePreferences)
  })

  it('stays usable when localStorage access fails', () => {
    Object.defineProperty(globalThis, 'localStorage', {
      configurable: true,
      value: {
        getItem: () => {
          throw new Error('storage unavailable')
        },
        setItem: () => {
          throw new Error('storage unavailable')
        },
      },
    })

    expect(readUpdatePreferences()).toEqual(defaultUpdatePreferences)
    expect(() => saveUpdatePreferences({
      autoCheck: true,
      autoDownload: true,
    })).not.toThrow()
  })

  it('turns automatic downloads off when automatic checks are disabled', () => {
    saveUpdatePreferences({ autoCheck: true, autoDownload: true })
    saveUpdatePreferences({ autoCheck: false, autoDownload: true })

    expect(readUpdatePreferences()).toEqual({
      autoCheck: false,
      autoDownload: false,
    })
  })

  it('does not allow automatic downloads to be enabled without automatic checks', () => {
    saveUpdatePreferences({ autoCheck: false, autoDownload: true })

    expect(storedValues.get(updatePreferencesStorageKey)).toBe(
      JSON.stringify({ autoCheck: false, autoDownload: false }),
    )
  })
})
