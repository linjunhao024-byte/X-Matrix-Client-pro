//! Turning a key press into the text the backend stores.
//!
//! The chord lives in the settings as plain text (`Ctrl+Shift+X`) because that
//! is what a person can read and retype. This module is the one place that
//! knows how a browser `KeyboardEvent` becomes that text, so the settings page
//! and its tests cannot disagree about which key produced which chord.

/** Modifiers in the order the backend prints them. */
const MODIFIER_ORDER = ['Ctrl', 'Alt', 'Shift', 'Win'] as const

/** Keys that are modifiers on their own: pressing one is not a chord yet. */
const MODIFIER_KEYS = new Set(['Control', 'Alt', 'Shift', 'Meta', 'AltGraph', 'CapsLock', 'Fn'])

/** Named keys the backend understands, spelled as it spells them. */
const NAMED_KEYS: Record<string, string> = {
  ' ': 'Space',
  Spacebar: 'Space',
  Enter: 'Enter',
  Return: 'Enter',
  Tab: 'Tab',
  Escape: 'Esc',
  Esc: 'Esc',
  Backspace: 'Backspace',
  Insert: 'Insert',
  Delete: 'Delete',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
}

/** A press that can become a chord. */
export interface KeyPress {
  key: string
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  metaKey: boolean
}

/**
 * The chord a press describes, or `null` when the press is not a chord yet
 * (a bare modifier, or a key the backend would refuse).
 *
 * Letters are uppercased and everything else is passed through the named-key
 * table, because the settings text is what the backend parses and it accepts
 * exactly those spellings.
 */
export function comboFromKeyPress(press: KeyPress): string | null {
  const key = press.key
  if (!key || MODIFIER_KEYS.has(key)) {
    return null
  }
  const name = keyName(key)
  if (!name) {
    return null
  }
  const modifiers: string[] = []
  if (press.ctrlKey) modifiers.push(MODIFIER_ORDER[0])
  if (press.altKey) modifiers.push(MODIFIER_ORDER[1])
  if (press.shiftKey) modifiers.push(MODIFIER_ORDER[2])
  if (press.metaKey) modifiers.push(MODIFIER_ORDER[3])
  if (modifiers.length === 0) {
    // A bare key would fire while typing anywhere on the desktop; the backend
    // allows it for function keys, and the page offers the same leniency.
    return name
  }
  return [...modifiers, name].join('+')
}

/** The backend's spelling of one key, or `null` for keys it does not know. */
export function keyName(key: string): string | null {
  // Named keys first: the space bar arrives as a one-character key, so the
  // letter/digit fast path below would refuse it before this table is asked.
  const named = NAMED_KEYS[key]
  if (named) {
    return named
  }
  if (key.length === 1) {
    const upper = key.toUpperCase()
    if (/^[A-Z0-9]$/.test(upper)) {
      return upper
    }
    // Punctuation and IME keys are not in the backend's table: saying "not a
    // chord" is better than storing text the save would then reject.
    return null
  }
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) {
    return key.toUpperCase()
  }
  return null
}

/** Human-readable hint shown while the row waits for a press. */
export function isChordComplete(text: string): boolean {
  return text.trim().length > 0
}
