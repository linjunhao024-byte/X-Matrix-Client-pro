<script setup lang="ts">
/**
 * Home: the one screen that answers "is it on, and is traffic moving".
 *
 * Every number here comes from the Rust runtime; nothing is derived locally
 * except the transfer rates, which are a difference between two samples.
 */
import { computed, defineAsyncComponent, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import AppIcon from '../components/AppIcon.vue'

// ECharts is ~160 kB gzipped: load it after the page has painted instead of
// making the first screen wait for a chart that is empty anyway.
const TrafficChart = defineAsyncComponent(() => import('../components/TrafficChart.vue'))
const LogDialog = defineAsyncComponent(() => import('../components/LogDialog.vue'))
const ConfigDialog = defineAsyncComponent(() => import('../components/ConfigDialog.vue'))
import { t } from '../i18n'
import { api, describeError, type AppInfo, type EngineKind } from '../services/contract'
import { formatBytes, formatRate } from '../services/format'
import { useEngineStore } from '../stores/engine'
import { useNodesStore } from '../stores/nodes'

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

const router = useRouter()

const nodes = useNodesStore()
const engine = useEngineStore()

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

const engineLabel = computed(() => {
  const kind = engine.status?.kind
  if (!kind) return t('common.stopped')
  return kind === 'xray' ? 'Xray' : 'sing-box'
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

onMounted(async () => {
  try {
    info.value = await api.appInfo()
  } catch (cause) {
    infoError.value = describeError(cause)
  }
  await nodes.load()
  await engine.refreshBinaries()
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
          <select v-model="selectedNodeId" class="select" :disabled="engine.running">
            <option v-for="node in nodes.items" :key="node.id" :value="node.id">
              {{ node.name }} · {{ node.server }}:{{ node.port }}
            </option>
          </select>
        </label>

        <div class="field">
          <span class="field-label">{{ t('home.control.engine') }}</span>
          <div class="segmented">
            <button
              v-for="item in kinds"
              :key="item.value"
              type="button"
              :class="{ active: engine.kind === item.value }"
              :aria-pressed="engine.kind === item.value"
              :disabled="engine.running"
              @click="engine.kind = item.value"
            >
              {{ item.label }}
            </button>
          </div>
          <p v-if="engine.running" class="field-hint">{{ t('home.control.locked') }}</p>
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

      <div v-if="engine.binaryMissing && engine.currentBinary" class="alert">
        <div class="row">
          <span>
            {{ t('home.binaryMissing', { name: engine.currentBinary.name }) }}
          </span>
          <button class="btn btn-sm" type="button" @click="engine.openEnginesDir()">
            {{ t('common.openDir') }}
          </button>
        </div>
      </div>

      <dl v-if="engine.status?.running" class="facts">
        <div><dt>PID</dt><dd class="mono">{{ engine.status.pid }}</dd></div>
        <div><dt>{{ t('home.fact.mixed') }}</dt><dd class="mono">{{ engine.status.mixed_port }}</dd></div>
        <div><dt>{{ t('home.fact.api') }}</dt><dd class="mono">{{ engine.status.api_port }}</dd></div>
        <div class="fact-wide"><dt>{{ t('home.fact.config') }}</dt><dd class="mono ellipsis">{{ engine.status.config_path }}</dd></div>
      </dl>

      <p v-else-if="engine.status?.last_exit_code !== null && engine.status?.last_exit_code !== undefined" class="alert">
        {{ t('home.lastExit', { code: engine.status.last_exit_code ?? '—' }) }}
      </p>

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

.control-node {
  flex: 1;
  min-width: 240px;
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
</style>
