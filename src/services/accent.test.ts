import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'

import {
  ACCENTS,
  DEFAULT_ACCENT,
  contrastRatio,
  deriveCustom,
  isPreset,
  relativeLuminance,
  resolveAccent,
} from './accent'

const tokensCss = readFileSync(new URL('../styles/tokens.css', import.meta.url), 'utf8')

/** The declarations inside the first block whose selector starts with `selector`. */
function block(selector: string): string {
  const start = tokensCss.indexOf(selector)
  expect(start, `tokens.css has no ${selector}`).toBeGreaterThanOrEqual(0)
  const open = tokensCss.indexOf('{', start)
  const close = tokensCss.indexOf('}', open)
  return tokensCss.slice(open + 1, close)
}

function customProperty(css: string, name: string): string {
  const match = new RegExp(`--${name}:\\s*([^;]+);`).exec(css)
  return match === null ? '' : match[1].trim()
}

describe('ACCENTS', () => {
  it('keeps a fixed order and unique ids', () => {
    expect(ACCENTS.map((preset) => preset.id)).toEqual([
      'indigo',
      'violet',
      'ocean',
      'teal',
      'amber',
      'rose',
    ])
    expect(new Set(ACCENTS.map((preset) => preset.id)).size).toBe(ACCENTS.length)
  })

  it('ships indigo exactly as tokens.css does', () => {
    // The default has to be a no-op: picking indigo must repaint nothing.
    const indigo = ACCENTS[0]
    expect(indigo.id).toBe(DEFAULT_ACCENT)

    const light = block(':root {')
    expect(customProperty(light, 'accent')).toBe(indigo.light)
    expect(customProperty(light, 'accent-hover')).toBe(indigo.hoverLight)
    expect(customProperty(light, 'accent-soft')).toBe(indigo.softLight)
    expect(customProperty(light, 'accent-contrast')).toBe(indigo.contrastLight)

    const dark = block(':root[data-theme="dark"] {')
    expect(customProperty(dark, 'accent')).toBe(indigo.dark)
    expect(customProperty(dark, 'accent-hover')).toBe(indigo.hoverDark)
    expect(customProperty(dark, 'accent-soft')).toBe(indigo.softDark)
    expect(customProperty(dark, 'accent-contrast')).toBe(indigo.contrastDark)
  })

  it('keeps every tokens.css accent block in step with the table', () => {
    // Presets are painted by CSS before any JS runs; this is what stops the two
    // sources of truth from drifting apart.
    for (const preset of ACCENTS) {
      const light = block(`:root[data-accent="${preset.id}"]`)
      expect(customProperty(light, 'accent')).toBe(preset.light)
      expect(customProperty(light, 'accent-hover')).toBe(preset.hoverLight)
      expect(customProperty(light, 'accent-soft')).toBe(preset.softLight)
      expect(customProperty(light, 'accent-contrast')).toBe(preset.contrastLight)

      const dark = block(`:root[data-theme="dark"][data-accent="${preset.id}"]`)
      expect(customProperty(dark, 'accent')).toBe(preset.dark)
      expect(customProperty(dark, 'accent-hover')).toBe(preset.hoverDark)
      expect(customProperty(dark, 'accent-soft')).toBe(preset.softDark)
      expect(customProperty(dark, 'accent-contrast')).toBe(preset.contrastDark)
    }
  })

  it('reaches AA on every preset, in both themes', () => {
    // A filled primary button is normal-sized text: 4.5:1, not 3:1.
    for (const preset of ACCENTS) {
      expect(contrastRatio(preset.light, preset.contrastLight), preset.id).toBeGreaterThanOrEqual(4.5)
      expect(contrastRatio(preset.dark, preset.contrastDark), preset.id).toBeGreaterThanOrEqual(4.5)
    }
  })

  it('picks the label colour by the documented rule, not by hand', () => {
    for (const preset of ACCENTS) {
      const expectedLight = contrastRatio(preset.light, '#ffffff') >= 4.5 ? '#ffffff' : '#10121a'
      const expectedDark = contrastRatio(preset.dark, '#ffffff') >= 4.5 ? '#ffffff' : '#10121a'
      expect(preset.contrastLight, preset.id).toBe(expectedLight)
      expect(preset.contrastDark, preset.id).toBe(expectedDark)
    }
  })
})

describe('contrast maths', () => {
  it('follows the WCAG luminance endpoints', () => {
    expect(relativeLuminance('#000000')).toBe(0)
    expect(relativeLuminance('#ffffff')).toBe(1)
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21)
    expect(contrastRatio('#ffffff', '#ffffff')).toBeCloseTo(1)
  })

  it('is symmetric and accepts either case', () => {
    expect(contrastRatio('#7c3aed', '#ffffff')).toBeCloseTo(contrastRatio('#FFFFFF', '#7C3AED'))
  })
})

describe('resolveAccent', () => {
  it('defaults to indigo when nothing is stored', () => {
    expect(resolveAccent(null)).toEqual({ kind: 'preset', id: 'indigo' })
  })

  it('accepts a known preset id', () => {
    expect(resolveAccent('rose')).toEqual({ kind: 'preset', id: 'rose' })
    expect(isPreset('rose')).toBe(true)
    expect(isPreset('rosegold')).toBe(false)
  })

  it('accepts a well-formed custom hex', () => {
    expect(resolveAccent('custom:#7c3aed')).toEqual({ kind: 'custom', hex: '#7c3aed' })
  })

  it('falls back to indigo for garbage', () => {
    expect(resolveAccent('custom:#zzzzzz')).toEqual({ kind: 'preset', id: 'indigo' })
    expect(resolveAccent('custom:red')).toEqual({ kind: 'preset', id: 'indigo' })
    expect(resolveAccent('nonsense')).toEqual({ kind: 'preset', id: 'indigo' })
  })
})

describe('deriveCustom', () => {
  it('steps the hover state the way the theme expects', () => {
    // Light surfaces press darker; a light accent has to lift off a dark one.
    const light = deriveCustom('#7c3aed', false)
    const dark = deriveCustom('#7c3aed', true)
    expect(relativeLuminance(light.hover)).toBeLessThan(relativeLuminance(light.accent))
    expect(relativeLuminance(dark.hover)).toBeGreaterThan(relativeLuminance(dark.accent))
  })

  it('normalises the hex and weights the wash per theme', () => {
    const light = deriveCustom('#7C3AED', false)
    const dark = deriveCustom('#7C3AED', true)
    expect(light.accent).toBe('#7c3aed')
    expect(dark.accent).toBe('#7c3aed')
    expect(light.soft).toBe('rgba(124, 58, 237, 0.12)')
    expect(dark.soft).toBe('rgba(124, 58, 237, 0.16)')
  })

  it('picks a readable label on the derived accent', () => {
    const derived = deriveCustom('#7c3aed', false)
    expect(derived.contrast).toBe('#ffffff')
    expect(contrastRatio(derived.accent, derived.contrast)).toBeGreaterThanOrEqual(4.5)
  })

  it('rejects a malformed hex instead of painting black', () => {
    expect(() => deriveCustom('#12345', false)).toThrow(/invalid hex/)
  })
})