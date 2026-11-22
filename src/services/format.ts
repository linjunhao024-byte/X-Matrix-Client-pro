/**
 * Byte and rate formatting.
 *
 * Kept away from the contract layer so it can be unit-tested without touching
 * the Tauri bridge, and so every view formats numbers the same way.
 */
import { t } from '../i18n'

/**
 * Byte counters cross the IPC boundary as `number | null` (JSON has no NaN, so
 * the generated types are honest about it). Everything that does arithmetic
 * funnels through here instead of sprinkling `?? 0` around.
 */
export function bytes(value: number | null | undefined): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

/** `1.4 MB`, `812 B`, … — never more than one decimal place. */
export function formatBytes(value: number | null | undefined): string {
  const size = bytes(value)
  if (size < 1024) return `${Math.round(size)} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let scaled = size / 1024
  let unit = 0
  while (scaled >= 1024 && unit < units.length - 1) {
    scaled /= 1024
    unit += 1
  }
  return `${scaled.toFixed(1)} ${units[unit]}`
}

export function formatRate(bytesPerSecond: number | null | undefined): string {
  return `${formatBytes(Math.max(0, Math.round(bytes(bytesPerSecond))))}/s`
}

/**
 * Human age of a timestamp, in the current UI language (`time.*` keys).
 *
 * Timestamps cross IPC as unix milliseconds in an `f64`, so a missing value is
 * `null` rather than 0 — 0 would render as 1970.
 */
export function formatWhen(at: number | null | undefined, now = Date.now()): string {
  if (typeof at !== 'number' || !Number.isFinite(at) || at <= 0) return t('time.never')
  const minutes = Math.floor(Math.max(0, now - at) / 60_000)
  if (minutes < 1) return t('time.now')
  if (minutes < 60) return t('time.minutes', { count: minutes })
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return t('time.hours', { count: hours })
  const days = Math.floor(hours / 24)
  if (days === 1) return t('time.yesterday')
  if (days < 30) return t('time.days', { count: days })
  return t('time.months', { count: Math.floor(days / 30) })
}

/**
 * Clock time for a timestamp the kernel handed us as text.
 *
 * sing-box reports `2026-11-22T03:55:30.4070053+08:00`; printing that raw in a
 * live table is unreadable. The date is added only when the moment is not
 * today, so a row from a long-lived connection is not mistaken for one just
 * opened.
 */
export function formatClock(value: string | null | undefined, now = Date.now()): string {
  if (!value) return '—'
  const at = new Date(value)
  if (Number.isNaN(at.getTime())) return value
  const time = at.toLocaleTimeString(undefined, { hour12: false })
  if (new Date(now).toDateString() === at.toDateString()) return time
  const month = String(at.getMonth() + 1).padStart(2, '0')
  const day = String(at.getDate()).padStart(2, '0')
  return `${month}-${day} ${time}`
}

/** `100 GB` instead of `100.0 GB` — a decimal that only costs column width. */
export function formatBytesCompact(value: number | null | undefined): string {
  return formatBytes(value).replace(/\.0 (?=[A-Z])/, ' ')
}

/**
 * `2026-03-01` — a plain calendar day, for things like a plan's expiry date
 * where "3 天后" would be a moving target nobody can plan around.
 */
export function formatDay(value: number | null | undefined): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) return '—'
  const at = new Date(value)
  if (Number.isNaN(at.getTime())) return '—'
  const month = String(at.getMonth() + 1).padStart(2, '0')
  const day = String(at.getDate()).padStart(2, '0')
  return `${at.getFullYear()}-${month}-${day}`
}
