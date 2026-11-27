/**
 * Kernel updates: one quiet look-up a day, and the first-run "where is it" hint.
 *
 * The install itself stays in Settings (`kernelInstall`); nothing here calls it.
 * Telling the user that a newer official build exists is a notification, not a
 * decision to swap a binary under a running tunnel.
 *
 * Failures are deliberately silent: offline, a GitHub outage and a proxy that
 * will not carry the request all produce `null`, no banner and no stored
 * timestamp. A check that did not answer is not a check that happened.
 */
import { ref } from 'vue'

import { api, type EngineKind, type KernelRelease_Serialize } from './contract'

const DAY_MS = 86_400_000

/** Last moment a check succeeded, in localStorage. Read by the startup gate. */
export const CHECKED_AT_KEY = 'xm.ui.kernelCheckedAt'

/**
 * Newest release seen this session, per kernel. Memory only: the feed is the
 * kind of thing that changes tomorrow, and a stale `store.json` entry would
 * keep claiming an update long after one was installed.
 */
export const releases = ref<Partial<Record<EngineKind, KernelRelease_Serialize>>>({})

/**
 * localStorage, or null where it does not exist.
 *
 * Unit tests and a browser preview have no storage; a preference that cannot be
 * written still has a value for this session, it just is not remembered. Never
 * throw from here: a full quota must not stop the app from painting.
 */
function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage
  } catch {
    return null
  }
}

/** Raw timestamp of the last successful check, or null when there was none. */
export function readCheckedAt(): string | null {
  try {
    return storage()?.getItem(CHECKED_AT_KEY) ?? null
  } catch {
    return null
  }
}

/**
 * Is a check due?
 *
 * "At most once a day", measured from the last successful one (exactly one day
 * counts as due). An entry that cannot be parsed — or one dated in the future,
 * which a clock change produces — is treated as absent: it proves nothing about
 * the last check, and honouring it could lock updates out for good.
 */
export function shouldCheck(now: number, raw: string | null = readCheckedAt()): boolean {
  if (!raw) return true
  const at = Number(raw)
  if (!Number.isFinite(at) || at <= 0) return true
  if (at > now) return true
  return now - at >= DAY_MS
}

/**
 * Remember that a check happened at `now`.
 *
 * Called only after a successful answer, so a failed attempt leaves the gate
 * open for the next launch instead of making the user wait a day for a one-off
 * DNS hiccup.
 */
export function recordCheck(now: number): void {
  try {
    storage()?.setItem(CHECKED_AT_KEY, String(now))
  } catch {
    // A preference that cannot be stored costs a second check, not the app.
  }
}

/**
 * Ask the release feed and cache the answer. Never throws.
 *
 * The cached value comes from the backend verbatim — version numbers shown in
 * the UI are the ones `kernel_check` returned, never a local guess.
 */
export async function run(
  kind: EngineKind,
  now: number = Date.now(),
): Promise<KernelRelease_Serialize | null> {
  try {
    const release = await api.kernelCheck(kind)
    releases.value = { ...releases.value, [kind]: release }
    recordCheck(now)
    return release
  } catch {
    return null
  }
}

export interface AutoCheckOptions {
  /** The user's `kernelUpdateCheck` preference; false means do not even ask. */
  enabled: boolean
  /** Injectable clock, so the 24-hour boundary is testable. */
  now?: number
}

/**
 * The startup call: one check, at most daily, only when the switch is on.
 *
 * `enabled` is passed in rather than read from a store so this module stays a
 * plain service and "off really means off" is testable without Pinia.
 */
export async function autoCheck(
  kind: EngineKind,
  options: AutoCheckOptions,
): Promise<KernelRelease_Serialize | null> {
  if (!options.enabled) return null
  const now = options.now ?? Date.now()
  if (!shouldCheck(now)) return null
  return run(kind, now)
}

export interface KernelHintState {
  /** The selected kernel's binary check says the file is not there. */
  missing: boolean
  /** A kernel is running: the binary it was launched from exists right now. */
  running: boolean
  /** The user closed the hint once; it stays closed. */
  dismissed: boolean
}

/**
 * Should the first-run "no kernel" hint appear?
 *
 * All three conditions exist to avoid nagging: a running kernel cannot be
 * missing, a present binary has nothing to explain, and a dismissed hint stays
 * dismissed. Everything the hint says is also available in Settings, so hiding
 * it costs the user nothing.
 */
export function showKernelHint(state: KernelHintState): boolean {
  return state.missing && !state.running && !state.dismissed
}

/**
 * Folder a full binary path lives in.
 *
 * The hint names the real directory the backend reports instead of a hard-coded
 * `%APPDATA%` string: a custom `xray_binary` override is looked up somewhere
 * else, and sending the user to the wrong folder would be worse than saying
 * nothing. Handles both separators; a bare filename has no folder to name.
 */
export function kernelDirectory(path: string): string {
  const cut = Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'))
  return cut > 0 ? path.slice(0, cut) : path
}