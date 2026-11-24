<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'

import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import ModalDialog from '../components/ModalDialog.vue'
import ToggleSwitch from '../components/ToggleSwitch.vue'
import { t } from '../i18n'
import { describeError, type Subscription_Serialize } from '../services/contract'
import { formatBytesCompact, formatDay, formatNextRefresh, formatWhen } from '../services/format'
import { useEngineStore } from '../stores/engine'
import { useSettingsStore } from '../stores/settings'
import { useSubscriptionsStore } from '../stores/subscriptions'

const store = useSubscriptionsStore()
const engine = useEngineStore()
const settings = useSettingsStore()

/** `0` = only when I ask, which is what "manual" means everywhere else too. */
const intervals = [
  { value: 0, labelKey: 'sub.interval.manual' },
  { value: 60, labelKey: 'sub.interval.hourly' },
  { value: 360, labelKey: 'sub.interval.6h' },
  { value: 720, labelKey: 'sub.interval.12h' },
  { value: 1440, labelKey: 'sub.interval.daily' },
]

const editing = ref(false)
const confirming = ref<Subscription_Serialize | null>(null)
const form = reactive({
  id: '',
  name: '',
  url: '',
  /**
   * The source's own rename/filter policy. They are applied by the backend
   * while the document is parsed, so the editor only has to round-trip them;
   * an empty string is "off" for all three.
   */
  rename_prefix: '',
  include: '',
  exclude: '',
  enabled: true,
  update_interval_minutes: 360,
  /** Carried through an edit so saving never resets the count to zero. */
  node_count: 0 as number | null,
  /**
   * The quota numbers come from the provider's `subscription-userinfo` header.
   * The edit form cannot produce them, so it carries them through untouched —
   * blanking them here would erase what the last refresh learned.
   */
  traffic_used: null as number | null,
  traffic_total: null as number | null,
  expires_at_ms: null as number | null,
})
const formError = ref<string | null>(null)
const saving = ref(false)
const note = ref<string | null>(null)
/**
 * Set when the refresh took away the node the kernel was running: the backend
 * stops it and hands the proxy back, and the page has to say so rather than
 * let a kernel that is no longer up look like it still is.
 */
const stoppedNodeGone = ref(false)
/**
 * Which route the last refresh took, when it was not the one the setting asked
 * for. Kept apart from `note`: that one counts nodes, this one is a caveat about
 * the fetch itself, and folding them together makes both unreadable.
 */
const routeNote = ref<string | null>(null)

/**
 * One clock for the whole page, refreshed every 30 s, so every row's countdown
 * moves together. Rows must not own intervals: a table of ten subscriptions
 * would otherwise wake up ten times per tick and drift apart.
 */
const now = ref(Date.now())
const CLOCK_MS = 30_000
let clock: ReturnType<typeof setInterval> | null = null

const isEmpty = computed(() => store.loaded && store.items.length === 0)

function intervalLabel(minutes: number | null): string {
  const match = intervals.find((item) => item.value === (minutes ?? 0))
  return match ? t(match.labelKey) : t('sub.interval.custom', { count: minutes ?? 0 })
}

/**
 * Usage and expiry are two lines rather than one: joined they were 230px wide,
 * which alone was enough to push the table into a horizontal scroll.
 * Usage without a cap is shown alone — "1.2 GB / 0 B" reads as broken.
 */
function usageLabel(subscription: Subscription_Serialize): string {
  if (subscription.traffic_used == null) return ''
  return subscription.traffic_total != null
    ? `${formatBytesCompact(subscription.traffic_used)} / ${formatBytesCompact(subscription.traffic_total)}`
    : formatBytesCompact(subscription.traffic_used)
}

function expiryLabel(subscription: Subscription_Serialize): string {
  if (subscription.expires_at_ms == null) return ''
  return t('sub.quota.expires', { day: formatDay(subscription.expires_at_ms) })
}

/** `还有 5 小时` / `该刷新了` / `只手动刷新`, or `''` when the row has none to show. */
function nextRefreshLabel(subscription: Subscription_Serialize): string {
  return formatNextRefresh(subscription, now.value, t)
}

/**
 * `前缀 机场A · · 保留 香港|台湾 · 排除 过期` — the source's own policy in one
 * quiet line, so a list that looks shorter than the provider says explains
 * itself without opening the editor. `''` hides the line entirely.
 */
function tuningLabel(subscription: Subscription_Serialize): string {
  // Trimmed only for the "is this set?" question: the prefix is used verbatim
  // (its trailing space is the separator), and whitespace alone is "off" on
  // the backend too, so it must not show up as a policy here.
  const prefix = (subscription.rename_prefix ?? '').trim()
  const include = (subscription.include ?? '').trim()
  const exclude = (subscription.exclude ?? '').trim()
  const parts: string[] = []
  if (prefix) {
    parts.push(t('sub.tuning.prefix', { prefix: subscription.rename_prefix }))
  }
  if (include) {
    parts.push(t('sub.tuning.include', { pattern: subscription.include }))
  }
  if (exclude) {
    parts.push(t('sub.tuning.exclude', { pattern: subscription.exclude }))
  }
  return parts.join(' · ')
}

function openCreate(): void {
  form.id = ''
  form.name = ''
  form.url = ''
  form.rename_prefix = ''
  form.include = ''
  form.exclude = ''
  form.enabled = true
  form.update_interval_minutes = 360
  form.node_count = 0
  form.traffic_used = null
  form.traffic_total = null
  form.expires_at_ms = null
  formError.value = null
  editing.value = true
}

function openEdit(subscription: Subscription_Serialize): void {
  form.id = subscription.id
  form.name = subscription.name
  form.url = subscription.url
  form.rename_prefix = subscription.rename_prefix ?? ''
  form.include = subscription.include ?? ''
  form.exclude = subscription.exclude ?? ''
  form.enabled = subscription.enabled
  form.update_interval_minutes = subscription.update_interval_minutes ?? 0
  form.node_count = subscription.node_count
  form.traffic_used = subscription.traffic_used ?? null
  form.traffic_total = subscription.traffic_total ?? null
  form.expires_at_ms = subscription.expires_at_ms ?? null
  formError.value = null
  editing.value = true
}

async function submit(): Promise<void> {
  if (!form.name.trim()) {
    formError.value = t('sub.err.name')
    return
  }
  if (!/^https?:\/\//i.test(form.url.trim())) {
    formError.value = t('sub.err.url')
    return
  }
  saving.value = true
  formError.value = null
  try {
    if (form.id) {
      await store.save({
        id: form.id,
        name: form.name.trim(),
        url: form.url.trim(),
        rename_prefix: form.rename_prefix,
        include: form.include,
        exclude: form.exclude,
        enabled: form.enabled,
        update_interval_minutes: form.update_interval_minutes,
        node_count: form.node_count,
        traffic_used: form.traffic_used,
        traffic_total: form.traffic_total,
        expires_at_ms: form.expires_at_ms,
      })
    } else {
      const created = await store.create(form.name.trim(), form.url.trim())
      // `create_subscription` still knows only name and url; the policy travels
      // through the whole-object upsert. Taking the id first means a rejected
      // pattern leaves the dialog on the edit path, so fixing it and pressing
      // save again never creates a second source.
      form.id = created.id
      if (form.rename_prefix || form.include || form.exclude) {
        await store.save({
          ...created,
          rename_prefix: form.rename_prefix,
          include: form.include,
          exclude: form.exclude,
        })
      }
    }
    editing.value = false
  } catch (cause) {
    formError.value = describeError(cause)
  } finally {
    saving.value = false
  }
}

/**
 * Did the refresh just take the kernel's node away?
 *
 * A refresh mints new ids for the nodes it imports, so the id the kernel was
 * started with never survives one; the backend stops a kernel that was on that
 * node and forgets the id. Asking the backend afterwards is the honest check:
 * this line only appears when a kernel really went down, never for a refresh
 * that left the running node in place.
 */
async function kernelStopped(wasRunning: boolean, wasOn: string | null): Promise<boolean> {
  if (!wasRunning || !wasOn) return false
  await engine.refreshStatus()
  if (engine.running) return false
  // The backend also cleared `last_node_id`; the shell reads it from here.
  await settings.load(true)
  return true
}

async function refreshOne(subscription: Subscription_Serialize): Promise<void> {
  note.value = null
  routeNote.value = null
  stoppedNodeGone.value = false
  const wasRunning = engine.running
  const wasOn = engine.status?.node_id ?? null
  try {
    const outcome = await store.refresh(subscription.id)
    const skipped = outcome.skipped?.length ?? 0
    // A node the source's own filter dropped is not a duplicate and not a
    // failure: it needs its own number, or the count looks like data loss.
    const filtered = outcome.filtered ?? 0
    routeNote.value = outcome.note ?? null
    note.value =
      t('sub.note.refreshed', {
        name: outcome.subscription.name,
        imported: outcome.imported ?? 0,
        removed: outcome.removed ?? 0,
      }) +
      (skipped > 0 ? t('sub.note.skipped', { count: skipped }) : '') +
      (filtered > 0 ? t('sub.note.filtered', { count: filtered }) : '')
    stoppedNodeGone.value = await kernelStopped(wasRunning, wasOn)
  } catch {
    /* store.error already carries the reason */
  }
}

async function refreshAll(): Promise<void> {
  note.value = null
  routeNote.value = null
  stoppedNodeGone.value = false
  const wasRunning = engine.running
  const wasOn = engine.status?.node_id ?? null
  try {
    const { imported, skipped, filtered, note: caveat } = await store.refreshAll()
    const head = imported > 0 ? t('sub.note.due', { count: imported }) : t('sub.note.noneDue')
    note.value =
      head +
      (skipped > 0 ? t('sub.note.skipped', { count: skipped }) : '') +
      (filtered > 0 ? t('sub.note.filtered', { count: filtered }) : '')
    routeNote.value = caveat
    stoppedNodeGone.value = await kernelStopped(wasRunning, wasOn)
  } catch {
    /* store.error already carries the reason */
  }
}

async function confirmDelete(): Promise<void> {
  const target = confirming.value
  if (!target) return
  try {
    await store.remove(target.id)
    note.value = t('sub.note.deleted', { name: target.name })
  } catch (cause) {
    store.error = describeError(cause)
  } finally {
    confirming.value = null
  }
}

onMounted(() => {
  void store.load()
  clock = setInterval(() => {
    now.value = Date.now()
  }, CLOCK_MS)
})

onBeforeUnmount(() => {
  if (clock !== null) clearInterval(clock)
})
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.subscriptions.title') }}</h1>
        <p class="page-subtitle">
          {{ t('sub.hint') }}
        </p>
      </div>
      <div class="row">
        <button type="button" class="btn btn-sm" :disabled="store.loading" @click="refreshAll">
          {{ t('sub.refreshDue') }}
        </button>
        <button type="button" class="btn btn-primary btn-sm" @click="openCreate">{{ t('sub.new') }}</button>
      </div>
    </header>

    <p v-if="store.error" class="alert">{{ store.error }}</p>
    <p v-if="note" class="notice">{{ note }}</p>
    <p v-if="stoppedNodeGone" class="notice">{{ t('sub.stoppedNodeGone') }}</p>
    <p v-if="routeNote" class="alert alert-inline">{{ routeNote }}</p>

    <p v-if="store.loading && !store.loaded" class="muted small">
      {{ t('common.loading') }}
    </p>

    <EmptyState
      v-else-if="isEmpty"
      icon="cloud"
      :title="t('sub.empty.title')"
      :body="t('sub.empty.body')"
    >
      <template #actions>
        <button type="button" class="btn btn-primary btn-sm" @click="openCreate">{{ t('sub.new') }}</button>
      </template>
    </EmptyState>

    <div v-else class="surface-plain table-wrap">
      <table class="data-table">
        <thead>
          <tr>
            <th scope="col">{{ t('sub.col.name') }}</th>
            <th scope="col" class="num">{{ t('sub.col.nodes') }}</th>
            <th scope="col">{{ t('sub.col.interval') }}</th>
            <th scope="col">{{ t('sub.col.updated') }}</th>
            <th scope="col">{{ t('sub.col.status') }}</th>
            <th scope="col" class="actions">{{ t('sub.col.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr
            v-for="subscription in store.items"
            :key="subscription.id"
            :class="{ dim: !subscription.enabled }"
          >
            <td>
              <div class="sub-name" :title="subscription.url">{{ subscription.name }}</div>
              <div v-if="usageLabel(subscription)" class="sub-quota">{{ usageLabel(subscription) }}</div>
              <div v-if="expiryLabel(subscription)" class="sub-quota">{{ expiryLabel(subscription) }}</div>
              <div v-if="!usageLabel(subscription) && !expiryLabel(subscription)" class="sub-quota">—</div>
              <!-- The policy line below the numbers: a provider that lists 200
                   nodes while the row says 20 should say why without the editor. -->
              <div v-if="tuningLabel(subscription)" class="sub-tuning">{{ tuningLabel(subscription) }}</div>
            </td>
            <td class="num nowrap">{{ subscription.node_count ?? 0 }}</td>
            <td class="nowrap">{{ intervalLabel(subscription.update_interval_minutes) }}</td>
            <td>
              <div class="nowrap">{{ formatWhen(subscription.updated_at_ms) }}</div>
              <!-- Under the last-update time, not in a column of its own: the two
                   lines answer one question ("is this source fresh?") and the
                   table already scrolls sideways at 1050px. -->
              <span v-if="nextRefreshLabel(subscription)" class="muted small nowrap">
                {{ nextRefreshLabel(subscription) }}
              </span>
            </td>
            <td>
              <span v-if="subscription.last_error" class="badge badge-danger" :title="subscription.last_error">
                {{ t('sub.status.failed') }}
              </span>
              <span v-else-if="!subscription.enabled" class="badge">{{ t('sub.status.disabled') }}</span>
              <span v-else class="badge badge-success">{{ t('sub.status.ok') }}</span>
              <!-- A badge saying "failed" with the reason hidden in a hover
                   title is a dead end on touch and on OCR'd VMs alike. -->
              <span v-if="subscription.last_error" class="failure-reason">
                {{ subscription.last_error }}
              </span>
            </td>
            <td class="actions">
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :disabled="store.busy === subscription.id"
                @click="refreshOne(subscription)"
              >
                {{ store.busy === subscription.id ? t('common.refreshing') : t('common.refresh') }}
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :title="t('common.edit')"
                :aria-label="t('common.edit')"
                @click="openEdit(subscription)"
              >
                <AppIcon name="edit" :size="14" />
              </button>
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                :title="t('common.delete')"
                :aria-label="t('common.delete')"
                @click="confirming = subscription"
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
      :title="form.id ? t('sub.editor.edit') : t('sub.editor.new')"
      :close-on-overlay="false"
      @close="editing = false"
    >
      <div class="field">
        <label class="field-label" for="sub-name">{{ t('sub.editor.name') }}</label>
        <input id="sub-name" v-model="form.name" class="input" :placeholder="t('sub.editor.namePlaceholder')" />
      </div>
      <div class="field">
        <label class="field-label" for="sub-url">{{ t('sub.editor.url') }}</label>
        <input
          id="sub-url"
          v-model="form.url"
          class="input input-mono"
          placeholder="https://example.com/api/v1/client/subscribe?token=…"
        />
        <p class="field-hint">{{ t('sub.editor.urlHint') }}</p>
      </div>
      <!-- The source's own policy. Kept next to the url because it is part of
           "what this source brings in", not a refresh setting; all three apply
           while the document is parsed, so they show up on the next refresh. -->
      <div class="field">
        <p class="field-hint">{{ t('sub.editor.tuningHint') }}</p>
      </div>
      <div class="field">
        <label class="field-label" for="sub-prefix">{{ t('sub.editor.prefix') }}</label>
        <input
          id="sub-prefix"
          v-model="form.rename_prefix"
          class="input"
          :placeholder="t('sub.editor.prefixPlaceholder')"
        />
        <p class="field-hint">{{ t('sub.editor.prefixHint') }}</p>
      </div>
      <div class="field">
        <label class="field-label" for="sub-include">{{ t('sub.editor.include') }}</label>
        <input
          id="sub-include"
          v-model="form.include"
          class="input input-mono"
          :placeholder="t('sub.editor.includePlaceholder')"
        />
        <p class="field-hint">{{ t('sub.editor.includeHint') }}</p>
      </div>
      <div class="field">
        <label class="field-label" for="sub-exclude">{{ t('sub.editor.exclude') }}</label>
        <input
          id="sub-exclude"
          v-model="form.exclude"
          class="input input-mono"
          :placeholder="t('sub.editor.excludePlaceholder')"
        />
        <p class="field-hint">{{ t('sub.editor.excludeHint') }}</p>
      </div>
      <div class="field">
        <label class="field-label" for="sub-interval">{{ t('sub.editor.interval') }}</label>
        <select id="sub-interval" v-model.number="form.update_interval_minutes" class="select">
          <option v-for="option in intervals" :key="option.value" :value="option.value">
            {{ t(option.labelKey) }}
          </option>
        </select>
      </div>
      <div class="field">
        <ToggleSwitch
          v-model="form.enabled"
          :label="t('sub.editor.enabled')"
          :description="t('sub.editor.enabledHint')"
        />
      </div>
      <p v-if="formError" class="field-error">{{ formError }}</p>
      <template #footer>
        <button type="button" class="btn btn-ghost btn-sm" @click="editing = false">
          {{ t('common.cancel') }}
        </button>
        <button type="button" class="btn btn-primary btn-sm" :disabled="saving" @click="submit">
          {{ saving ? t('common.saving') : t('common.save') }}
        </button>
      </template>
    </ModalDialog>

    <ModalDialog :open="confirming !== null" :title="t('sub.delete.title')" @close="confirming = null">
      <p>{{ t('sub.delete.body', { name: confirming?.name ?? '' }) }}</p>
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
.dim {
  opacity: 0.55;
}
.num {
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.table-wrap {
  /* Eight columns do not fit a 1050px window: without this the status and
     action columns are simply clipped away with no way to reach them. */
  overflow-x: auto;
}

/* The quota lives under the name instead of in its own column: as a column it
   was the one that got scrolled out of sight, and it is the number people open
   this page for. */
.sub-name {
  line-height: 1.3;
  max-width: 250px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* Wraps rather than forcing the whole table to scroll: "12.5 GB / 100.0 GB ·
   到期 2026-03-01" on one line alone is 230px wide. */
.nowrap {
  white-space: nowrap;
}

.failure-reason {
  display: block;
  max-width: 34ch;
  margin-top: 4px;
  font-size: 12px;
  line-height: 1.45;
  color: var(--text-secondary);
  overflow-wrap: anywhere;
}

.sub-quota {
  font-family: var(--font-mono);
  font-size: 11.5px;
  color: var(--text-secondary);
  white-space: nowrap;
  line-height: 1.35;
}
/* The rename/filter policy: quieter than the quota lines, and clipped rather
   than wrapped because a long regex would otherwise widen the name column. */
.sub-tuning {
  max-width: 250px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--font-mono);
  font-size: 11.5px;
  line-height: 1.35;
  color: var(--text-tertiary);
}
/* Same definition as the other data views: `muted small` is the page's quiet
   second line, and the countdown is exactly that. */
.small {
  font-size: 12px;
}
.actions {
  text-align: right;
  white-space: nowrap;
}
</style>
