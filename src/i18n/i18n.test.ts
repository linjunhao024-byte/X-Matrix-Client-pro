import { describe, expect, it } from 'vitest'

import { availableLocales, en, keyDiff, locale, setLocale, t, zh } from './index'

describe('i18n', () => {
  it('keeps exactly the same keys in both dictionaries', () => {
    expect(keyDiff(zh, en)).toEqual([])
  })

  it('has no empty translation', () => {
    for (const [name, table] of [
      ['zh', zh],
      ['en', en],
    ] as const) {
      for (const [key, value] of Object.entries(table)) {
        expect(value.trim(), `${name}:${key}`).not.toBe('')
      }
    }
  })

  it('substitutes parameters', () => {
    expect(t('nodes.latency.ms', { value: 42 })).toContain('42')
    expect(t('nodes.count', { count: 3 })).toContain('3')
  })

  it('returns the key itself when a translation is missing', () => {
    expect(t('nope.not.here')).toBe('nope.not.here')
  })

  it('switches locale and back', () => {
    setLocale('en')
    expect(t('nav.home')).toBe('Home')
    setLocale('zh')
    expect(t('nav.home')).toBe('首页')
    expect(locale.value).toBe('zh')
  })

  it('offers both locales', () => {
    expect(availableLocales().map((item) => item.value)).toEqual(['zh', 'en'])
  })
})
