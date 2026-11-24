<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import StaleConfig from '../components/StaleConfig.vue'
import ToggleSwitch from '../components/ToggleSwitch.vue'
import { availableLocales, locale, setLocale, t } from '../i18n'
import {
  api,
  describeError,
  type EngineBinary,
  type EngineKind,
  type ProxyMode,
  type Settings_Serialize,
} from '../services/contract'
import { useEngineStore } from '../stores/engine'
import { useSettingsStore } from '../stores/settings'
import { useSystemProxyStore } from '../stores/sysproxy'

const store = useSettingsStore()
const proxy = useSystemProxyStore()
const engine = useEngineStore()
const draft = ref<Settings_Serialize | null>(null)
const error = ref<string | null>(null)
const saved = ref(false)
const autostart = ref<boolean | null>(null)
/** Result line for backup/restore: what happened, in one sentence. */
const backupNote = ref<string | null>(null)
const backupBusy = ref(false)

/**
 * Compare resolver lists by content, not spelling: the kernel gets the parsed
 * list, so "1.1.1.1, 8.8.8.8" and "1.1.1.1 ,8.8.8.8" are the same setting.
 */
function dnsList(raw: string): string {
  return raw
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry !== '')
    .join(',')
}

/**
 * The i18n keys of every saved setting the running kernel was started with
 * differently.
 *
 * Ports, the log level, LAN sharing, the resolver list and multiplexing are all
 * baked into the rendered config at start, so saving a new value changes only
 * the document until the kernel restarts; "已保存" alone would look broken. One
 * notice for all of them, in panel order, so the page never stacks near
 * duplicates. A stopped kernel has nothing stale — the next start renders the
 * saved settings anyway — and an unloaded document cannot be compared.
 * Deliberately compares the saved settings, not `draft`: a value the user typed
 * but has not saved is not a divergence yet. The stream cap only counts while
 * both sides multiplex: with the feature off it is not part of the config.
 */
const staleItems = computed<string[]>(() => {
  const saved = store.settings
  const status = engine.status
  if (!engine.running || !store.loaded || saved === null || status === null) return []

  const items: string[] = []
  if (saved.mixed_port !== status.mixed_port || saved.api_port !== status.api_port) {
    items.push('settings.stale.itemPorts')
  }
  if (saved.log_level !== status.log_level) items.push('settings.stale.itemLog')
  if (saved.allow_lan !== status.allow_lan) items.push('settings.stale.itemLan')
  if (
    saved.mux_enabled !== status.mux_enabled ||
    (saved.mux_enabled && saved.mux_concurrency !== status.mux_concurrency)
  ) {
    items.push('settings.stale.itemMux')
  }
  if (dnsList(saved.dns_servers) !== dnsList(status.dns_servers)) {
    items.push('settings.stale.itemDns')
  }
  return items
})

const staleMessage = computed(() =>
  t('settings.staleList', {
    items: staleItems.value.map((key) => t(key)).join(t('settings.stale.sep')),
  }),
)

const modes: { value: ProxyMode; labelKey: string }[] = [
  { value: 'rule', labelKey: 'settings.mode.rule' },
  { value: 'global', labelKey: 'settings.mode.global' },
  { value: 'direct', labelKey: 'settings.mode.direct' },
]

/**
 * Each language names itself, so these labels are not translated.
 */
const locales = availableLocales()

/** Kernel verbosity. `warn` is the default because a debug log grows fast. */
/** `0` is "only when I press the button" — see `Settings::auto_test_minutes`. */
const testIntervals: { value: number; labelKey: string }[] = [
  { value: 0, labelKey: 'settings.autoTest.off' },
  { value: 5, labelKey: 'settings.autoTest.min5' },
  { value: 15, labelKey: 'settings.autoTest.min15' },
  { value: 60, labelKey: 'settings.autoTest.min60' },
]

const logLevels: { value: Settings_Serialize['log_level']; labelKey: string }[] = [
  { value: 'warn', labelKey: 'settings.logLevel.warn' },
  { value: 'info', labelKey: 'settings.logLevel.info' },
  { value: 'debug', labelKey: 'settings.logLevel.debug' },
]

const autostartState = computed(() =>
  autostart.value === null
    ? t('settings.autostartUnknown')
    : autostart.value
      ? t('settings.autostartOn')
      : t('settings.autostartOff'),
)

/**
 * Export the whole configuration to a file the user picks.
 *
 * A cancelled dialog is not a failure, so it says nothing rather than painting
 * a red line.
 */
async function exportConfig(): Promise<void> {
  backupBusy.value = true
  backupNote.value = null
  try {
    const path = await api.backupSave()
    if (path) backupNote.value = t('settings.backup.saved', { path })
  } catch (error) {
    backupNote.value = describeError(error)
  } finally {
    backupBusy.value = false
  }
}

/**
 * Replace the configuration from a backup file.
 *
 * The kernel must be stopped first (the backend refuses otherwise), because a
 * running kernel was rendered from the settings we are about to replace.
 */
async function restoreConfig(): Promise<void> {
  backupBusy.value = true
  backupNote.value = null
  try {
    const summary = await api.backupLoad()
    if (summary) {
      // specta renders numeric fields as `number | null`; a count that arrived
      // is never null, but the type says otherwise.
      backupNote.value = t('settings.backup.restored', {
        nodes: summary.nodes ?? 0,
        rules: summary.rules ?? 0,
        groups: summary.groups ?? 0,
        ruleSets: summary.rule_sets ?? 0,
      })
      // Everything on screen came from the old document.
      await store.load(true)
      if (store.settings) draft.value = { ...store.settings }
    }
  } catch (error) {
    backupNote.value = describeError(error)
  } finally {
    backupBusy.value = false
  }
}

async function refreshAutostart() {
  try {
    autostart.value = await api.autostartStatus()
  } catch {
    autostart.value = null
  }
}

/**
 * The two kernels, in the order a user meets them.
 *
 * `Xray` and `sing-box` are product names, so they are not translated — the
 * same rule the language list follows.
 */
const kernels: { kind: EngineKind; label: string }[] = [
  { kind: 'xray', label: 'Xray' },
  { kind: 'sing-box', label: 'sing-box' },
]

/** Which settings field carries the override for each kernel. */
const overrideField: Record<EngineKind, 'xray_binary' | 'singbox_binary'> = {
  xray: 'xray_binary',
  'sing-box': 'singbox_binary',
}

/** What the runtime would actually start, per kernel, as last saved. */
const kernelBinaries = ref<EngineBinary[]>([])
/** Kind currently talking to the backend — a dialog or a `version` run. */
const kernelBusy = ref<EngineKind | null>(null)
/** One line per kernel: the reported version, or why there is none. */
const kernelNotes = ref<Partial<Record<EngineKind, string>>>({})

const kernelRows = computed(() =>
  kernels.map((kernel) => ({
    ...kernel,
    binary: kernelBinaries.value.find((item) => item.kind === kernel.kind),
    override: (draft.value?.[overrideField[kernel.kind]] ?? '').trim(),
  })),
)

async function refreshKernelBinaries(): Promise<void> {
  try {
    kernelBinaries.value = await api.engineBinaries()
  } catch (cause) {
    kernelNotes.value.xray = describeError(cause)
  }
}

/**
 * Persist one override (or clear it) through the settings store.
 *
 * Saving is what applies the path to the running runtime, so the row must not
 * only change the draft: the next 启动 has to use the chosen file without an
 * app restart. Both the picker and 恢复默认 go through here.
 */
async function saveKernelOverride(kind: EngineKind, path: string): Promise<void> {
  if (!draft.value) return
  kernelBusy.value = kind
  try {
    await store.save({ ...draft.value, [overrideField[kind]]: path })
    draft.value = { ...(store.settings as Settings_Serialize) }
    // The old note described the previous file; it would now be a lie.
    delete kernelNotes.value[kind]
    await refreshKernelBinaries()
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    kernelBusy.value = null
  }
}

async function chooseKernel(kind: EngineKind): Promise<void> {
  if (!draft.value) return
  kernelBusy.value = kind
  let chosen: string | null = null
  try {
    chosen = await api.pickEngineBinary(kind)
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    kernelBusy.value = null
  }
  // Cancelling the dialog is a decision, not a failure: keep everything as is.
  if (chosen) await saveKernelOverride(kind, chosen)
}

async function resetKernel(kind: EngineKind): Promise<void> {
  await saveKernelOverride(kind, '')
}

/**
 * Ask the kernel to report itself.
 *
 * A missing file comes back as an error naming the path — that is the honest
 * answer, and it is shown in the same line instead of a version.
 */
async function probeKernelVersion(kind: EngineKind): Promise<void> {
  kernelBusy.value = kind
  try {
    kernelNotes.value[kind] = await api.engineVersion(kind)
  } catch (cause) {
    kernelNotes.value[kind] = describeError(cause)
  } finally {
    kernelBusy.value = null
  }
}

onMounted(async () => {
  await store.load()
  if (store.settings) draft.value = { ...store.settings }
  await proxy.refresh()
  await refreshAutostart()
  await refreshKernelBinaries()
})

async function save() {
  if (!draft.value) return
  error.value = null
  saved.value = false
  try {
    await store.save({ ...draft.value })
    draft.value = { ...(store.settings as Settings_Serialize) }
    saved.value = true
    await refreshAutostart()
    // Saving may have just taken or released the system proxy (the backend
    // applies it to a running kernel immediately), so the live block below the
    // toggle must show the machine as it is now, not as it was on mount.
    await proxy.refresh()
    window.setTimeout(() => {
      saved.value = false
    }, 2000)
  } catch (cause) {
    error.value = describeError(cause)
  }
}
</script>

<template>
  <div class="page settings-page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.settings.title') }}</h1>
        <p class="page-subtitle">{{ t('page.settings.subtitle') }}</p>
      </div>
      <div class="row">
        <span v-if="saved" class="badge badge-success">{{ t('common.saved') }}</span>
        <button class="btn btn-primary" type="button" :disabled="!draft || store.saving" @click="save">
          {{ store.saving ? t('common.saving') : t('common.save') }}
        </button>
      </div>
    </header>

    <div class="page-body">
    <p v-if="store.error" class="alert">{{ store.error }}</p>
    <p v-if="error" class="alert">{{ error }}</p>

    <section v-if="!draft" class="surface-plain">
      <div class="empty-state">
        <div class="empty-state-title">{{ t('common.loading') }}</div>
      </div>
    </section>

    <template v-else>
      <!-- One notice for the whole page, at the top: it lists every setting the
           running kernel is behind on, so scattering copies next to each panel
           would repeat the same "not live yet" sentence. -->
      <StaleConfig
        :show="staleItems.length > 0"
        :message="staleMessage"
        :apply-label="t('settings.staleListApply')"
      />

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.ports') }}</h2>
        <div class="grid">
          <label class="field">
            <span class="field-label">{{ t('settings.mixed') }}</span>
            <input v-model.number="draft.mixed_port" class="input input-narrow" type="number" min="1024" max="65535" />
            <span class="field-hint">{{ t('settings.mixedHint') }}</span>
          </label>
          <label class="field">
            <span class="field-label">{{ t('settings.api') }}</span>
            <input v-model.number="draft.api_port" class="input input-narrow" type="number" min="1024" max="65535" />
            <span class="field-hint">{{ t('settings.apiHint') }}</span>
          </label>
        </div>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.mode') }}</h2>
        <div class="segmented">
          <button
            v-for="mode in modes"
            :key="mode.value"
            type="button"
            :aria-pressed="draft.mode === mode.value"
            @click="draft.mode = mode.value"
          >
            {{ t(mode.labelKey) }}
          </button>
        </div>
        <p class="field-hint">
          {{ t('settings.modeHint') }}
        </p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.system') }}</h2>
        <ToggleSwitch
          v-model="draft.allow_lan"
          :label="t('settings.allowLan')"
          :description="t('settings.allowLanHint')"
        />
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.system_proxy"
          :label="t('settings.systemProxy')"
          :description="t('settings.systemProxyHint')"
        />
        <div class="proxy-state">
          <span class="muted">
            {{ t('settings.proxyState') }}
            <strong>{{ proxy.enabled ? t('settings.proxyOn') : t('settings.proxyOff') }}</strong>
            <template v-if="proxy.status?.server">{{
              t('settings.proxyServer', { server: proxy.status.server })
            }}</template>
            <template v-if="proxy.held">{{ t('settings.proxyHeldSuffix') }}</template>
          </span>
          <button
            v-if="proxy.held"
            class="btn btn-sm"
            type="button"
            :disabled="proxy.busy"
            @click="proxy.restore()"
          >
            {{ t('settings.restoreProxy') }}
          </button>
        </div>
        <p v-if="proxy.error" class="alert">{{ proxy.error }}</p>
        <label class="field">
          <span class="field-label">{{ t('settings.bypass') }}</span>
          <input v-model="draft.proxy_bypass" class="input input-mono" type="text" />
        </label>
        <p class="field-hint">{{ t('settings.bypassHint') }}</p>
        <p class="field-hint">
          {{ t('settings.proxyIntent') }}
          {{ t('settings.proxyWarning') }}
        </p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.auto_start"
          :label="t('settings.autostart')"
          :description="t('settings.autostartHint')"
        />
        <p class="field-hint">
          {{ t('settings.autostartState', { state: autostartState }) }}
          {{ t('settings.autostartNote') }}
        </p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.auto_connect"
          :label="t('settings.autoConnect')"
          :description="t('settings.autoConnectHint')"
        />
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.start_minimized"
          :label="t('settings.startMinimized')"
          :description="t('settings.startMinimizedHint')"
        />
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.close_to_tray"
          :label="t('settings.closeToTray')"
          :description="t('settings.closeToTrayHint')"
        />
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.mux') }}</h2>
        <ToggleSwitch v-model="draft.mux_enabled" :label="t('settings.mux')" />
        <div class="divider" />
        <label class="field">
          <span class="field-label">{{ t('settings.muxConcurrency') }}</span>
          <input
            v-model.number="draft.mux_concurrency"
            class="input input-narrow"
            type="number"
            min="1"
            max="128"
            step="1"
            :disabled="!draft.mux_enabled"
          />
        </label>
        <p class="field-hint">{{ t('settings.muxHint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.kernel.title') }}</h2>
        <p class="field-hint">{{ t('settings.kernel.hint') }}</p>
        <div v-for="row in kernelRows" :key="row.kind" class="kernel-row">
          <div class="kernel-head">
            <span class="field-label">{{ row.label }}</span>
            <span class="badge" :class="row.binary?.present ? 'badge-success' : 'badge-warning'">
              {{ row.binary?.present ? t('settings.kernel.present') : t('settings.kernel.missing') }}
            </span>
            <span class="badge badge-accent">
              {{ row.override ? t('settings.kernel.custom') : t('settings.kernel.default') }}
            </span>
          </div>
          <code class="kernel-path">{{ row.binary?.path ?? '—' }}</code>
          <div class="row">
            <button
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null"
              @click="chooseKernel(row.kind)"
            >
              {{ t('settings.kernel.choose') }}
            </button>
            <button
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null || !row.override"
              @click="resetKernel(row.kind)"
            >
              {{ t('settings.kernel.reset') }}
            </button>
            <button
              class="btn btn-sm"
              type="button"
              :disabled="kernelBusy !== null"
              @click="probeKernelVersion(row.kind)"
            >
              {{ t('settings.kernel.version') }}
            </button>
          </div>
          <p v-if="kernelNotes[row.kind]" class="field-hint">{{ kernelNotes[row.kind] }}</p>
        </div>
        <p class="field-hint">{{ t('settings.kernel.note') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.logLevel') }}</h2>
        <div class="segmented">
          <button
            v-for="level in logLevels"
            :key="level.value"
            type="button"
            :class="{ active: draft.log_level === level.value }"
            :aria-pressed="draft.log_level === level.value"
            @click="draft.log_level = level.value"
          >
            {{ t(level.labelKey) }}
          </button>
        </div>
        <p class="field-hint">{{ t('settings.logLevelHint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.autoTest') }}</h2>
        <div class="segmented">
          <button
            v-for="option in testIntervals"
            :key="option.value"
            type="button"
            :class="{ active: draft.auto_test_minutes === option.value }"
            :aria-pressed="draft.auto_test_minutes === option.value"
            @click="draft.auto_test_minutes = option.value"
          >
            {{ t(option.labelKey) }}
          </button>
        </div>
        <p class="field-hint">{{ t('settings.autoTestHint') }}</p>
        <div class="field">
          <span class="field-label">{{ t('settings.testUrl') }}</span>
          <input v-model="draft.test_url" class="input input-mono" type="text" />
        </div>
        <p class="field-hint">{{ t('settings.testUrlHint') }}</p>
        <div class="grid">
          <label class="field">
            <span class="field-label">{{ t('settings.testTimeout') }}</span>
            <input
              v-model.number="draft.test_timeout_ms"
              class="input input-narrow"
              type="number"
              min="500"
              max="20000"
              step="500"
            />
          </label>
          <label class="field">
            <span class="field-label">{{ t('settings.testConcurrency') }}</span>
            <input
              v-model.number="draft.test_concurrency"
              class="input input-narrow"
              type="number"
              min="1"
              max="16"
              step="1"
            />
          </label>
        </div>
        <p class="field-hint">{{ t('settings.testBudgetHint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.subAgent') }}</h2>
        <div class="field">
          <span class="field-label">{{ t('settings.subAgentLabel') }}</span>
          <input
            v-model="draft.subscription_user_agent"
            class="input input-mono"
            type="text"
            :placeholder="t('settings.subAgentPlaceholder')"
          />
        </div>
        <p class="field-hint">{{ t('settings.subAgentHint') }}</p>
        <div class="divider" />
        <ToggleSwitch
          v-model="draft.subscription_via_proxy"
          :label="t('settings.subViaProxy')"
          :description="t('settings.subViaProxyHint')"
        />
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.dns.title') }}</h2>
        <div class="field">
          <span class="field-label">{{ t('settings.dns.servers') }}</span>
          <input
            v-model="draft.dns_servers"
            class="input input-mono"
            type="text"
            :placeholder="t('settings.dns.placeholder')"
          />
        </div>
        <p class="field-hint">{{ t('settings.dns.hint') }}</p>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.backup.title') }}</h2>
        <p class="field-hint">{{ t('settings.backup.hint') }}</p>
        <div class="row">
          <button class="btn btn-sm" type="button" :disabled="backupBusy" @click="exportConfig()">
            {{ t('settings.backup.export') }}
          </button>
          <button class="btn btn-sm" type="button" :disabled="backupBusy" @click="restoreConfig()">
            {{ t('settings.backup.restore') }}
          </button>
        </div>
        <p v-if="backupNote" class="field-hint">{{ backupNote }}</p>
      </section>

      <p class="field-hint">
        {{ t('settings.tray') }}
      </p>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.hotkeysTitle') }}</h2>
        <p class="field-hint">{{ t('settings.hotkeysHint') }}</p>
        <div class="hotkey-row">
          <kbd>Ctrl+Shift+X</kbd>
          <span>{{ t('settings.hotkeyToggleHint') }}</span>
        </div>
        <div class="hotkey-row">
          <kbd>Ctrl+Shift+W</kbd>
          <span>{{ t('settings.hotkeyWindowHint') }}</span>
        </div>
      </section>

      <section class="surface-plain panel">
        <h2 class="section-title">{{ t('settings.language') }}</h2>
        <div class="segmented">
          <button
            v-for="item in locales"
            :key="item.value"
            type="button"
            :class="{ active: locale === item.value }"
            :aria-pressed="locale === item.value"
            @click="setLocale(item.value)"
          >
            {{ item.label }}
          </button>
        </div>
        <p class="field-hint">{{ t('settings.languageHint') }}</p>
      </section>
    </template>
    </div>
  </div>
</template>

<style scoped>
/* Nine panels do not fit in a 944px window, and saving is the only way to
   commit any of them: pin the header so the button cannot scroll out of reach. */
.settings-page {
  /* Header out of the scroller, panels in their own scroller: a sticky bar has
     to be opaque and still let a band of panels show above it, and the one
     control that matters (保存) must never scroll away. */
  gap: 0;
  padding: 0;
  overflow: hidden;
}

.page-header {
  flex: none;
  padding: 22px 32px 14px;
  background: var(--surface-solid);
  border-bottom: 1px solid var(--border-subtle);
}

.page-body {
  display: flex;
  flex-direction: column;
  gap: 20px;
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: 24px 32px 40px;
}

.hotkey-row {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-secondary);
}

kbd {
  flex: none;
  padding: 3px 8px;
  border: 1px solid var(--border-strong);
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
  color: var(--text-primary);
  font-family: var(--font-mono);
  font-size: 12px;
}

.panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 18px 20px;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 16px;
}

.divider {
  height: 1px;
  background: var(--border-subtle);
}

.proxy-state {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  font-size: 13px;
}

.kernel-row {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.kernel-head {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.kernel-path {
  font-family: var(--font-mono);
  font-size: 12px;
  color: var(--text-secondary);
  /* A full path is long and has no spaces to break at on Windows. */
  overflow-wrap: anywhere;
}
</style>
