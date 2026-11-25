/**
 * Backend-pushed events, in one place.
 *
 * The architecture guard (`scripts/check-architecture.mjs`) lets only
 * `src/services/` touch `@tauri-apps/api/event`, so every subscription goes
 * through here and the stores keep calling typed helpers.
 */

import { listen } from '@tauri-apps/api/event'

/** Stops a subscription; the shape `listen` hands back. */
export type Unlisten = () => void

/** A kernel died on its own. The payload is its exit code. */
export const ENGINE_EXITED = 'engine://exited'

/**
 * Subscribe to a kernel that exited unexpectedly.
 *
 * A stop the user asked for never reaches this channel: the backend marks those
 * exits as expected, so a handler here only ever runs for a real crash.
 */
export function onEngineExit(handler: (code: number) => void): Promise<Unlisten> {
  return listen<number>(ENGINE_EXITED, (event) => handler(event.payload))
}

/**
 * The window left or returned to the tray (`xm:quiet`, emitted from
 * `src-tauri/src/lib.rs::set_quiet`).
 *
 * The payload is the new state, not a toggle: the backend sends it on every
 * hide and every reveal — tray icon, tray menu, close button, single instance —
 * so the UI never has to deduce where the window is.
 */
export const QUIET = 'xm:quiet'

export function onQuiet(handler: (quiet: boolean) => void): Promise<Unlisten> {
  // `=== true` rather than trusting the payload: the one thing the UI must not
  // do is pause its timers because a malformed payload looked truthy.
  return listen<boolean>(QUIET, (event) => handler(event.payload === true))
}
