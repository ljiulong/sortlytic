import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  changeAppLanguage,
  defaultLanguage,
  detectInitialLanguage,
  i18n,
  languageStorageKey,
  normalizeLanguage,
} from './index'

const originalNavigatorLanguage = globalThis.navigator?.language ?? 'zh-CN'

function setNavigatorLanguage(language: string) {
  Object.defineProperty(globalThis, 'navigator', {
    configurable: true,
    value: { language },
  })
}

function setBrowserEnvironment(enabled: boolean) {
  if (enabled) {
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: {},
    })
  } else {
    Reflect.deleteProperty(globalThis, 'window')
  }
}

const storage = new Map<string, string>()

Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: {
    clear: () => storage.clear(),
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => storage.set(key, value),
  },
})

describe('language configuration', () => {
  beforeEach(() => {
    globalThis.localStorage.clear()
    setBrowserEnvironment(true)
    setNavigatorLanguage(originalNavigatorLanguage)
  })

  afterEach(() => {
    globalThis.localStorage.clear()
    setBrowserEnvironment(false)
    setNavigatorLanguage(originalNavigatorLanguage)
  })

  it('normalizes supported language families and falls back to Chinese', () => {
    expect(normalizeLanguage('zh')).toBe('zh-CN')
    expect(normalizeLanguage('zh-TW')).toBe('zh-CN')
    expect(normalizeLanguage('en')).toBe('en-US')
    expect(normalizeLanguage('en-GB')).toBe('en-US')
    expect(normalizeLanguage('fr-FR')).toBe(defaultLanguage)
  })

  it('prefers the saved language over the system language', () => {
    setNavigatorLanguage('en-GB')
    globalThis.localStorage.setItem(languageStorageKey, 'zh-CN')

    expect(detectInitialLanguage()).toBe('zh-CN')
  })

  it('uses the normalized system language when no language is saved', () => {
    setNavigatorLanguage('en-GB')

    expect(detectInitialLanguage()).toBe('en-US')
  })

  it('falls back to Chinese when no browser environment is available', () => {
    setBrowserEnvironment(false)
    setNavigatorLanguage('en-US')

    expect(detectInitialLanguage()).toBe('zh-CN')
  })

  it('persists a valid language after changing the current session', async () => {
    await changeAppLanguage('en-US')

    expect(i18n.language).toBe('en-US')
    expect(globalThis.localStorage.getItem(languageStorageKey)).toBe('en-US')
  })

  it('reports persistence failure while keeping the session language change', async () => {
    const originalStorage = globalThis.localStorage
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

    try {
      await expect(changeAppLanguage('en-US')).rejects.toThrow(
        'LANGUAGE_PERSISTENCE_FAILED',
      )
      expect(i18n.language).toBe('en-US')
    } finally {
      Object.defineProperty(globalThis, 'localStorage', {
        configurable: true,
        value: originalStorage,
      })
    }
  })
})
