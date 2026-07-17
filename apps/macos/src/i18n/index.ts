import i18next from 'i18next'
import { initReactI18next } from 'react-i18next'
import { resources } from './resources'

export const supportedLanguages = ['zh-CN', 'en-US'] as const
export type AppLanguage = (typeof supportedLanguages)[number]
export const defaultLanguage: AppLanguage = 'zh-CN'
export const languageStorageKey = 'sortlytic-language'

function isAppLanguage(value: string | null | undefined): value is AppLanguage {
  return value === 'zh-CN' || value === 'en-US'
}

export function normalizeLanguage(language: string | null | undefined): AppLanguage {
  const normalized = language?.trim().toLowerCase()

  if (normalized === 'zh' || normalized?.startsWith('zh-')) {
    return 'zh-CN'
  }

  if (normalized === 'en' || normalized?.startsWith('en-')) {
    return 'en-US'
  }

  return defaultLanguage
}

function readStoredLanguage(): AppLanguage | null {
  try {
    const stored = globalThis.localStorage?.getItem(languageStorageKey)
    return isAppLanguage(stored) ? stored : null
  } catch {
    return null
  }
}

export function detectInitialLanguage(): AppLanguage {
  return readStoredLanguage() ?? normalizeLanguage(globalThis.navigator?.language)
}

export const i18n = i18next

void i18n.use(initReactI18next).init({
  debug: false,
  defaultNS: 'common',
  fallbackLng: defaultLanguage,
  initAsync: false,
  interpolation: {
    escapeValue: false,
  },
  lng: detectInitialLanguage(),
  ns: Object.keys(resources[defaultLanguage]),
  resources,
  supportedLngs: [...supportedLanguages],
})

export async function changeAppLanguage(language: AppLanguage): Promise<void> {
  await i18n.changeLanguage(language)

  try {
    globalThis.localStorage?.setItem(languageStorageKey, language)
  } catch {
    // Language changes remain active for this session when storage is unavailable.
  }
}

declare module 'i18next' {
  interface CustomTypeOptions {
    defaultNS: 'common'
    resources: typeof resources['zh-CN']
  }
}
