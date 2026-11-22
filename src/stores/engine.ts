/**
 * Engine runtime state — the *only* place the UI learns what is running.
 *
 * The Rust side owns the truth; this store just mirrors it and never guesses.
 * One poller drives both the status badge and the traffic figures, so the whole
 * app ticks at a single cadence instead of one timer per view.
 */

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import {
  api,
  describeError,
  type EngineBinary,
  type EngineKind,
  type EngineStatus_Serialize,
  type TrafficSample,
} from '../services/contract'
import { bytes } from '../services/format'

const POLL_INTERVAL_MS = 1000
/** One minute of samples at the poll cadence — enough to see a trend, cheap to keep. */
const HISTORY_LIMIT = 60

export interface TrafficPoint {
  at: number
  up: number
  down: number
}

export const useEngineStore = defineStore('engine', () => {
  const status = ref<EngineStatus_Serialize | null>(null)
  const sample = ref<TrafficSample | null>(null)
  const kind = ref<EngineKind>('xray')
  const busy = ref(false)
  const error = ref<string | null>(null)
  const rates = ref({ up: 0, down: 0 })
  const binaries = ref<EngineBinary[]>([])
  /**
   * Version string per kernel, filled lazily.
   *
   * Missing entry means "not asked yet" or "not installed"; the UI shows the
   * version only when it has one, so a failed probe never becomes a scary
   * banner.
   */
  const versions = ref<Partial<Record<EngineKind, string>>>({})
  const history = ref<TrafficPoint[]>([])
  /**
   * Bytes counted since the last reset, not since the kernel booted.
   *
   * The kernel's own totals only ever grow (Xray) or report a sliding window
   * (sing-box), so neither can answer "how much did I use this session".
   */
  const session = ref({ up: 0, down: 0 })

  const running = computed(() => status.value?.running === true)
  const hasConnectionsApi = computed(() => sample.value?.supports_connections === true)
  const connections = computed(() => sample.value?.connections ?? [])

  /** The binary for the selected kernel, or null before the first load. */
  const currentBinary = computed(() => binaries.value.find((item) => item.kind === kind.value) ?? null)
  /**
   * True when the selected kernel is not installed. The UI explains this
   * before the user presses start, instead of surfacing a file-not-found.
   */
  const binaryMissing = computed(() => currentBinary.value !== null && !currentBinary.value.present)

  let timer: ReturnType<typeof setInterval> | undefined
  let previous: { at: number; up: number; down: number } | null = null

  function resetRates() {
    previous = null
    rates.value = { up: 0, down: 0 }
    history.value = []
  }

  /**
   * Zero the session counters without losing the curve.
   *
   * Only the counter is cleared; `previous` still holds the last sample, so the
   * next tick's delta is the traffic since this moment rather than the kernel's
   * whole total.
   */
  function resetSession() {
    session.value = { up: 0, down: 0 }
  }

  async function refreshVersion(target: EngineKind): Promise<void> {
    try {
      // Assign only after the await resolves. Spreading `versions` before it
      // makes two concurrent probes both write the snapshot they read at call
      // time, so whichever answers last wins and the other kernel's version
      // vanishes — exactly the "Xray line is missing" bug found on a real
      // machine, where both probes take ~1 s and finish in launch order.
      const reported = await api.engineVersion(target)
      versions.value = { ...versions.value, [target]: reported }
    } catch {
      // A kernel that cannot self-report is not an error worth interrupting the
      // user for: the binary check already explains a missing file.
    }
  }

  async function refreshStatus() {
    try {
      status.value = await api.engineStatus()
      // The backend is the truth. Auto-connect may have started the *other*
      // kernel than the one this tab had selected, and a UI that keeps
      // pointing at "xray" while sing-box runs sends the log viewer, the
      // binary check and the start button to the wrong place.
      if (status.value.running && status.value.kind) {
        kind.value = status.value.kind
      }
    } catch (cause) {
      error.value = describeError(cause)
    }
  }

  async function refreshTraffic() {
    if (!running.value) {
      sample.value = null
      resetRates()
      return
    }
    try {
      const next = await api.trafficSample()
      sample.value = next
      if (!next) {
        resetRates()
        return
      }
      const now = Date.now()
      const up = bytes(next.up_total)
      const down = bytes(next.down_total)
      if (previous) {
        const seconds = Math.max(0.001, (now - previous.at) / 1000)
        const deltaUp = Math.max(0, up - previous.up)
        const deltaDown = Math.max(0, down - previous.down)
        rates.value = { up: deltaUp / seconds, down: deltaDown / seconds }
        session.value = {
          up: session.value.up + deltaUp,
          down: session.value.down + deltaDown,
        }
      }
      previous = { at: now, up, down }
      const series = [...history.value, { at: now, up: rates.value.up, down: rates.value.down }]
      history.value =
        series.length > HISTORY_LIMIT ? series.slice(series.length - HISTORY_LIMIT) : series
    } catch {
      // A kernel that just stopped makes one sample fail; the next status poll
      // explains it. Never surface this as a UI error.
      sample.value = null
      resetRates()
    }
  }

  async function refreshBinaries() {
    try {
      binaries.value = await api.engineBinaries()
    } catch (cause) {
      error.value = describeError(cause)
    }
    // Ask only about kernels that exist: a version probe on a missing file is a
    // guaranteed error message with nothing to say.
    for (const binary of binaries.value) {
      if (binary.present) void refreshVersion(binary.kind)
    }
  }

  async function openEnginesDir() {
    try {
      await api.engineOpenDir()
    } catch (cause) {
      error.value = describeError(cause)
    }
  }

  async function tick() {
    await refreshStatus()
    await refreshTraffic()
  }

  function startPolling() {
    if (timer) return
    void tick()
    timer = setInterval(() => void tick(), POLL_INTERVAL_MS)
  }

  function stopPolling() {
    if (!timer) return
    clearInterval(timer)
    timer = undefined
  }

  async function start(nodeId: string) {
    busy.value = true
    error.value = null
    try {
      status.value = await api.engineStart(kind.value, nodeId)
      resetRates()
      session.value = { up: 0, down: 0 }
      await refreshTraffic()
    } catch (cause) {
      error.value = describeError(cause)
      throw cause
    } finally {
      busy.value = false
    }
  }

  async function stop() {
    busy.value = true
    error.value = null
    try {
      status.value = await api.engineStop()
      sample.value = null
      resetRates()
      session.value = { up: 0, down: 0 }
    } catch (cause) {
      error.value = describeError(cause)
      throw cause
    } finally {
      busy.value = false
    }
  }

  return {
    status,
    sample,
    kind,
    busy,
    error,
    rates,
    running,
    hasConnectionsApi,
    connections,
    binaries,
    history,
    session,
    resetSession,
    currentBinary,
    binaryMissing,
    versions,
    refreshBinaries,
    refreshVersion,
    openEnginesDir,
    startPolling,
    stopPolling,
    refreshStatus,
    refreshTraffic,
    start,
    stop,
  }
})
