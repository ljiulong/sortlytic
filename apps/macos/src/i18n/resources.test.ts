import { describe, expect, it } from 'vitest'
import { resources } from './resources'

describe('translation resources', () => {
  it('keeps the Chinese and English key sets identical in every namespace', () => {
    for (const [namespace, locales] of Object.entries(resources['zh-CN'])) {
      const chineseKeys = Object.keys(locales).sort()
      const englishKeys = Object.keys(resources['en-US'][namespace as keyof typeof resources['en-US']]).sort()

      expect(englishKeys, namespace).toEqual(chineseKeys)
    }
  })

  it('keeps the English resource values free of Chinese text', () => {
    const nonAsciiChinese = /[\u4E00-\u9FFF]/u

    for (const [namespace, locales] of Object.entries(resources['en-US'])) {
      for (const [key, value] of Object.entries(locales)) {
        expect(value, `${namespace}.${key}`).not.toMatch(nonAsciiChinese)
      }
    }
  })
})
