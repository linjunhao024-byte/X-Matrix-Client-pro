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
import { api, describeError, type AppInfo, type AppPaths } from '../services/contract'

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

async function openDataDir(): Promise<void> {
  try {
    await api.appOpenDataDir()
  } catch (problem) {
    error.value = describeError(problem)
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
      <h2 class="section-title">{{ t('about.engines.title') }}</h2>
      <p class="about-body">{{ t('about.engines.body') }}</p>
      <code class="engines-path mono">{{ paths?.engines_dir ?? t('about.engines.unknown') }}</code>
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
</style>
