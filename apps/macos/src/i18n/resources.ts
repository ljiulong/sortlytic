import type { AppLanguage } from './index'

export type TranslationNamespace = Record<string, string>
export type TranslationBundle = Record<AppLanguage, TranslationNamespace>

const namespaceNames = [
  'common',
  'navigation',
  'dashboard',
  'collection',
  'tasks',
  'settings',
  'guide',
  'updates',
  'messages',
] as const

export type TranslationNamespaceName = (typeof namespaceNames)[number]
export type LocaleResources = Record<TranslationNamespaceName, TranslationNamespace>

const bundles = import.meta.glob<TranslationBundle>('./resource-bundles/*.ts', {
  eager: true,
  import: 'default',
})

function buildLocale(language: AppLanguage): LocaleResources {
  return Object.fromEntries(
    namespaceNames.map((namespace) => {
      const bundle = bundles[`./resource-bundles/${namespace}.ts`]
      return [namespace, bundle?.[language] ?? {}]
    }),
  ) as LocaleResources
}

export const resources = {
  'zh-CN': buildLocale('zh-CN'),
  'en-US': buildLocale('en-US'),
} satisfies Record<AppLanguage, LocaleResources>
