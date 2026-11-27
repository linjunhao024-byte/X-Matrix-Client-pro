import { beforeEach, describe, expect, it, vi } from 'vitest'

/**
 * The daily kernel lookup has two promises to keep, and both are easy to break
 * silently: "once a day" must not become "never" after a clock bump, and "off"
 * must be a real off — not a hidden banner over a request that was still sent.
 * The first-run hint is pinned here too, because its one rule is a *negative*
 * (never over a running kernel) and negatives are what regressions hide in.
 */

const { api } = vi.hoisted(() => ({ api: { kernelCheck: vi.fn() } }))

vi.mock('./contract', () => ({ api }))

import {
  CHECKED_AT_KEY,
  autoCheck,
  kernelDirectory,
  readCheckedAt,
  recordCheck,
  releases,
  shouldCheck,
  showKernelHint,
} from './kernelupdate'

const NOW = 1_767_000_000_000
const DAY = 86_400_000

/** One plausible `kernel_check` answer; only the fields the UI reads matter. */
function release(current: string | null, version = '26.1.1') {
  return {
    kind: 'xray' as const,
    tag: `v${version}`,
    version,
    url: 'https://example.test/xray.zip',
    bytes: 12,
    current,
    note: null,
  }
}

let store: Map<string, string>

beforeEach(() => {
  // The vitest environment is node: there is no localStorage unless stubbed.
  store = new Map()
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => {
      store.set(key, String(value))
    },
    removeItem: (key: string) => {
      store.delete(key)
    },
    clear: () => store.clear(),
  })
  vi.clearAllMocks()
  releases.value = {}
  api.kernelCheck.mockResolvedValue(release('26.1.0'))
})

describe('shouldCheck', () => {
  it('asks when nothing was ever recorded', () => {
    expect(shouldCheck(NOW, null)).toBe(true)
  })

  it('stays quiet one millisecond before a day has passed', () => {
    expect(shouldCheck(NOW, String(NOW - DAY + 1))).toBe(false)
  })

  it('asks exactly one day later', () => {
    expect(shouldCheck(NOW, String(NOW - DAY))).toBe(true)
  })

  it('keeps asking after that', () => {
    expect(shouldCheck(NOW, String(NOW - 3 * DAY))).toBe(true)
  })

  it('treats a corrupt entry as absent instead of locking updates out', () => {
    expect(shouldCheck(NOW, 'yesterday-ish')).toBe(true)
    expect(shouldCheck(NOW, '-1')).toBe(true)
    expect(shouldCheck(NOW, '')).toBe(true)
  })

  it('treats an entry dated in the future as absent', () => {
    // A clock change or a restored profile can leave a timestamp ahead of now;
    // honouring it would mean waiting days longer than the user expects.
    expect(shouldCheck(NOW, String(NOW + DAY))).toBe(true)
  })

  it('reads the stored timestamp when none is passed', () => {
    recordCheck(NOW)
    expect(shouldCheck(NOW + DAY - 1)).toBe(false)
    expect(shouldCheck(NOW + DAY)).toBe(true)
  })
})

describe('recordCheck', () => {
  it('stores the moment under the documented key', () => {
    recordCheck(NOW)
    expect(store.get(CHECKED_AT_KEY)).toBe(String(NOW))
    expect(readCheckedAt()).toBe(String(NOW))
  })
})

describe('autoCheck', () => {
  it('does not even send a request when the switch is off', async () => {
    await expect(autoCheck('xray', { enabled: false, now: NOW })).resolves.toBeNull()
    expect(api.kernelCheck).not.toHaveBeenCalled()
    // Nothing was recorded either, so turning it back on checks immediately
    // rather than serving a leftover "already checked" penalty.
    expect(readCheckedAt()).toBeNull()
  })

  it('asks, caches the answer and records the moment', async () => {
    const result = await autoCheck('xray', { enabled: true, now: NOW })
    expect(result?.version).toBe('26.1.1')
    expect(api.kernelCheck).toHaveBeenCalledTimes(1)
    expect(readCheckedAt()).toBe(String(NOW))
    expect(releases.value.xray?.current).toBe('26.1.0')
  })

  it('does not ask again inside the same day', async () => {
    await autoCheck('xray', { enabled: true, now: NOW })
    await expect(autoCheck('xray', { enabled: true, now: NOW + DAY - 1 })).resolves.toBeNull()
    expect(api.kernelCheck).toHaveBeenCalledTimes(1)
  })

  it('asks again after a day', async () => {
    await autoCheck('xray', { enabled: true, now: NOW })
    await autoCheck('xray', { enabled: true, now: NOW + DAY })
    expect(api.kernelCheck).toHaveBeenCalledTimes(2)
  })

  it('stays silent when the lookup fails, and does not count it as a check', async () => {
    api.kernelCheck.mockRejectedValue(new Error('offline'))
    await expect(autoCheck('xray', { enabled: true, now: NOW })).resolves.toBeNull()
    expect(releases.value.xray).toBeUndefined()
    // No timestamp: the next launch retries instead of waiting a day because
    // GitHub happened to be unreachable once.
    expect(readCheckedAt()).toBeNull()
    expect(shouldCheck(NOW + 1)).toBe(true)
  })
})

describe('showKernelHint', () => {
  it('shows when the selected kernel is missing and nothing blocks it', () => {
    expect(showKernelHint({ missing: true, running: false, dismissed: false })).toBe(true)
  })

  it('never shows over a running kernel', () => {
    expect(showKernelHint({ missing: true, running: true, dismissed: false })).toBe(false)
  })

  it('never shows when the binary is present', () => {
    expect(showKernelHint({ missing: false, running: false, dismissed: false })).toBe(false)
  })

  it('stays closed once dismissed', () => {
    expect(showKernelHint({ missing: true, running: false, dismissed: true })).toBe(false)
  })
})

describe('kernelDirectory', () => {
  it('strips the file name from a Windows path', () => {
    expect(kernelDirectory('C:\\Users\\me\\AppData\\Roaming\\com.xmatrix.client\\engines\\xray.exe')).toBe(
      'C:\\Users\\me\\AppData\\Roaming\\com.xmatrix.client\\engines',
    )
  })

  it('handles forward slashes', () => {
    expect(kernelDirectory('/usr/local/bin/sing-box')).toBe('/usr/local/bin')
  })

  it('leaves a bare file name alone', () => {
    expect(kernelDirectory('xray.exe')).toBe('xray.exe')
  })
})