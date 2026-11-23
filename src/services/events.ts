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
