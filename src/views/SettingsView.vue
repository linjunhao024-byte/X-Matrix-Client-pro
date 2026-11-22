<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import ToggleSwitch from '../components/ToggleSwitch.vue'
import { availableLocales, locale, setLocale, t } from '../i18n'
import { api, describeError, type ProxyMode, type Settings_Serialize } from '../services/contract'
import { useSettingsStore } from '../stores/settings'
import { useSystemProxyStore } from '../stores/sysproxy'

const store = useSettingsStore()
const proxy = useSystemProxyStore()
const draft = ref<Settings_Serialize | null>(null)
const error = ref<string | null>(null)
const saved = ref(false)
const autostart = ref<boolean | null>(null)
/** Result line for backup/restore: what happened, in one sentence. */
const backupNote = ref<string | null>(null)
const backupBusy = ref(false)

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

onMounted(async () => {
  await store.load()
  if (store.settings) draft.value = { ...store.settings }
  await proxy.refresh()
  await refreshAutostart()
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
    window.setTimeout(() => {
      saved.value = false
    }, 2000)
  } catch (cause) {
    error.value = describeError(cause)
  }
}
</script>

<template>
  <div class="page">
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

    <p v-if="store.error" class="alert">{{ store.error }}</p>
    <p v-if="error" class="alert">{{ error }}</p>

    <section v-if="!draft" class="surface-plain">
      <div class="empty-state">
        <div class="empty-state-title">{{ t('common.loading') }}</div>
      </div>
    </section>

    <template v-else>
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
</template>

<style scoped>
/* Nine panels do not fit in a 944px window, and saving is the only way to
   commit any of them: pin the header so the button cannot scroll out of reach. */
.page-header {
  position: sticky;
  top: 0;
  z-index: 6;
  margin: -28px -32px 0;
  padding: 22px 32px 14px;
  background: var(--surface-glass-strong);
  backdrop-filter: blur(var(--blur-glass));
  border-bottom: 1px solid var(--border-subtle);
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
</style>
