import { describe, expect, it } from 'vitest'
import { validateQueryLocale } from './query-locale-validation'

describe('validateQueryLocale', () => {
  it('requires the Bulgarian primary locale before saving an edited task', () => {
    expect(validateQueryLocale('en-BG', 'BG', true)).toContain('bg-BG')
    expect(validateQueryLocale('bg-BG', 'BG', true)).toBe('')
  })
})
