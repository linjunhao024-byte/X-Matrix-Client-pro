/**
 * A very small translation layer.
 *
 * Deliberately not `vue-i18n`: the app needs `t()` and two locales, and a
 * hand-written dictionary keeps the key set testable (`i18n.test.ts` asserts
 * `zh` and `en` have exactly the same keys, so a half-translated screen fails
 * the build instead of shipping).
 *
 * `t()` reads `locale` during render, so switching language re-renders every
 * component that uses it - no reload, no event bus.
 */
import { ref } from 'vue'

import { en } from './en'
import { zh } from './zh'

export type Locale = 'zh' | 'en'

const messages: Record<Locale, Record<string, string>> = { zh, en }

export const locale = ref<Locale>('zh')

/** Where the UI keeps the language the user picked. */
const STORAGE_KEY = 'xm.ui.locale'

function isLocale(value: unknown): value is Locale {
  return value === 'zh' || value === 'en'
}

function applyDocumentLang(next: Locale): void {
  // Screen readers and spell checkers both read this. Guarded because the
  // dictionary tests run in Node, where there is no document.
  if (typeof document !== 'undefined') {
    document.documentElement.lang = next === 'zh' ? 'zh-CN' : 'en'
  }
}

function remember(next: Locale): void {
  // Guarded for Node (the dictionary tests) and for a browser that refuses
  // storage; neither should break a language switch.
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(STORAGE_KEY, next)
  } catch {
    /* private mode: the choice just will not survive a restart */
  }
}

export function setLocale(next: Locale): void {
  locale.value = next
  applyDocumentLang(next)
  remember(next)
}

/**
 * Restore the saved locale. Called once from `main.ts` before the first paint,
 * so a returning user does not see a flash of the wrong language. Does nothing
 * where there is no storage, which is also what the Node tests want.
 */
export function initLocale(): void {
  if (typeof localStorage === 'undefined') return
  let saved: string | null = null
  try {
    saved = localStorage.getItem(STORAGE_KEY)
  } catch {
    saved = null
  }
  if (isLocale(saved)) locale.value = saved
  applyDocumentLang(locale.value)
}

export function availableLocales(): { value: Locale; label: string }[] {
  return [
    { value: 'zh', label: '简体中文' },
    { value: 'en', label: 'English' },
  ]
}

/**
 * Translate `key`, substituting `{name}` placeholders.
 *
 * A missing key returns the key itself rather than an empty string: a visible
 * `nodes.empty.title` is a bug report, an empty label is a mystery.
 */
export function t(key: string, params?: Record<string, string | number>): string {
  const table = messages[locale.value] ?? zh
  const template = table[key] ?? zh[key] ?? key
  if (!params) return template
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  )
}

/** Keys present in one dictionary but not the other - used by the test. */
export function keyDiff(a: Record<string, string>, b: Record<string, string>): string[] {
  const missing = Object.keys(a).filter((key) => !(key in b))
  const extra = Object.keys(b).filter((key) => !(key in a))
  return [...missing.map((key) => `missing:${key}`), ...extra.map((key) => `extra:${key}`)]
}

export { en, messages, zh }
