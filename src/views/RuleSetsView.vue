<script setup lang="ts">
import { computed, onMounted, reactive, ref } from 'vue'

import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import ModalDialog from '../components/ModalDialog.vue'
import StaleConfig from '../components/StaleConfig.vue'
import ToggleSwitch from '../components/ToggleSwitch.vue'
import { t } from '../i18n'
import {
  api,
  describeError,
  type GeoAssetStatus_Serialize,
  type RuleSet_Serialize,
  type RuleSetSource,
} from '../services/contract'
import { formatBytes, formatWhen } from '../services/format'
import { useEngineStore } from '../stores/engine'
import { useRuleSetsStore } from '../stores/rulesets'

const store = useRuleSetsStore()
const engine = useEngineStore()

/**
 * A set's contents, its enabled flag and its address only reach a kernel when
 * it renders its config at startup, so a running kernel keeps resolving the
 * old set until it is restarted. Nothing is stale while nothing runs.
 */
const stale = ref(false)

function markStale(): void {
  if (engine.running) stale.value = true
}

/** `0` = only when I ask, same meaning as on the subscriptions page. */
const intervals = [
  { value: 0, labelKey: 'rs.interval.manual' },
  { value: 60, labelKey: 'rs.interval.hourly' },
  { value: 360, labelKey: 'rs.interval.6h' },
  { value: 720, labelKey: 'rs.interval.12h' },
  { value: 1440, labelKey: 'rs.interval.daily' },
]

const editing = ref(false)
const pasting = ref<RuleSet_Serialize | null>(null)
const confirming = ref<RuleSet_Serialize | null>(null)
const form = reactive({
  id: '',
  name: '',
  source: 'remote' as RuleSetSource,
  url: '',
  enabled: true,
  update_interval_minutes: 1440,
  /** Carried through an edit so saving never resets the count to zero. */
  entry_count: 0 as number | null,
})
const formError = ref<string | null>(null)
const saving = ref(false)

const pasteText = ref('')
const pasteError = ref<string | null>(null)
const note = ref<string | null>(null)

/**
 * The geo data files. A `geoip:`/`geosite:` rule silently matches nothing when
 * they are absent, so the page states whether they exist instead of letting
 * the rule look broken.
 */
const geo = ref<GeoAssetStatus_Serialize[]>([])
const geoBusy = ref(false)
const geoNote = ref<string | null>(null)
/**
 * The geo data files are read once, at kernel start. Replacing them under a
 * running kernel therefore changes nothing until it restarts — the same gap as
 * saving a rule, so it gets the same notice.
 */
const geoStale = ref(false)

const isEmpty = computed(() => store.loaded && store.items.length === 0)

function intervalLabel(minutes: number | null): string {
  const match = intervals.find((item) => item.value === (minutes ?? 0))
  return match ? t(match.labelKey) : t('rs.interval.custom', { count: minutes ?? 0 })
}

function sourceLabel(source: RuleSetSource): string {
  return t(source === 'remote' ? 'rs.source.remote' : 'rs.source.local')
}

function openCreate(): void {
  form.id = ''
  form.name = ''
  form.source = 'remote'
  form.url = ''
  form.enabled = true
  form.update_interval_minutes = 1440
  form.entry_count = 0
  formError.value = null
  editing.value = true
}

function openEdit(set: RuleSet_Serialize): void {
  form.id = set.id
  form.name = set.name
  form.source = set.source
  form.url = set.url
  form.enabled = set.enabled
  form.update_interval_minutes = set.update_interval_minutes ?? 0
  form.entry_count = set.entry_count
  formError.value = null
  editing.value = true
}

/** A local set has no URL to validate; a remote one must be fetchable. */
async function submit(): Promise<void> {
  if (!form.name.trim()) {
    formError.value = t('rs.editor.err.name')
    return
  }
  const url = form.source === 'remote' ? form.url.trim() : ''
  if (form.source === 'remote' && !/^https?:\/\//i.test(url)) {
    formError.value = t('rs.editor.err.url')
    return
  }
  saving.value = true
  formError.value = null
  try {
    if (form.id) {
      await store.save({
        id: form.id,
        name: form.name.trim(),
        enabled: form.enabled,
        source: form.source,
        url,
        update_interval_minutes: form.update_interval_minutes,
        entry_count: form.entry_count,
      })
    } else {
      await store.create(form.name.trim(), url)
    }
    markStale()
    editing.value = false
  } catch (cause) {
    formError.value = describeError(cause)
  } finally {
    saving.value = false
  }
}

function openPaste(set: RuleSet_Serialize): void {
  pasting.value = set
  pasteText.value = ''
  pasteError.value = null
}

async function savePaste(): Promise<void> {
  const target = pasting.value
  if (!target) return
  if (!pasteText.value.trim()) {
    pasteError.value = t('rs.paste.empty')
    return
  }
  try {
    const updated = await store.setText(target.id, pasteText.value)
    markStale()
    note.value = t('rs.note.saved', { name: updated.name, count: updated.entry_count ?? 0 })
    pasting.value = null
  } catch (cause) {
    pasteError.value = describeError(cause)
  }
}

async function refreshOne(set: RuleSet_Serialize): Promise<void> {
  note.value = null
  try {
    const updated = await store.refresh(set.id)
    markStale()
    note.value = t('rs.note.refreshed', { name: updated.name, count: updated.entry_count ?? 0 })
  } catch {
    /* store.error already carries the reason */
  }
}

async function confirmDelete(): Promise<void> {
  const target = confirming.value
  if (!target) return
  try {
    await store.remove(target.id)
    markStale()
    note.value = t('rs.note.deleted', { name: target.name })
  } catch (cause) {
    store.error = describeError(cause)
  } finally {
    confirming.value = null
  }
}

/** The enable switch is a save too: a disabled set stops being a rule target. */
async function toggleOne(set: RuleSet_Serialize): Promise<void> {
  try {
    await store.toggle(set.id)
    markStale()
  } catch (cause) {
    store.error = describeError(cause)
  }
}

async function loadGeo(): Promise<void> {
  try {
    geo.value = await api.geoAssets()
  } catch {
    // A missing directory is the normal "not downloaded yet" state; the rows
    // simply stay empty rather than putting an error on the page.
    geo.value = []
  }
}

async function updateGeo(): Promise<void> {
  geoBusy.value = true
  geoNote.value = null
  try {
    const result = await api.updateGeoAssets()
    geoNote.value = t('rs.geo.done') + (result.lines.length ? ` · ${result.lines.join(' · ')}` : '')
    if (result.restart_needed) geoStale.value = true
  } catch (cause) {
    geoNote.value = t('rs.geo.failed', { reason: describeError(cause) })
  } finally {
    geoBusy.value = false
    await loadGeo()
  }
}

onMounted(() => {
  void store.load()
  void loadGeo()
})
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.ruleSets.title') }}</h1>
        <p class="page-subtitle">{{ t('rs.hint') }}</p>
      </div>
      <div class="row">
        <button type="button" class="btn btn-sm" :disabled="store.loading" @click="store.load(true)">
          {{ t('common.refresh') }}
        </button>
        <button type="button" class="btn btn-primary btn-sm" @click="openCreate">{{ t('rs.new') }}</button>
      </div>
    </header>

    <p v-if="store.error" class="alert">{{ store.error }}</p>
    <p v-if="note" class="notice">{{ note }}</p>

    <StaleConfig :show="stale" @applied="stale = false" />

    <section class="surface-plain geo-card">
      <div class="geo-head">
        <div>
          <h2 class="section-title">{{ t('rs.geo.title') }}</h2>
          <p class="field-hint">{{ t('rs.geo.hint') }}</p>
        </div>
        <button type="button" class="btn btn-sm" :disabled="geoBusy" @click="updateGeo">
          {{ geoBusy ? t('rs.geo.busy') : t('rs.geo.update') }}
        </button>
      </div>
      <p v-if="geoNote" class="notice">{{ geoNote }}</p>
      <StaleConfig
        :show="geoStale"
        :message="t('rs.geo.stale')"
        :apply-label="t('rs.geo.staleApply')"
        @applied="geoStale = false"
        @failed="geoStale = false"
      />
      <ul class="geo-list">
        <li v-for="asset in geo" :key="asset.name">
          <span class="geo-name">{{ asset.name }}</span>
          <span v-if="asset.exists" class="muted small">
            {{ formatBytes(asset.bytes) }} · {{ formatWhen((asset.modified ?? 0) * 1000) }}
          </span>
          <span v-else class="badge">{{ t('rs.geo.missing') }}</span>
        </li>
      </ul>
    </section>

    <p v-if="store.loading && !store.loaded" class="muted small">
      {{ t('common.loading') }}
    </p>

    <EmptyState
      v-else-if="isEmpty"
      icon="inbox"
      :title="t('rs.empty.title')"
      :body="t('rs.empty.body')"
    >
      <template #actions>
        <button type="button" class="btn btn-primary btn-sm" @click="openCreate">{{ t('rs.new') }}</button>
      </template>
    </EmptyState>

    <div v-else class="surface-plain table-wrap">
      <table class="data-table">
        <thead>
          <tr>
            <th scope="col">{{ t('rs.col.name') }}</th>
            <th scope="col">{{ t('rs.col.source') }}</th>
            <th scope="col" class="num">{{ t('rs.col.entries') }}</th>
            <th scope="col">{{ t('rs.col.interval') }}</th>
            <th scope="col">{{ t('rs.col.updated') }}</th>
            <th scope="col">{{ t('rs.col.status') }}</th>
            <th scope="col" class="actions">{{ t('rs.col.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="set in store.items" :key="set.id" :class="{ dim: !set.enabled }">
            <td>{{ set.name }}</td>
            <td>{{ sourceLabel(set.source) }}</td>
            <td class="num">{{ set.entry_count ?? 0 }}</td>
            <td>{{ intervalLabel(set.update_interval_minutes) }}</td>
            <td>{{ formatWhen(set.updated_at_ms) }}</td>
            <td>
              <span v-if="set.last_error" class="badge badge-danger" :title="set.last_error">
                {{ t('rs.status.failed') }}
              </span>
              <span v-else-if="!set.enabled" class="badge">{{ t('rs.status.disabled') }}</span>
              <span v-else class="badge badge-success">{{ t('rs.status.ok') }}</span>
            </td>
            <td class="actions">
              <button type="button" class="btn btn-ghost btn-sm" @click="openPaste(set)">
                {{ t('rs.paste') }}
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :title="t('rs.refresh')"
                :aria-label="t('rs.refresh')"
                :disabled="store.busyId === set.id"
                @click="refreshOne(set)"
              >
                <AppIcon name="refresh" :size="14" />
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :title="t('common.edit')"
                :aria-label="t('common.edit')"
                @click="openEdit(set)"
              >
                <AppIcon name="edit" :size="14" />
              </button>
              <button type="button" class="btn btn-ghost btn-sm" @click="toggleOne(set)">
                {{ set.enabled ? t('common.disable') : t('common.enable') }}
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :title="t('common.delete')"
                :aria-label="t('common.delete')"
                @click="confirming = set"
              >
                <AppIcon name="trash" :size="14" />
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <ModalDialog
      :open="editing"
      :title="form.id ? t('rs.editor.edit') : t('rs.editor.new')"
      :width="560"
      :close-on-overlay="false"
      @close="editing = false"
    >
      <div class="form-body">
        <label class="field">
          <span class="field-label">{{ t('rs.editor.name') }}</span>
          <input v-model="form.name" class="input" :placeholder="t('rs.editor.namePlaceholder')" />
        </label>

        <div class="field">
          <span class="field-label">{{ t('rs.editor.source') }}</span>
          <div class="segmented">
            <button
              type="button"
              :class="{ active: form.source === 'remote' }"
              :aria-pressed="form.source === 'remote'"
              @click="form.source = 'remote'"
            >
              {{ t('rs.source.remote') }}
            </button>
            <button
              type="button"
              :class="{ active: form.source === 'local' }"
              :aria-pressed="form.source === 'local'"
              @click="form.source = 'local'"
            >
              {{ t('rs.source.local') }}
            </button>
          </div>
        </div>

        <label v-if="form.source === 'remote'" class="field">
          <span class="field-label">{{ t('rs.editor.url') }}</span>
          <input v-model="form.url" class="input input-mono" placeholder="https://example.com/ads.txt" />
          <span class="field-hint">{{ t('rs.editor.urlHint') }}</span>
        </label>

        <label v-if="form.source === 'remote'" class="field">
          <span class="field-label">{{ t('rs.editor.interval') }}</span>
          <select v-model.number="form.update_interval_minutes" class="select">
            <option v-for="item in intervals" :key="item.value" :value="item.value">
              {{ t(item.labelKey) }}
            </option>
          </select>
        </label>

        <ToggleSwitch v-model="form.enabled" :label="t('common.enabled')" />

        <p v-if="formError" class="field-error">{{ formError }}</p>
      </div>

      <template #footer>
        <button type="button" class="btn btn-ghost btn-sm" @click="editing = false">
          {{ t('common.cancel') }}
        </button>
        <button type="button" class="btn btn-primary btn-sm" :disabled="saving" @click="submit">
          {{ saving ? t('common.saving') : t('common.save') }}
        </button>
      </template>
    </ModalDialog>

    <ModalDialog
      :open="pasting !== null"
      :title="t('rs.paste.title')"
      :width="640"
      :close-on-overlay="false"
      @close="pasting = null"
    >
      <div class="form-body">
        <p class="field-hint">{{ t('rs.paste.hint') }}</p>
        <textarea
          v-model="pasteText"
          class="input input-mono paste-area"
          rows="12"
          :placeholder="t('rs.paste.placeholder')"
        />
        <p v-if="pasteError" class="field-error">{{ pasteError }}</p>
      </div>

      <template #footer>
        <button type="button" class="btn btn-ghost btn-sm" @click="pasting = null">
          {{ t('common.cancel') }}
        </button>
        <button type="button" class="btn btn-primary btn-sm" @click="savePaste">
          {{ t('rs.paste.save') }}
        </button>
      </template>
    </ModalDialog>

    <ModalDialog
      :open="confirming !== null"
      :title="t('rs.delete.title')"
      :width="440"
      @close="confirming = null"
    >
      <p>{{ t('rs.delete.body', { name: confirming?.name ?? '' }) }}</p>
      <template #footer>
        <button type="button" class="btn btn-ghost btn-sm" @click="confirming = null">
          {{ t('common.cancel') }}
        </button>
        <button type="button" class="btn btn-danger btn-sm" @click="confirmDelete">
          {{ t('common.delete') }}
        </button>
      </template>
    </ModalDialog>
  </div>
</template>

<style scoped>
.geo-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 18px;
}

.geo-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

.geo-head .field-hint {
  margin-top: 4px;
  max-width: 72ch;
}

.geo-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 22px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.geo-list li {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}

.geo-name {
  font-family: var(--font-mono);
  color: var(--text-primary);
}

.form-body {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.paste-area {
  min-height: 220px;
  resize: vertical;
  font-size: 12px;
  line-height: 1.6;
}

.dim {
  opacity: 0.55;
}

.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.table-wrap {
  /* A narrow window must scroll a wide table instead of clipping its action
     column out of reach. */
  overflow-x: auto;
}

.actions {
  text-align: right;
  white-space: nowrap;
}
</style>
