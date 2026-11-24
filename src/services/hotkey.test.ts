import { describe, expect, it } from 'vitest'
import { comboFromKeyPress, isChordComplete, keyName } from './hotkey'

const press = (key: string, mods: Partial<Record<'ctrl' | 'alt' | 'shift' | 'meta', boolean>> = {}) => ({
  key,
  ctrlKey: mods.ctrl ?? false,
  altKey: mods.alt ?? false,
  shiftKey: mods.shift ?? false,
  metaKey: mods.meta ?? false,
})

describe('comboFromKeyPress', () => {
  it('spells the shipped defaults the way the backend does', () => {
    expect(comboFromKeyPress(press('x', { ctrl: true, shift: true }))).toBe('Ctrl+Shift+X')
    expect(comboFromKeyPress(press('w', { ctrl: true, shift: true }))).toBe('Ctrl+Shift+W')
  })

  it('puts modifiers in one order no matter how they were pressed', () => {
    expect(comboFromKeyPress(press('t', { shift: true, alt: true, ctrl: true }))).toBe('Ctrl+Alt+Shift+T')
    expect(comboFromKeyPress(press('t', { ctrl: true, alt: true, shift: true }))).toBe('Ctrl+Alt+Shift+T')
  })

  it('maps named keys to the backend spelling', () => {
    expect(comboFromKeyPress(press(' ', { ctrl: true }))).toBe('Ctrl+Space')
    expect(comboFromKeyPress(press('ArrowUp', { alt: true }))).toBe('Alt+Up')
    expect(comboFromKeyPress(press('F5', { ctrl: true }))).toBe('Ctrl+F5')
    expect(comboFromKeyPress(press('Escape', { ctrl: true, shift: true }))).toBe('Ctrl+Shift+Esc')
  })

  it('treats Windows as a modifier of its own', () => {
    expect(comboFromKeyPress(press('d', { meta: true }))).toBe('Win+D')
  })

  it('returns null while only a modifier is held down', () => {
    for (const key of ['Control', 'Shift', 'Alt', 'Meta', 'CapsLock']) {
      expect(comboFromKeyPress(press(key, { ctrl: true }))).toBeNull()
    }
  })

  it('refuses keys the backend would reject instead of storing them', () => {
    // Punctuation is not in the backend table, and saving it would only produce
    // an error message after the user has already moved on.
    expect(comboFromKeyPress(press(';', { ctrl: true }))).toBeNull()
    expect(comboFromKeyPress(press('Dead', { ctrl: true }))).toBeNull()
    expect(comboFromKeyPress(press('F25', { ctrl: true }))).toBeNull()
  })

  it('allows a bare function key, which Windows also accepts', () => {
    expect(comboFromKeyPress(press('F9'))).toBe('F9')
  })
})

describe('keyName', () => {
  it('uppercases letters and digits only', () => {
    expect(keyName('a')).toBe('A')
    expect(keyName('7')).toBe('7')
    expect(keyName('a1')).toBeNull()
  })
})

describe('isChordComplete', () => {
  it('treats blank text as a switched-off chord', () => {
    expect(isChordComplete('')).toBe(false)
    expect(isChordComplete('   ')).toBe(false)
    expect(isChordComplete('Ctrl+Shift+X')).toBe(true)
  })
})
