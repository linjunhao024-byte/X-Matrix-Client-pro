/**
 * Accent colours: six presets plus one custom hex.
 *
 * The accent is a *semantic* colour like success/warning/danger, except it is
 * chosen by the user. Everything that paints the accent (primary buttons,
 * selection, focus rings, the chart line, the light halo behind the app) reads
 * `--accent` and its three companions, so a preset only needs to publish four
 * values per theme. Keeping the table here — instead of in CSS alone — lets the
 * UI colour its swatches from the same numbers, and lets the tests check the
 * contrast rule that a user could otherwise only discover by squinting.
 *
 * `tokens.css` carries one `[data-accent="…"]` block per preset that repeats
 * these values. That duplication is deliberate: the CSS has to work before JS
 * runs, and a test in `accent.test.ts` fails if the two ever disagree.
 */

export interface AccentPreset {
  id: string
  light: string
  dark: string
  /** One step darker than `light`, for the pressed/hover state. */
  hoverLight: string
  /** One step lighter than `dark`. */
  hoverDark: string
  /** The accent at 12% — used for focus rings and soft fills on light. */
  softLight: string
  /** The accent at 16% — the dark theme needs a slightly stronger wash. */
  softDark: string
  /** Text that sits on top of `light` and still passes WCAG AA. */
  contrastLight: string
  /** Text that sits on top of `dark`. */
  contrastDark: string
}

export type ResolvedAccent =
  | { kind: 'preset'; id: string }
  | { kind: 'custom'; hex: string }

/**
 * Order is part of the product: it is the order of the swatches, and `indigo`
 * first means "the default is the first thing the eye lands on".
 *
 * The indigo row must stay byte-for-byte equal to the values `tokens.css`
 * ships, or switching back to the default would visibly repaint the app.
 */
export const ACCENTS: AccentPreset[] = [
  {
    id: 'indigo',
    light: '#5b5bd6',
    dark: '#8f8ff8',
    hoverLight: '#4a4ac4',
    hoverDark: '#a5a5ff',
    softLight: 'rgba(91, 91, 214, 0.12)',
    softDark: 'rgba(143, 143, 248, 0.16)',
    contrastLight: '#ffffff',
    contrastDark: '#10121a',
  },
  {
    id: 'violet',
    light: '#7c3aed',
    dark: '#a78bfa',
    hoverLight: '#6d28d9',
    hoverDark: '#c4b5fd',
    softLight: 'rgba(124, 58, 237, 0.12)',
    softDark: 'rgba(167, 139, 250, 0.16)',
    contrastLight: '#ffffff',
    contrastDark: '#10121a',
  },
  {
    id: 'ocean',
    light: '#0284c7',
    dark: '#38bdf8',
    hoverLight: '#0369a1',
    hoverDark: '#7dd3fc',
    softLight: 'rgba(2, 132, 199, 0.12)',
    softDark: 'rgba(56, 189, 248, 0.16)',
    // White on #0284c7 is 4.10:1 — below AA for normal text — so this is the
    // one light preset whose button label has to be the ink, not white.
    contrastLight: '#10121a',
    contrastDark: '#10121a',
  },
  {
    id: 'teal',
    light: '#0d9488',
    dark: '#2dd4bf',
    hoverLight: '#0f766e',
    hoverDark: '#5eead4',
    softLight: 'rgba(13, 148, 136, 0.12)',
    softDark: 'rgba(45, 212, 191, 0.16)',
    contrastLight: '#10121a',
    contrastDark: '#10121a',
  },
  {
    id: 'amber',
    light: '#b45309',
    dark: '#fbbf24',
    hoverLight: '#92400e',
    hoverDark: '#fcd34d',
    softLight: 'rgba(180, 83, 9, 0.12)',
    softDark: 'rgba(251, 191, 36, 0.16)',
    contrastLight: '#ffffff',
    contrastDark: '#10121a',
  },
  {
    id: 'rose',
    light: '#be123c',
    dark: '#fb7185',
    hoverLight: '#9f1239',
    hoverDark: '#fda4af',
    softLight: 'rgba(190, 18, 60, 0.12)',
    softDark: 'rgba(251, 113, 133, 0.16)',
    contrastLight: '#ffffff',
    contrastDark: '#10121a',
  },
]

export const DEFAULT_ACCENT = 'indigo'

/** The stored form of a custom accent: `custom:#rrggbb`. */
const CUSTOM_PREFIX = 'custom:'
const CUSTOM_HEX = /^#[0-9a-fA-F]{6}$/
const WHITE = '#ffffff'
/** Near-black used for labels on a bright accent — same ink as the dark theme. */
const INK = '#10121a'
/** AA for normal-size text. */
const MIN_CONTRAST = 4.5

export function isPreset(id: string): boolean {
  return ACCENTS.some((preset) => preset.id === id)
}

/**
 * Read what `localStorage` holds into something the UI can act on.
 *
 * Anything unrecognised falls back to indigo rather than throwing: a hand-edited
 * storage entry must not be able to leave the app without an accent.
 */
export function resolveAccent(stored: string | null): ResolvedAccent {
  if (stored !== null && stored.startsWith(CUSTOM_PREFIX)) {
    const hex = stored.slice(CUSTOM_PREFIX.length).trim()
    if (CUSTOM_HEX.test(hex)) return { kind: 'custom', hex: hex.toLowerCase() }
    return { kind: 'preset', id: DEFAULT_ACCENT }
  }
  if (stored !== null && isPreset(stored)) return { kind: 'preset', id: stored }
  return { kind: 'preset', id: DEFAULT_ACCENT }
}

interface Rgb {
  r: number
  g: number
  b: number
  a: number
}

/**
 * Parse `#rgb` / `#rrggbb` (with or without the hash) into channels.
 *
 * Throws on anything else — callers that take user input validate first, and a
 * silent black would be worse than a loud failure in the one place it is used.
 */
function parseHex(hex: string): Rgb {
  let value = hex.trim().replace(/^#/, '')
  if (value.length === 3) {
    value = value
      .split('')
      .map((char) => char + char)
      .join('')
  }
  if (!/^[0-9a-fA-F]{6}$/.test(value)) throw new Error(`invalid hex colour: ${hex}`)
  return {
    r: Number.parseInt(value.slice(0, 2), 16),
    g: Number.parseInt(value.slice(2, 4), 16),
    b: Number.parseInt(value.slice(4, 6), 16),
    a: 1,
  }
}

function toHex({ r, g, b }: Rgb): string {
  const pair = (channel: number) => Math.round(channel).toString(16).padStart(2, '0')
  return `#${pair(r)}${pair(g)}${pair(b)}`
}

/** One sRGB channel → linear light, per WCAG 2.1 §relative luminance. */
function linear(channel: number): number {
  const value = channel / 255
  return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4
}

export function relativeLuminance(hex: string): number {
  const { r, g, b } = parseHex(hex)
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

export function contrastRatio(a: string, b: string): number {
  const first = relativeLuminance(a)
  const second = relativeLuminance(b)
  const lighter = Math.max(first, second)
  const darker = Math.min(first, second)
  return (lighter + 0.05) / (darker + 0.05)
}

/**
 * Which text colour a filled accent control should use.
 *
 * The rule is fixed so every preset can be checked mechanically: white when the
 * accent is dark enough for white text, otherwise near-black. The result is
 * then asserted against the 4.5:1 floor in `accent.test.ts`.
 */
function pickContrast(accent: string): string {
  return contrastRatio(accent, WHITE) >= MIN_CONTRAST ? WHITE : INK
}

/** Mix toward black (light theme hover) — `amount` is the share of black. */
function darken(rgb: Rgb, amount: number): Rgb {
  return {
    r: rgb.r * (1 - amount),
    g: rgb.g * (1 - amount),
    b: rgb.b * (1 - amount),
    a: rgb.a,
  }
}

/** Mix toward white (dark theme hover). */
function lighten(rgb: Rgb, amount: number): Rgb {
  return {
    r: rgb.r + (255 - rgb.r) * amount,
    g: rgb.g + (255 - rgb.g) * amount,
    b: rgb.b + (255 - rgb.b) * amount,
    a: rgb.a,
  }
}

function softColor(rgb: Rgb, alpha: number): string {
  return `rgba(${Math.round(rgb.r)}, ${Math.round(rgb.g)}, ${Math.round(rgb.b)}, ${alpha})`
}

/**
 * Derive the four accent variables from one free-form hex.
 *
 * Light theme steps *darker* on hover (a filled button reacts like a surface
 * being pressed), dark theme steps *lighter* (a light accent needs to lift off
 * a dark surface instead). The soft wash keeps the 12% / 16% split the presets
 * use, so custom and preset accents cast the same weight of shadow.
 */
export function deriveCustom(
  hex: string,
  dark: boolean,
): { accent: string; hover: string; soft: string; contrast: string } {
  const rgb = parseHex(hex)
  const accent = toHex(rgb)
  const hover = toHex(dark ? lighten(rgb, 0.18) : darken(rgb, 0.18))
  return {
    accent,
    hover,
    soft: softColor(rgb, dark ? 0.16 : 0.12),
    contrast: pickContrast(accent),
  }
}

/**
 * The four CSS variables an accent publishes, in the order the store writes
 * them. Exported so `ui.ts` and the tests agree on the spelling.
 */
export const ACCENT_VARS = ['--accent', '--accent-hover', '--accent-soft', '--accent-contrast'] as const

/**
 * Resolve one accent to its four values for a theme, whatever kind it is.
 *
 * This is the bridge between the table above and the DOM: the store calls it
 * once per apply, so preset and custom accents travel the same code path.
 */
export function accentValues(
  accent: ResolvedAccent,
  dark: boolean,
): { accent: string; hover: string; soft: string; contrast: string } {
  if (accent.kind === 'custom') return deriveCustom(accent.hex, dark)
  const preset = ACCENTS.find((item) => item.id === accent.id) ?? ACCENTS[0]
  return {
    accent: dark ? preset.dark : preset.light,
    hover: dark ? preset.hoverDark : preset.hoverLight,
    soft: dark ? preset.softDark : preset.softLight,
    contrast: dark ? preset.contrastDark : preset.contrastLight,
  }
}