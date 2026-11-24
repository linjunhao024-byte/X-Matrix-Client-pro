import { describe, expect, it } from 'vitest'

import { availableLocales, en, keyDiff, locale, setLocale, t, tw, zh } from './index'

describe('i18n', () => {
  it('keeps exactly the same keys in every dictionary', () => {
    expect(keyDiff(zh, en)).toEqual([])
    expect(keyDiff(zh, tw)).toEqual([])
  })

  it('has no empty translation', () => {
    for (const [name, table] of [
      ['zh', zh],
      ['zh-tw', tw],
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
    setLocale('zh-tw')
    expect(t('nav.home')).toBe('首頁')
    setLocale('zh')
    expect(t('nav.home')).toBe('首页')
    expect(locale.value).toBe('zh')
  })

  it('offers every locale, each named in itself', () => {
    expect(availableLocales().map((item) => item.value)).toEqual(['zh', 'zh-tw', 'en'])
    expect(availableLocales().map((item) => item.label)).toEqual(['简体中文', '繁體中文', 'English'])
  })

  it('speaks Taiwan in the Traditional column', () => {
    // Spot checks on the words a straight character conversion gets wrong:
    // these are the substitutions the phrase table exists for.
    expect(tw['nav.home']).toBe('首頁')
    expect(tw['engine.stale']).toContain('儲存')
    expect(tw['settings.hotkeySaveHint']).toContain('儲存')
    expect(tw['nodes.sort.latency']).toBe('按延遲')
    expect(tw['about.caps.backup']).toBe('設定備份')
  })
})
