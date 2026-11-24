<script setup lang="ts">
/**
 * About: version, where things live on disk, and the licence.
 *
 * The four paths are the answer to "where did my config go?" — every support
 * question about this kind of app starts there, so they are copyable text
 * rather than a sentence in a README nobody opens.
 */
import { computed, onMounted, ref } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import { t } from '../i18n'
import { api, describeError, type AppInfo, type AppPaths, type UpdateStatus } from '../services/contract'

const info = ref<AppInfo | null>(null)
const paths = ref<AppPaths | null>(null)
const error = ref<string | null>(null)

/** Copying the path is what people actually want; opening it is the shortcut. */
const copied = ref<string | null>(null)

/** `std::env::consts::OS` is lowercase (`windows`); a product screen is not. */
const platformLabel = computed(() => {
  const value = info.value?.platform
  return value ? value.charAt(0).toUpperCase() + value.slice(1) : '—'
})

onMounted(async () => {
  try {
    const [app, where] = await Promise.all([api.appInfo(), api.appPaths()])
    info.value = app
    paths.value = where
  } catch (problem) {
    error.value = describeError(problem)
  }
})

/**
 * What this client does and does not do, next to the two clients people
 * compare it with.
 *
 * The "not supported" rows are deliberate: a switch that silently does nothing
 * is worse than a line that says no and explains why. TUN needs a virtual
 * adapter driver and would collide with an already-running proxy; PAC exists,
 * but only for the part a script can express (domain names).
 */
interface Capability {
  key: string
  supported: boolean
  note: string
}

const capabilities: Capability[] = [
  { key: 'proxy', supported: true, note: 'about.caps.proxy.note' },
  { key: 'rules', supported: true, note: 'about.caps.rules.note' },
  { key: 'groups', supported: true, note: 'about.caps.groups.note' },
  { key: 'subscription', supported: true, note: 'about.caps.subscription.note' },
  { key: 'connections', supported: true, note: 'about.caps.connections.note' },
  { key: 'backup', supported: true, note: 'about.caps.backup.note' },
  { key: 'tun', supported: false, note: 'about.caps.tun.note' },
  { key: 'pac', supported: true, note: 'about.caps.pac.note' },
  { key: 'update', supported: false, note: 'about.caps.update.note' },
]

async function openDataDir(): Promise<void> {
  try {
    await api.appOpenDataDir()
  } catch (problem) {
    error.value = describeError(problem)
  }
}

/** Where the last report landed, so the hint can say so instead of "done". */
const reportPath = ref<string | null>(null)
const exporting = ref(false)

async function exportDiagnostics(): Promise<void> {
  exporting.value = true
  try {
    const written = await api.exportDiagnostics()
    reportPath.value = written ? written : null
  } catch (problem) {
    error.value = describeError(problem)
  } finally {
    exporting.value = false
  }
}

/**
 * The update answer, once we have one.
 *
 * `failed` is separate from `error` on purpose: a feed we cannot reach is not
 * a broken app, and it should not paint the page red. It gets its own line
 * under the button, phrased as "could not ask".
 */
const update = ref<UpdateStatus | null>(null)
const checking = ref(false)
const updateFailed = ref<string | null>(null)

async function checkUpdate(): Promise<void> {
  checking.value = true
  updateFailed.value = null
  try {
    update.value = await api.checkUpdate()
  } catch (problem) {
    update.value = null
    updateFailed.value = describeError(problem)
  } finally {
    checking.value = false
  }
}

/** Opening the release page is the user's click, not ours. */
async function openRelease(): Promise<void> {
  const url = update.value?.url
  if (!url) return
  try {
    await api.appOpenUrl(url)
  } catch (problem) {
    updateFailed.value = describeError(problem)
  }
}

async function copy(value: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(value)
    copied.value = value
    window.setTimeout(() => {
      if (copied.value === value) copied.value = null
    }, 1600)
  } catch {
    // Clipboard permission is not worth an error banner: the text is on screen
    // and selectable, which is the fallback that always works.
  }
}
</script>

<template>
  <section class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.about.title') }}</h1>
        <p class="page-subtitle">{{ t('page.about.subtitle') }}</p>
      </div>
    </header>

    <p v-if="error" class="alert">{{ error }}</p>

    <div class="about-grid">
      <article class="card glass">
        <h2 class="section-title">{{ t('about.version.title') }}</h2>
        <dl class="facts">
          <div class="fact">
            <dt>{{ t('about.version.name') }}</dt>
            <dd>{{ info?.name ?? '—' }}</dd>
          </div>
          <div class="fact">
            <dt>{{ t('about.version.version') }}</dt>
            <dd class="mono">{{ info?.version ?? '—' }}</dd>
          </div>
          <div class="fact">
            <dt>{{ t('about.version.platform') }}</dt>
            <dd class="mono">{{ platformLabel }}</dd>
          </div>
          <div class="fact">
            <dt>{{ t('about.version.arch') }}</dt>
            <dd class="mono">{{ info?.arch ?? '—' }}</dd>
          </div>
        </dl>
      </article>

      <article class="card glass">
        <h2 class="section-title">{{ t('about.license.title') }}</h2>
        <p class="about-body">{{ t('about.license.body') }}</p>
        <p class="about-body muted">{{ t('about.license.upstream') }}</p>
      </article>
    </div>

    <article class="card glass">
      <div class="row">
        <h2 class="section-title">{{ t('about.paths.title') }}</h2>
        <span class="spacer" />
        <button type="button" class="btn btn-ghost btn-sm" @click="openDataDir">
          <AppIcon name="inbox" :size="15" />
          {{ t('about.paths.open') }}
        </button>
      </div>
      <p class="about-body muted">{{ t('about.paths.hint') }}</p>
      <ul class="path-list">
        <li v-for="entry in [
          { key: 'data_dir', label: t('about.paths.data'), value: paths?.data_dir },
          { key: 'engines_dir', label: t('about.paths.engines'), value: paths?.engines_dir },
          { key: 'config_dir', label: t('about.paths.config'), value: paths?.config_dir },
          { key: 'log_dir', label: t('about.paths.logs'), value: paths?.log_dir },
        ]" :key="entry.key" class="path-row">
          <span class="path-label">{{ entry.label }}</span>
          <code class="path-value mono">{{ entry.value ?? '—' }}</code>
          <button
            v-if="entry.value"
            type="button"
            class="btn btn-sm"
            @click="copy(entry.value)"
          >
            {{ copied === entry.value ? t('common.copied') : t('common.copy') }}
          </button>
        </li>
      </ul>
    </article>

    <article class="card glass">
      <h2 class="section-title">{{ t('about.update.title') }}</h2>
      <p class="about-body">{{ t('about.update.hint') }}</p>
      <div class="diagnostics-row">
        <button type="button" class="btn btn-sm" :disabled="checking" @click="checkUpdate">
          <AppIcon name="refresh" :size="15" />
          {{ checking ? t('about.update.checking') : t('about.update.check') }}
        </button>
        <template v-if="update">
          <span v-if="update.has_update" class="update-chip">
            {{ t('about.update.available', { version: update.latest }) }}
          </span>
          <span v-else class="muted small">
            {{ t('about.update.current', { version: update.current }) }}
          </span>
          <button
            v-if="update.has_update && update.url"
            type="button"
            class="btn btn-primary btn-sm"
            @click="openRelease"
          >
            <AppIcon name="arrow-up" :size="15" />
            {{ t('about.update.open') }}
          </button>
        </template>
      </div>
      <p v-if="update?.has_update && update.notes" class="update-notes mono">{{ update.notes }}</p>
      <p v-if="updateFailed" class="update-failed">
        {{ t('about.update.failed', { reason: updateFailed }) }}
      </p>
    </article>

    <article class="card glass">
      <h2 class="section-title">{{ t('about.diagnostics.title') }}</h2>
      <p class="about-body">{{ t('about.diagnostics.hint') }}</p>
      <div class="diagnostics-row">
        <button type="button" class="btn btn-primary btn-sm" :disabled="exporting" @click="exportDiagnostics">
          <AppIcon name="file" :size="15" />
          {{ t('about.diagnostics.export') }}
        </button>
        <code v-if="reportPath" class="path-value mono">
          {{ t('about.diagnostics.saved', { path: reportPath }) }}
        </code>
      </div>
    </article>

    <article class="card glass">
      <h2 class="section-title">{{ t('about.engines.title') }}</h2>
      <p class="about-body">{{ t('about.engines.body') }}</p>
      <code class="engines-path mono">{{ paths?.engines_dir ?? t('about.engines.unknown') }}</code>
    </article>

    <article class="card glass">
      <h2 class="section-title">{{ t('about.caps.title') }}</h2>
      <p class="about-body muted">{{ t('about.caps.hint') }}</p>
      <ul class="cap-list">
        <li v-for="item in capabilities" :key="item.key" class="cap-row">
          <span class="cap-name">{{ t(`about.caps.${item.key}`) }}</span>
          <span class="cap-badge" :class="item.supported ? 'cap-yes' : 'cap-no'">
            {{ item.supported ? t('about.caps.supported') : t('about.caps.unsupported') }}
          </span>
          <span class="cap-note">{{ t(item.note) }}</span>
        </li>
      </ul>
    </article>
  </section>
</template>

<style scoped>
.about-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(280px, 1fr));
  gap: 12px;
}

.card {
  padding: 16px 18px;
  border-radius: var(--radius-card);
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.engines-path {
  display: block;
  padding: 8px 10px;
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
  font-size: 12.5px;
  color: var(--text-secondary);
  word-break: break-all;
}

.facts {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 0;
}

.fact {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  font-size: 13px;
}

.fact dt {
  color: var(--text-secondary);
}

.fact dd {
  margin: 0;
  color: var(--text-primary);
}

.about-body {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-secondary);
}

.path-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.path-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
}

.path-label {
  flex: none;
  width: 96px;
  font-size: 12px;
  color: var(--text-secondary);
}

.diagnostics-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

/* A new release is news, not a warning: accent, not danger. */
.update-chip {
  padding: 3px 9px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 600;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 14%, transparent);
}

.update-notes {
  margin: 10px 0 0;
  padding: 10px 12px;
  max-height: 132px;
  overflow-y: auto;
  border: 1px solid var(--border-subtle);
  border-radius: 10px;
  background: var(--surface-sunken);
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

/* "Could not ask" is not "you are broken": muted, not red. */
.update-failed {
  margin: 10px 0 0;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
  overflow-wrap: anywhere;
}

.path-value {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-primary);
  /* A path broken mid-segment ("com.xma / trix.client") is unreadable; let the
     row scroll sideways instead. */
  white-space: nowrap;
  overflow-x: auto;
  scrollbar-width: thin;
}

.cap-list {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.cap-row {
  display: grid;
  grid-template-columns: 140px 68px 1fr;
  align-items: baseline;
  gap: 10px;
  font-size: 13px;
}

.cap-name {
  color: var(--text-primary);
}

.cap-badge {
  font-size: 11.5px;
  padding: 1px 8px;
  border-radius: 999px;
  text-align: center;
  border: 1px solid transparent;
}

.cap-yes {
  color: var(--success);
  background: color-mix(in srgb, var(--success) 14%, transparent);
}

.cap-no {
  color: var(--text-tertiary);
  background: var(--surface-sunken);
  border-color: var(--border-subtle);
}

.cap-note {
  color: var(--text-secondary);
  line-height: 1.5;
}

@media (max-width: 720px) {
  .cap-row {
    grid-template-columns: 1fr auto;
  }

  .cap-note {
    grid-column: 1 / -1;
  }
}
</style>
