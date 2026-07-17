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
    setNavigatorLanguage(originalNavigatorLanguage)
  })

  afterEach(() => {
    globalThis.localStorage.clear()
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

  it('persists a valid language after changing the current session', async () => {
    await changeAppLanguage('en-US')

    expect(i18n.language).toBe('en-US')
    expect(globalThis.localStorage.getItem(languageStorageKey)).toBe('en-US')
  })
})
