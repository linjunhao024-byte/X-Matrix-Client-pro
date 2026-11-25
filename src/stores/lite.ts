/**
 * Lite mode: what the window does while it is not on screen.
 *
 * The backend emits `xm:quiet` on every hide and every reveal
 * (`set_quiet` in `src-tauri/src/lib.rs`); this store keeps that one boolean
 * so the views can stop and restart their own timers without each of them
 * subscribing to the event.
 *
 * `initQuiet()` also probes `isVisible()` once at startup. When the option
 * "start minimised" is on, the window is hidden during setup — before the
 * webview has loaded its JavaScript — so that first "we are hidden" event is
 * emitted with nobody listening. The probe is what makes that path honest:
 * the interface starts paused instead of starting its timers and only then
 * finding out the window is in the tray.
 */
import { readonly, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { onQuiet, type Unlisten } from '../services/events'

const quiet = ref(false)
let dispose: Unlisten | null = null
let starting: Promise<void> | null = null

/** `true` while the window is hidden in the tray and the UI must stay idle. */
export function useQuiet() {
  return readonly(quiet)
}

/**
 * Subscribe to the backend and learn the current state.
 *
 * Safe to call more than once: the second call reuses the first promise, so a
 * hot reload never stacks a second listener on the same channel.
 */
export function initQuiet(): Promise<void> {
  if (!starting) starting = wire()
  return starting
}

async function wire(): Promise<void> {
  try {
    dispose = await onQuiet((next) => {
      quiet.value = next
    })
  } catch (error) {
    // No backend channel (a plain browser dev server): leave `quiet` alone.
    // Nothing told us the window is hidden, so claiming it is would be a lie.
    console.warn('x-matrix: 轻量模式事件订阅失败：', error)
  }
  try {
    // Only a hidden window starts quiet; a visible one keeps whatever the
    // event already said (which is `false` unless a hide raced the startup).
    if (!(await getCurrentWindow().isVisible())) quiet.value = true
  } catch (error) {
    console.warn('x-matrix: 读取窗口可见性失败：', error)
  }
}

/** Drop the subscription. Used by tests and by an unmounting app shell. */
export function disposeQuiet(): void {
  dispose?.()
  dispose = null
  starting = null
}