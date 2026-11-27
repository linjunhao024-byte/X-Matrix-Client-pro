<script setup lang="ts">
/**
 * Home: the one screen that answers "is it on, and is traffic moving".
 *
 * Every number here comes from the Rust runtime; nothing is derived locally
 * except the transfer rates, which are a difference between two samples.
 */
import { computed, defineAsyncComponent, onMounted, onUnmounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../components/AppIcon.vue'

// ECharts is ~160 kB gzipped: load it after the page has painted instead of
// making the first screen wait for a chart that is empty anyway.
const TrafficChart = defineAsyncComponent(() => import('../components/TrafficChart.vue'))
const LogDialog = defineAsyncComponent(() => import('../components/LogDialog.vue'))
const ConfigDialog = defineAsyncComponent(() => import('../components/ConfigDialog.vue'))
import { t } from '../i18n'
import { api, describeError, type AppInfo, type EngineKind, type ExitIp, type ProxyMode } from '../services/contract'
import { formatBytes, formatRate, formatWhen } from '../services/format'
import { kernelDirectory, releases as kernelUpdates, showKernelHint } from '../services/kernelupdate'
import { quotaAlerts } from '../services/quota'
import { useEngineStore } from '../stores/engine'
import { initQuiet, useQuiet } from '../stores/lite'
import { useNodesStore } from '../stores/nodes'
import { useSettingsStore } from '../stores/settings'
import { useSubscriptionsStore } from '../stores/subscriptions'

const info = ref<AppInfo | null>(null)
const infoError = ref<string | null>(null)
const selectedNodeId = ref<string>('')
const logOpen = ref(false)
const configOpen = ref(false)
/** Real delay of the running tunnel, cleared whenever the engine changes. */
const realDelay = ref<number | null>(null)
/** Bytes per second through the tunnel, from the last speed sample. */
const realSpeed = ref<number | null>(null)
const speedPending = ref(false)
const delayPending = ref(false)
const delayError = ref<string | null>(null)

/**
 * Per-minute history that survives restarts. The live chart above is a
 * 60-second ring buffer, so "how much did I use today" needs its own record —
 * Rust keeps one on disk and this is only a view of it.
 */
type HistoryPoint = { minute: number | null; up: number | null; down: number | null }
const history = ref<HistoryPoint[]>([])
/** 60 or 1440: the panel only offers the two windows people actually ask for. */
const historyWindow = ref<number>(60)
let historyTimer: ReturnType<typeof setInterval> | null = null
/**
 * Lite mode: while the window is in the tray the minute-by-minute record goes
 * unread — nobody can see it — so its timer parks. On the way back the record
 * is read once *before* the timer re-arms, so the panel never shows the minute
 * the window was hidden as the current one.
 */
const quiet = useQuiet()
/** Cleared on unmount: a pending `initQuiet` must not arm a timer for nothing. */
let live = true

function startHistoryTimer(): void {
  if (historyTimer) return
  historyTimer = setInterval(() => void loadHistory(), 60_000)
}

function stopHistoryTimer(): void {
  if (!historyTimer) return
  clearInterval(historyTimer)
  historyTimer = null
}

watch(quiet, (idle) => {
  if (idle) {
    stopHistoryTimer()
    return
  }
  void loadHistory()
  startHistoryTimer()
})

async function loadHistory(): Promise<void> {
  try {
    history.value = await api.trafficHistory(historyWindow.value)
  } catch {
    // History is a convenience: a broken read must not put an error banner on
    // the screen that the user cannot act on.
    history.value = []
  }
}

function setHistoryWindow(minutes: number): void {
  if (historyWindow.value === minutes) return
  historyWindow.value = minutes
  void loadHistory()
}

async function clearHistory(): Promise<void> {
  try {
    await api.trafficClearHistory()
    history.value = []
  } catch {
    history.value = []
  }
}

/** Bars normalised against the busiest minute in the window. */
const historyBars = computed(() => {
  const points = history.value
  const peak = Math.max(1, ...points.map((point) => (point.up ?? 0) + (point.down ?? 0)))
  return points.map((point) => {
    const up = point.up ?? 0
    const down = point.down ?? 0
    const total = up + down
    const at = (point.minute ?? 0) * 60_000
    return {
      key: point.minute ?? 0,
      height: Math.max(2, Math.round((total / peak) * 100)),
      tip: t('home.history.tip', {
        time: new Date(at).toLocaleTimeString(),
        down: formatBytes(down),
        up: formatBytes(up),
      }),
    }
  })
})

/** Totals for the window on screen, so the bars have a caption. */
const historyTotals = computed(() => {
  let up = 0
  let down = 0
  for (const point of history.value) {
    up += point.up ?? 0
    down += point.down ?? 0
  }
  return { up, down }
})

const router = useRouter()

const nodes = useNodesStore()
const engine = useEngineStore()
const settings = useSettingsStore()
const subscriptions = useSubscriptionsStore()

/**
 * Sources whose plan is running out.
 *
 * The home screen is where the user looks when the tunnel stops working, so a
 * plan that expired yesterday belongs here rather than buried on the
 * subscription page.
 */
const quotaNotices = computed(() => quotaAlerts(subscriptions.items))

/** The roadmap list only means something to whoever is building this. */
const showProgress = import.meta.env.DEV

/** One line per source, worded by how it hurts. */
function quotaText(notice: ReturnType<typeof quotaAlerts>[number]): string {
  const { level, report } = notice
  if (level === 'expired') return t('home.quota.expired', { name: notice.name })
  if (level === 'full') return t('home.quota.full', { name: notice.name })
  if (level === 'low') {
    return t('home.quota.low', {
      name: notice.name,
      percent: Math.round((report.ratio ?? 0) * 100),
    })
  }
  return t('home.quota.expiring', { name: notice.name, days: report.daysLeft ?? 0 })
}

const ONBOARDING_KEY = 'xm.ui.onboarding-dismissed'
const onboardingDismissed = ref(localStorage.getItem(ONBOARDING_KEY) === 'true')
/** Only while there is nothing to start: once a node exists the card is noise. */
const showOnboarding = computed(
  () => nodes.loaded && nodes.items.length === 0 && !onboardingDismissed.value,
)

function dismissOnboarding(): void {
  onboardingDismissed.value = true
  localStorage.setItem(ONBOARDING_KEY, 'true')
}

function goToSubscriptions(): void {
  void router.push({ name: 'subscriptions' })
}

const KERNEL_HINT_KEY = 'xm.ui.kernelHintDismissed'
const kernelHintDismissed = ref(localStorage.getItem(KERNEL_HINT_KEY) === 'true')

/**
 * The folder the hint names: the backend's own path, not a guessed
 * `%APPDATA%` string. A custom binary override is looked up somewhere else, and
 * sending the user to the default folder would be worse than saying nothing.
 */
const kernelHintDir = computed(() => kernelDirectory(engine.currentBinary?.path ?? ''))

/**
 * First launch with no kernel on disk.
 *
 * Hidden while a kernel runs — it was launched from a file, so nothing is
 * missing — and hidden once dismissed, because Settings repeats all of it and a
 * bar that cannot be closed turns a hint into nagging.
 */
const kernelHintVisible = computed(() =>
  showKernelHint({
    missing: engine.binaryMissing,
    running: engine.running,
    dismissed: kernelHintDismissed.value,
  }),
)

function dismissKernelHint(): void {
  kernelHintDismissed.value = true
  localStorage.setItem(KERNEL_HINT_KEY, 'true')
}

/**
 * Settings → the kernel block.
 *
 * Deliberately not `kernelInstall`: the startup check says "a newer build
 * exists", and swapping the binary under a live tunnel stays the user's click.
 */
function openKernelSettings(): void {
  void router.push({ name: 'settings', hash: '#kernel' })
}

/**
 * What the daily lookup found for the selected kernel, if it is news.
 *
 * A null `current` means "no binary here yet", which is the hint bar's job, not
 * an update; equal versions mean nothing to say. The version strings are the
 * backend's own, never a local guess.
 */
const kernelUpdate = computed<{ version: string; current: string } | null>(() => {
  const release = kernelUpdates.value[engine.kind]
  if (!release?.current || release.current === release.version) return null
  return { version: release.version, current: release.current }
})

const engineLabel = computed(() => {
  const kind = engine.status?.kind
  if (!kind) return t('common.stopped')
  return kind === 'xray' ? 'Xray' : 'sing-box'
})

/**
 * The inbound side is either one mixed port or two split ones. Keep the old
 * "入站端口 / 7890" wording when the two protocols share a port, and switch to
 * "HTTP 7891 / SOCKS 7892" only when the settings actually split them.
 */
const inboundFact = computed(() => {
  const status = engine.status
  if (!status) return { label: t('home.fact.mixed'), value: '—' }
  if (status.http_port === status.socks_port) {
    return { label: t('home.fact.mixed'), value: String(status.http_port) }
  }
  return {
    label: t('home.fact.inboundSplit'),
    value: `${status.http_port} / ${status.socks_port}`,
  }
})

const runningNodeName = computed(() => engine.status?.node_name || '—')

const canStart = computed(() => !engine.running && !engine.busy && selectedNodeId.value !== '')

/**
 * sing-box 1.14 cannot complete the REALITY handshake with an Xray 26 server
 * (measured 2026-11-22, see docs/ACCEPTANCE.md). The kernel reports
 * `reality verification failed` and the user sees "no internet"; saying so
 * before the click is cheaper than explaining the log afterwards.
 */
/**
 * The version line for the kernel the user has selected.
 *
 * Shown only once the kernel has answered `version`: an empty string here means
 * "not asked yet or not installed", and a placeholder would read as a failure.
 */
const kernelVersion = computed(() => engine.versions[engine.kind] ?? '')

/**
 * Switching kernels has to re-ask for that kernel's version.
 *
 * Versions are probed on mount for every installed binary, but a user who
 * switches to a kernel installed after launch (or whose probe failed) would
 * otherwise stare at an empty line that looks like a broken binary.
 */
watch(
  () => engine.kind,
  (target) => {
    if (!engine.versions[target] && engine.currentBinary?.present) void engine.refreshVersion(target)
  },
)

const realityOnSingBox = computed(() => {
  if (engine.kind !== 'sing-box') return false
  const node = nodes.items.find((item) => item.id === selectedNodeId.value)
  return node?.security?.kind === 'Reality'
})

const kinds: { value: EngineKind; label: string }[] = [
  { value: 'xray', label: 'Xray' },
  { value: 'sing-box', label: 'sing-box' },
]

/**
 * Rule / global / direct, on the screen people actually look at.
 *
 * The mode is baked into the config at render time, so it cannot change under
 * a running kernel — the control is disabled while running rather than
 * silently doing nothing. Changing it from idle applies to the next start,
 * which is the very next thing the user does anyway.
 */
const modes: { value: ProxyMode; labelKey: string }[] = [
  { value: 'rule', labelKey: 'settings.mode.rule' },
  { value: 'global', labelKey: 'settings.mode.global' },
  { value: 'direct', labelKey: 'settings.mode.direct' },
]
const modeSaving = ref(false)

/**
 * Switching under a running kernel is a restart on the same ports: the backend
 * swaps the process and keeps the proxy takeover up, so the cost is a second of
 * dropped connections instead of a stop, a click, and a start.
 */
async function applyLive(): Promise<void> {
  if (!engine.running || !selectedNodeId.value) return
  delayError.value = null
  try {
    await engine.apply(selectedNodeId.value)
  } catch (error) {
    delayError.value = describeError(error)
  }
}

function onNodeChange(): void {
  void applyLive()
}

function pickKind(value: EngineKind): void {
  if (engine.kind === value) return
  engine.kind = value
  void applyLive()
}

async function setMode(mode: ProxyMode): Promise<void> {
  const current = settings.settings
  if (!current || current.mode === mode) return
  modeSaving.value = true
  delayError.value = null
  try {
    await settings.save({ ...current, mode })
    await applyLive()
  } catch (error) {
    delayError.value = describeError(error)
  } finally {
    modeSaving.value = false
  }
}

onMounted(() => {
  // Wait for the visibility probe before the first read: a window started into
  // the tray must not spend a request that nobody can see the result of. The
  // probe is one IPC round-trip, and it is the earliest honest answer available.
  void initQuiet().then(() => {
    if (!live || quiet.value) return
    void loadHistory()
    // Once a minute is enough: the record itself is per-minute.
    startHistoryTimer()
  })
})

onUnmounted(() => {
  live = false
  stopHistoryTimer()
})

onMounted(async () => {
  try {
    info.value = await api.appInfo()
  } catch (cause) {
    infoError.value = describeError(cause)
  }
  await nodes.load()
  void settings.load()
  void subscriptions.load()
  await engine.refreshBinaries()
  // Asked on arrival so the card has a value without a click; the route is
  // decided in Rust from the live kernel state, not from what this page shows.
  void refreshExitIp()
  if (!selectedNodeId.value && nodes.items.length > 0) {
    selectedNodeId.value = nodes.items[0].id
  }
})

function resetDelay(): void {
  realDelay.value = null
  realSpeed.value = null
  delayError.value = null
}

/**
 * Throughput: read a body through the tunnel for a few seconds and divide.
 *
 * Separate from the delay probe on purpose — a node can answer in 80 ms and
 * still crawl on bulk transfer, and only a measured download shows that. Shares
 * `delayError` because both failures mean the same thing to the user: the
 * tunnel is not carrying traffic.
 */
async function testSpeed(): Promise<void> {
  speedPending.value = true
  delayError.value = null
  try {
    realSpeed.value = await api.proxySpeed()
  } catch (error) {
    realSpeed.value = null
    delayError.value = describeError(error)
  } finally {
    speedPending.value = false
  }
}

/**
 * Real delay: one request through the running tunnel.
 *
 * The per-node number in the node list is a TCP handshake to the node; this one
 * includes the kernel, the tunnel and the node's own DNS, which is the number
 * that predicts how a page load feels.
 */
async function testRealDelay(): Promise<void> {
  delayPending.value = true
  delayError.value = null
  try {
    realDelay.value = await api.proxyLatency()
  } catch (error) {
    realDelay.value = null
    delayError.value = describeError(error)
  } finally {
    delayPending.value = false
  }
}

/**
 * The address the outside world sees.
 *
 * Kept together with the moment it was read: an address probed before the
 * tunnel came up belongs to this machine, and a stale one shown without a time
 * would be read as "this is my IP right now". A failed attempt clears both: an
 * address and a "direct" badge left beside a fresh error would claim a route
 * this attempt never saw.
 */
const exit = ref<ExitIp | null>(null)
const exitAt = ref<number | null>(null)
const exitPending = ref(false)
const exitError = ref<string | null>(null)

async function refreshExitIp(): Promise<void> {
  if (exitPending.value) return
  exitPending.value = true
  exitError.value = null
  try {
    exit.value = await api.exitIp()
    exitAt.value = Date.now()
  } catch (problem) {
    // Nothing was proved by this attempt, so nothing is left to show but the
    // error: a remembered address would sit next to it wearing a route badge.
    exit.value = null
    exitAt.value = 0
    exitError.value = describeError(problem)
  } finally {
    exitPending.value = false
  }
}

async function startEngine() {
  resetDelay()
  if (!selectedNodeId.value) return
  try {
    await engine.start(selectedNodeId.value)
  } catch {
    // The store already captured a readable message.
  }
}

async function stopEngine() {
  resetDelay()
  try {
    await engine.stop()
  } catch {
    // Same: the message is in `engine.error`.
  }
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.home.title') }}</h1>
        <p class="page-subtitle">{{ t('page.home.subtitle') }}</p>
      </div>
      <div class="row">
        <span class="badge" :class="engine.running ? 'badge-success' : ''">
          {{ engine.running ? t('common.running') : t('common.stopped') }}
        </span>
      </div>
    </header>

    <p v-if="infoError" class="alert">{{ infoError }}</p>
    <p v-if="engine.error" class="alert">{{ engine.error }}</p>

    <!-- A plan that runs out in two days is page news, not a footnote of the
         start card: it used to scroll away with the control panel. -->
    <div v-if="quotaNotices.length" class="alert alert-warn quota-notices">
      <div class="quota-head">{{ t('home.quota.title') }}</div>
      <p v-for="notice in quotaNotices" :key="notice.id" class="quota-line">
        {{ quotaText(notice) }}
      </p>
      <RouterLink :to="{ name: 'subscriptions' }" class="quota-link">
        {{ t('home.quota.open') }}
      </RouterLink>
    </div>

    <!-- No kernel on disk: one bar that says which file, which folder, and the
         two ways out. It disappears the moment a binary exists, so the start
         button never fails with the backend's raw path error. -->
    <div v-if="kernelHintVisible" class="alert alert-warn kernel-hint">
      <div class="row kernel-hint-head">
        <strong>{{ t('home.kernelHint.title') }}</strong>
        <span class="spacer" />
        <button type="button" class="btn btn-sm" @click="dismissKernelHint()">
          {{ t('home.kernelHint.dismiss') }}
        </button>
      </div>
      <p class="kernel-hint-line">
        {{ t('home.kernelHint.missing', { name: engine.currentBinary?.name ?? '' }) }}
      </p>
      <code class="kernel-hint-dir mono">{{ kernelHintDir }}</code>
      <p class="kernel-hint-line">
        {{ t('home.kernelHint.how', { name: engine.currentBinary?.name ?? '' }) }}
      </p>
      <div class="row">
        <button type="button" class="btn btn-primary btn-sm" @click="openKernelSettings()">
          {{ t('home.kernelHint.settings') }}
        </button>
        <button type="button" class="btn btn-sm" @click="engine.openEnginesDir()">
          <AppIcon name="inbox" :size="15" />
          {{ t('common.openDir') }}
        </button>
      </div>
    </div>

    <!-- The daily check found a newer official build. One line and a door to
         Settings; nothing is installed from here. -->
    <div v-if="kernelUpdate" class="alert alert-info kernel-update">
      <div class="row">
        <span>
          {{ t('home.kernelUpdate.new', { version: kernelUpdate.version, current: kernelUpdate.current }) }}
        </span>
        <span class="spacer" />
        <button type="button" class="btn btn-sm" @click="openKernelSettings()">
          {{ t('home.kernelHint.settings') }}
        </button>
      </div>
      <p class="kernel-update-note">{{ t('home.kernelUpdate.note') }}</p>
    </div>

    <!-- First run: three steps, then it never comes back. -->
    <section v-if="showOnboarding" class="glass onboard">
      <div class="row">
        <h2 class="section-title">{{ t('onboarding.title') }}</h2>
        <span class="spacer" />
        <button type="button" class="btn btn-sm" @click="dismissOnboarding()">
          {{ t('onboarding.dismiss') }}
        </button>
      </div>
      <ol class="onboard-steps">
        <li>{{ t('onboarding.step.nodes') }}</li>
        <li>{{ t('onboarding.step.engines') }}</li>
        <li>{{ t('onboarding.step.start') }}</li>
      </ol>
      <div class="row">
        <button type="button" class="btn btn-primary btn-sm" @click="goToSubscriptions()">
          <AppIcon name="cloud" :size="15" />
          {{ t('onboarding.goNodes') }}
        </button>
        <button type="button" class="btn btn-sm" @click="engine.openEnginesDir()">
          <AppIcon name="inbox" :size="15" />
          {{ t('onboarding.openEngines') }}
        </button>
      </div>
    </section>

    <section class="surface-plain panel">
      <h2 class="section-title">{{ t('home.control.title') }}</h2>

      <div v-if="!nodes.loaded" class="muted">{{ t('common.loading') }}</div>
      <div v-else-if="nodes.items.length === 0" class="muted">
        {{ t('home.control.noNodes') }}
      </div>

      <div v-else class="control-row">
        <label class="field control-node">
          <span class="field-label">{{ t('home.control.node') }}</span>
          <select v-model="selectedNodeId" class="select" :disabled="engine.busy" @change="onNodeChange">
            <option v-for="node in nodes.items" :key="node.id" :value="node.id">
              {{ node.name }} · {{ node.server }}:{{ node.port }}
            </option>
          </select>
        </label>

        <div class="field control-engine">
          <span class="field-label">{{ t('home.control.engine') }}</span>
          <div class="segmented">
            <button
              v-for="item in kinds"
              :key="item.value"
              type="button"
              :class="{ active: engine.kind === item.value }"
              :aria-pressed="engine.kind === item.value"
              :disabled="engine.busy"
              @click="pickKind(item.value)"
            >
              {{ item.label }}
            </button>
          </div>
          <p v-if="engine.running" class="field-hint">{{ t('home.control.live') }}</p>
        </div>

        <div class="field control-mode">
          <span class="field-label">{{ t('settings.mode') }}</span>
          <div class="segmented">
            <button
              v-for="item in modes"
              :key="item.value"
              type="button"
              :class="{ active: settings.settings?.mode === item.value }"
              :aria-pressed="settings.settings?.mode === item.value"
              :disabled="engine.busy || modeSaving || !settings.settings"
              @click="setMode(item.value)"
            >
              {{ t(item.labelKey) }}
            </button>
          </div>
          <p v-if="engine.running" class="field-hint">{{ t('home.mode.live') }}</p>
        </div>
      </div>

      <p v-if="realityOnSingBox" class="alert alert-info">{{ t('home.realityHint') }}</p>

      <div class="control-row">
        <div class="field control-actions">
          <span class="field-label">&nbsp;</span>
          <div class="row">
            <button
              v-if="!engine.running"
              class="btn btn-primary"
              type="button"
              :disabled="!canStart || engine.binaryMissing"
              @click="startEngine"
            >
              <AppIcon name="play" :size="15" />
              {{ engine.busy ? t('common.starting') : t('common.start') }}
            </button>
            <button v-else class="btn btn-danger" type="button" :disabled="engine.busy" @click="stopEngine">
              <AppIcon name="stop" :size="15" />
              {{ engine.busy ? t('common.stopping') : t('common.stop') }}
            </button>
            <button class="btn btn-sm" type="button" @click="configOpen = true">{{ t('home.control.config') }}</button>
            <button class="btn btn-sm" type="button" @click="logOpen = true">{{ t('home.control.log') }}</button>
            <button
              v-if="engine.running"
              class="btn btn-sm"
              type="button"
              :disabled="delayPending"
              @click="testRealDelay()"
            >
              {{ delayPending ? t('home.realDelay.testing') : t('home.control.realDelay') }}
            </button>
            <button
              v-if="engine.running"
              class="btn btn-sm"
              type="button"
              :disabled="speedPending"
              @click="testSpeed()"
            >
              {{ speedPending ? t('home.speed.testing') : t('home.control.speed') }}
            </button>
          </div>
          <p v-if="delayError" class="field-hint">{{ delayError }}</p>
          <p v-else-if="realDelay !== null" class="field-hint">
            {{ t('home.realDelay.result', { ms: realDelay }) }}
          </p>
          <p v-if="realSpeed !== null && !delayError" class="field-hint">
            {{ t('home.speed.result', { rate: formatRate(realSpeed) }) }}
          </p>
        </div>
      </div>

      <p v-if="kernelVersion" class="field-hint kernel-version">{{ t('home.kernelVersion', { version: kernelVersion }) }}</p>

      <dl v-if="engine.status?.running" class="facts">
        <div><dt>PID</dt><dd class="mono">{{ engine.status.pid }}</dd></div>
        <div><dt>{{ inboundFact.label }}</dt><dd class="mono">{{ inboundFact.value }}</dd></div>
        <div><dt>{{ t('home.fact.api') }}</dt><dd class="mono">{{ engine.status.api_port }}</dd></div>
        <div class="fact-wide"><dt>{{ t('home.fact.config') }}</dt><dd class="mono ellipsis">{{ engine.status.config_path }}</dd></div>
      </dl>

      <p v-else-if="engine.status?.last_exit_code !== null && engine.status?.last_exit_code !== undefined" class="alert">
        {{ t('home.lastExit', { code: engine.status.last_exit_code ?? '—' }) }}
      </p>

      <!-- Which address the outside world sees. Deliberately outside the
           `facts` block above: it is just as useful before the tunnel is up,
           and then the badge says plainly that the answer is this machine. -->
      <div class="exit-ip">
        <div class="row">
          <span class="field-label">{{ t('home.exitIp.title') }}</span>
          <span class="spacer" />
          <button
            class="btn btn-sm"
            type="button"
            :disabled="exitPending"
            @click="refreshExitIp()"
          >
            <AppIcon name="refresh" :size="14" />
            {{ exitPending ? t('home.exitIp.checking') : t('home.exitIp.refresh') }}
          </button>
        </div>
        <div class="row exit-ip-line" aria-live="polite">
          <template v-if="exit">
            <span class="mono exit-ip-value">{{ exit.ip }}</span>
            <span class="badge" :class="exit.via === 'proxy' ? 'badge-accent' : ''">
              {{ exit.via === 'proxy' ? t('home.exitIp.proxy') : t('home.exitIp.direct') }}
            </span>
            <span class="muted small">{{ t('home.exitIp.updated', { when: formatWhen(exitAt) }) }}</span>
          </template>
          <span v-else class="muted small">{{ t('home.exitIp.never') }}</span>
        </div>
        <p v-if="exitError" class="exit-ip-failed">
          {{ t('home.exitIp.failed', { reason: exitError }) }}
        </p>
      </div>

      <ConfigDialog
        :open="configOpen"
        :kind="engine.kind"
        :node-id="selectedNodeId"
        @close="configOpen = false"
      />
      <LogDialog :open="logOpen" :kind="engine.kind" @close="logOpen = false" />
    </section>

    <section class="stat-grid">
      <article class="glass stat-card">
        <div class="stat-head">
          <AppIcon name="power" :size="16" />
          <span>{{ t('home.stat.engine') }}</span>
        </div>
        <div class="stat-value" :class="{ 'stat-value-ok': engine.running }">{{ engineLabel }}</div>
        <div class="stat-hint">
          {{
            engine.running
              ? t('home.nodeLine', { name: runningNodeName })
              : t('home.noEngine')
          }}
        </div>
      </article>

      <article class="glass stat-card">
        <div class="stat-head">
          <AppIcon name="arrow-down" :size="16" />
          <span>{{ t('home.stat.down') }}</span>
        </div>
        <div class="stat-value">{{ engine.running ? formatRate(engine.rates.down) : '—' }}</div>
        <div class="stat-hint mono">
          {{
            t('home.total', {
              value: engine.sample ? formatBytes(engine.sample.down_total) : '—',
            })
          }}
        </div>
      </article>

      <article class="glass stat-card">
        <div class="stat-head">
          <AppIcon name="arrow-up" :size="16" />
          <span>{{ t('home.stat.up') }}</span>
        </div>
        <div class="stat-value">{{ engine.running ? formatRate(engine.rates.up) : '—' }}</div>
        <div class="stat-hint mono">
          {{
            t('home.total', {
              value: engine.sample ? formatBytes(engine.sample.up_total) : '—',
            })
          }}
        </div>
      </article>

      <article class="glass stat-card">
        <div class="stat-head">
          <AppIcon name="nodes" :size="16" />
          <span>{{ t('home.stat.nodes') }}</span>
        </div>
        <div class="stat-value">{{ nodes.loaded ? nodes.items.length : '—' }}</div>
        <div class="stat-hint">{{ nodes.items.length ? t('home.nodesCount') : t('home.nodesEmptyHint') }}</div>
      </article>
    </section>

    <section class="surface-plain panel">
      <div class="row">
        <h2 class="section-title">{{ t('home.chart.title') }}</h2>
        <span class="spacer" />
        <span class="muted small mono">
          ↓ {{ engine.running ? formatRate(engine.rates.down) : '—' }} ·
          ↑ {{ engine.running ? formatRate(engine.rates.up) : '—' }}
        </span>
      </div>
      <TrafficChart :points="engine.history" />
      <div class="session-row">
        <span class="muted small">{{ t('home.session.label') }}</span>
        <span class="mono small">
          ↓ {{ formatBytes(engine.session.down) }} · ↑ {{ formatBytes(engine.session.up) }}
        </span>
        <span class="spacer" />
        <button
          class="btn btn-sm"
          type="button"
          :disabled="!engine.running"
          @click="engine.resetSession()"
        >
          {{ t('home.session.reset') }}
        </button>
      </div>
    </section>

    <section class="surface-plain panel">
      <div class="row">
        <h2 class="section-title">{{ t('home.history.title') }}</h2>
        <span class="spacer" />
        <div class="segmented">
          <button
            type="button"
            :class="{ active: historyWindow === 60 }"
            @click="setHistoryWindow(60)"
          >
            {{ t('home.history.hour') }}
          </button>
          <button
            type="button"
            :class="{ active: historyWindow === 1440 }"
            @click="setHistoryWindow(1440)"
          >
            {{ t('home.history.day') }}
          </button>
        </div>
        <button
          class="btn btn-sm"
          type="button"
          :disabled="!history.length"
          @click="clearHistory"
        >
          {{ t('home.history.clear') }}
        </button>
      </div>
      <ul v-if="historyBars.length" class="history-bars" role="img" :aria-label="t('home.history.title')">
        <li v-for="bar in historyBars" :key="bar.key" :title="bar.tip">
          <span class="history-bar" :style="{ height: bar.height + '%' }" />
        </li>
      </ul>
      <p v-else class="muted small history-empty">{{ t('home.history.empty') }}</p>
      <div v-if="historyBars.length" class="muted small mono history-totals">
        {{
          t('home.history.totals', {
            down: formatBytes(historyTotals.down),
            up: formatBytes(historyTotals.up),
          })
        }}
      </div>
    </section>

    <section v-if="showProgress" class="surface-plain panel">
      <h2 class="section-title">{{ t('home.progress.title') }}</h2>
      <ol class="steps">
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.m0') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.m1') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.m2') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.m3') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.sub') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.groups') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.i18n') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.ruleSets') }}</span>
        </li>
        <li class="step done">
          <AppIcon name="check" :size="15" />
          <span>{{ t('home.progress.polish') }}</span>
        </li>
        <li class="step">
          <span class="step-dot" />
          <span>{{ t('home.progress.left') }}</span>
        </li>
      </ol>
      <p class="muted small">
        {{
          info
            ? t('home.footer', {
                version: info.version,
                platform: info.platform,
                arch: info.arch,
              })
            : t('home.footerLoading', { version: '—' })
        }}
      </p>
    </section>
  </div>
</template>

<style scoped>
.onboard {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 18px;
  border-radius: var(--radius-card);
}

.onboard-steps {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding-left: 20px;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-secondary);
}

.stat-grid {
  display: grid;
  /* Fixed counts, never `auto-fit`: auto-fit happily leaves three cards on the
     first row and the fourth alone below, which reads like a broken layout.
     Four across when there is room, two by two when there is not. */
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 12px;
}

@media (max-width: 1120px) {
  .stat-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

.session-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border-subtle);
}

.stat-card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 16px;
}

.stat-head {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.stat-value {
  /* Numbers change every tick; proportional digits make the row jitter. */
  font-variant-numeric: tabular-nums;
  font-size: 24px;
  font-weight: 650;
  letter-spacing: -0.02em;
}

.stat-value-ok {
  color: var(--success);
}

.stat-hint {
  font-size: 12px;
  color: var(--text-tertiary);
}

.panel {
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.control-row {
  display: flex;
  flex-wrap: wrap;
  align-items: flex-end;
  gap: 14px;
}

/* Plan notices: the alert supplies the tone, this only lays the lines out. */
.quota-notices {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.quota-head {
  font-weight: 600;
}

.quota-line {
  margin: 0;
}

/* Kernel hint: the alert supplies tone, these rules only lay the sentences out.
   The folder is its own line because it gets copied into Explorer by hand. */
.kernel-hint {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.kernel-hint-head {
  align-items: center;
}

.kernel-hint-line {
  margin: 0;
  line-height: 1.6;
}

.kernel-hint-dir {
  font-size: 13px;
  overflow-wrap: anywhere;
  color: var(--text-primary);
}

.kernel-update-note {
  margin: 6px 0 0;
  color: var(--text-secondary);
}

.quota-link {
  align-self: flex-start;
  font-size: 12px;
}

.control-node {
  flex: 1;
  min-width: 240px;
}

/*
 * Fixed width so the segmented control does not slide sideways when the
 * "locked while running" hint appears: a button that moves between the two
 * states is a button people mis-click, and it broke my own click automation.
 */
.control-engine {
  width: 236px;
  flex: none;
}

.control-mode {
  width: 224px;
  flex: none;
}

.control-mode .field-hint {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.control-engine .field-hint {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.control-actions {
  margin-left: auto;
}

.facts {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 10px 18px;
  margin: 0;
  padding-top: 12px;
  border-top: 1px solid var(--border-subtle);
}

.facts div {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

.facts dt {
  font-size: 11px;
  color: var(--text-tertiary);
}

.facts dd {
  margin: 0;
  font-size: 13px;
}

.fact-wide {
  grid-column: 1 / -1;
}

.ellipsis {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Same separator as `.facts`: the two read as rows of the same panel. */
.exit-ip {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid var(--border-subtle);
}

.exit-ip-line {
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
}

.exit-ip-value {
  font-size: 13px;
}

/* "Could not ask" is not a broken tunnel: muted, not red — the same tone the
   About page uses for an unreachable release feed. */
.exit-ip-failed {
  margin: 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
  overflow-wrap: anywhere;
}

.steps {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.step {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-secondary);
}

.step.done {
  color: var(--text-primary);
}

.step.done svg {
  color: var(--success);
}

.step-dot {
  width: 7px;
  height: 7px;
  margin: 0 4px;
  border-radius: 50%;
  background: var(--border-strong);
}

.small {
  font-size: 12px;
}

.kernel-version {
  margin: 0;
}

/* History: one bar per minute, normalised to the busiest minute in the window.
   With 1440 bars the gap has to shrink or the bars become hairlines. */
.history-bars {
  display: flex;
  align-items: flex-end;
  gap: 2px;
  height: 64px;
  margin: 0;
  padding: 0;
  list-style: none;
  overflow: hidden;
}

.history-bars li {
  flex: 1 1 0;
  min-width: 0;
  height: 100%;
  display: flex;
  align-items: flex-end;
}

.history-bar {
  display: block;
  width: 100%;
  border-radius: 2px 2px 0 0;
  background: color-mix(in srgb, var(--accent) 78%, transparent);
}

.history-empty {
  margin: 0;
}

.history-totals {
  margin-top: 8px;
}

</style>
