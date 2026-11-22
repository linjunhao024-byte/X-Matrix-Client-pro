<script setup lang="ts">
/**
 * Connections: a live view of what the kernel reports.
 *
 * The three empty states are deliberately distinct — "no engine", "this kernel
 * cannot report connections" and "nothing is connected right now" mean very
 * different things to someone debugging their network.
 */
import { computed, ref } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import { t } from '../i18n'
import { api, describeError } from '../services/contract'
import { formatBytes, formatClock, formatRate } from '../services/format'
import { groupConnections } from '../services/connections'
import { connectionsToCsv } from '../services/csv'
import { useEngineStore } from '../stores/engine'

const engine = useEngineStore()
const closing = ref<string | null>(null)
const closeError = ref<string | null>(null)

/**
 * Drop one connection.
 *
 * Only sing-box can do this — its Clash API keeps a table with ids. The button
 * is hidden on Xray rather than shown-and-failing, because a control that can
 * only report "not supported" is worse than no control.
 */
async function disconnect(id: string): Promise<void> {
  closing.value = id
  closeError.value = null
  try {
    await api.closeConnection(id)
    // The next sample removes the row; no local surgery, so the list always
    // mirrors the kernel.
  } catch (error) {
    closeError.value = describeError(error)
  } finally {
    closing.value = null
  }
}

const engineLabel = computed(() => (engine.status?.kind === 'sing-box' ? 'sing-box' : 'Xray'))

/**
 * Sorting and filtering are client-side because the list is small (the kernel
 * only keeps live connections) and re-sorting a live table on every poll would
 * make rows jump while you read them. The order is stable between polls unless
 * the numbers actually change.
 */
type SortKey = 'start' | 'download' | 'upload' | 'host'

const search = ref('')
const sortBy = ref<SortKey>('start')

const rows = computed(() => {
  const needle = search.value.trim().toLowerCase()
  const matched = needle
    ? engine.connections.filter((row) =>
        [row.host, row.network, row.rule, ...row.chains]
          .filter(Boolean)
          .some((field) => String(field).toLowerCase().includes(needle)),
      )
    : [...engine.connections]

  return matched.sort((a, b) => {
    switch (sortBy.value) {
      // The kernel may omit the byte counts; treat a missing number as zero
      // rather than letting `undefined` poison the comparison.
      case 'download':
        return (b.download ?? 0) - (a.download ?? 0)
      case 'upload':
        return (b.upload ?? 0) - (a.upload ?? 0)
      case 'host':
        return (a.host ?? '').localeCompare(b.host ?? '')
      default:
        // Newest first: the connection you just made is the one you are looking at.
        return (b.start ?? '').localeCompare(a.start ?? '')
    }
  })
})

/**
 * Export what is on screen, not the raw kernel table: if the user filtered by
 * "github", the CSV holds the github rows. Anything else would make the export
 * disagree with the window it came from.
 */
const exporting = ref(false)
const exportNote = ref<string | null>(null)

async function exportCsv(): Promise<void> {
  exporting.value = true
  exportNote.value = null
  try {
    const stamp = new Date().toISOString().slice(0, 19).replace(/[:T]/g, '-')
    const saved = await api.saveTextFile(`x-matrix-connections-${stamp}.csv`, connectionsToCsv(rows.value))
    // `null` means the user closed the dialog; that is not an error and not
    // worth a message.
    if (saved) exportNote.value = t('conn.exported', { path: saved })
  } catch (error) {
    exportNote.value = describeError(error)
  } finally {
    exporting.value = false
  }
}

/**
 * Two readings of the same table: the live list, or a roll-up that answers
 * "where is my traffic going". Grouping happens client-side for the same
 * reason sorting does — the kernel only knows about live connections.
 */
type ViewMode = 'list' | 'host' | 'chain'

const viewMode = ref<ViewMode>('list')
const modes: { value: ViewMode; labelKey: string }[] = [
  { value: 'list', labelKey: 'conn.view.list' },
  { value: 'host', labelKey: 'conn.view.host' },
  { value: 'chain', labelKey: 'conn.view.chain' },
]

const groups = computed(() =>
  groupConnections(rows.value, viewMode.value === 'host' ? 'host' : 'chain'),
)

function chainLabel(chains: string[]): string {
  return chains.length > 0 ? chains.join(' → ') : '—'
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.connections.title') }}</h1>
        <p class="page-subtitle">{{ t('page.connections.subtitle') }}</p>
      </div>
      <div class="row">
        <span class="badge" :class="engine.running ? 'badge-success' : ''">
          {{ engine.running ? t('conn.sampling') : t('common.stopped') }}
        </span>
      </div>
    </header>

    <section class="surface-plain rates">
      <div class="rate">
        <AppIcon name="arrow-down" :size="15" />
        <span class="rate-label">{{ t('conn.rate.down') }}</span>
        <span class="rate-value">{{ engine.running ? formatRate(engine.rates.down) : '—' }}</span>
        <span class="rate-total muted mono">
          {{ engine.sample ? formatBytes(engine.sample.down_total) : '—' }}
        </span>
      </div>
      <div class="rate">
        <AppIcon name="arrow-up" :size="15" />
        <span class="rate-label">{{ t('conn.rate.up') }}</span>
        <span class="rate-value">{{ engine.running ? formatRate(engine.rates.up) : '—' }}</span>
        <span class="rate-total muted mono">
          {{ engine.sample ? formatBytes(engine.sample.up_total) : '—' }}
        </span>
      </div>
      <div class="rate">
        <AppIcon name="connections" :size="15" />
        <span class="rate-label">{{ t('conn.rate.active') }}</span>
        <span class="rate-value">{{ engine.running && engine.hasConnectionsApi ? rows.length : '—' }}</span>
      </div>
    </section>

    <div v-if="engine.running && engine.hasConnectionsApi" class="row conn-toolbar">
      <input
        v-model="search"
        class="input input-search"
        type="search"
        :placeholder="t('conn.search')"
        :aria-label="t('conn.search')"
      />
      <span class="spacer" />
      <div class="segmented">
        <button
          v-for="mode in modes"
          :key="mode.value"
          type="button"
          :class="{ active: viewMode === mode.value }"
              :aria-pressed="viewMode === mode.value"
          @click="viewMode = mode.value"
        >
          {{ t(mode.labelKey) }}
        </button>
      </div>
      <label class="muted small" for="conn-sort">{{ t('conn.sort') }}</label>
      <button class="btn btn-sm" type="button" :disabled="exporting || rows.length === 0" @click="exportCsv">
        {{ exporting ? t('common.loading') : t('conn.export') }}
      </button>
      <select id="conn-sort" v-model="sortBy" class="select select-sort">
        <option value="start">{{ t('conn.sort.start') }}</option>
        <option value="download">{{ t('conn.sort.download') }}</option>
        <option value="upload">{{ t('conn.sort.upload') }}</option>
        <option value="host">{{ t('conn.sort.host') }}</option>
      </select>
    </div>

    <p v-if="exportNote" class="notice">{{ exportNote }}</p>

    <section class="surface-plain">
      <EmptyState
        v-if="!engine.running"
        icon="connections"
        :title="t('conn.empty.stopped.title')"
        :body="t('conn.empty.stopped.body')"
      />

      <EmptyState
        v-else-if="!engine.hasConnectionsApi"
        icon="connections"
        :title="t('conn.empty.unsupported.title')"
        :body="t('conn.empty.unsupported.body')"
      />

      <EmptyState
        v-else-if="engine.connections.length === 0"
        icon="inbox"
        :title="t('conn.empty.idle.title')"
        :body="t('conn.empty.idle.body')"
      />

      <EmptyState
        v-else-if="rows.length === 0"
        icon="inbox"
        :title="t('conn.empty.search.title')"
        :body="t('conn.empty.search.body')"
      />

      <table v-else-if="viewMode !== 'list'" class="data-table">
        <thead>
          <tr>
            <th scope="col">{{ viewMode === 'host' ? t('conn.col.host') : t('conn.col.chains') }}</th>
            <th scope="col" class="col-num">{{ t('conn.col.count') }}</th>
            <th scope="col" class="col-num">{{ t('conn.col.upload') }}</th>
            <th scope="col" class="col-num">{{ t('conn.col.download') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="group in groups" :key="group.name">
            <td class="cell-mono ellipsis">{{ group.unknown ? t('conn.group.unknown') : group.name }}</td>
            <td class="col-num cell-mono">{{ group.count }}</td>
            <td class="col-num cell-mono">{{ formatBytes(group.upload) }}</td>
            <td class="col-num cell-mono">{{ formatBytes(group.download) }}</td>
          </tr>
        </tbody>
      </table>

      <table v-else class="data-table">
        <thead>
          <tr>
            <th scope="col">{{ t('conn.col.host') }}</th>
            <th scope="col">{{ t('conn.col.network') }}</th>
            <th scope="col">{{ t('conn.col.rule') }}</th>
            <th scope="col">{{ t('conn.col.chains') }}</th>
            <th scope="col" class="col-num">{{ t('conn.col.upload') }}</th>
            <th scope="col" class="col-num">{{ t('conn.col.download') }}</th>
            <th scope="col">{{ t('conn.col.start') }}</th>
            <th scope="col" class="actions">{{ t('conn.col.action') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="row in rows" :key="row.id">
            <td class="cell-mono ellipsis">{{ row.host || '—' }}</td>
            <td>{{ row.network || '—' }}</td>
            <td>{{ row.rule || '—' }}</td>
            <td class="cell-mono ellipsis">{{ chainLabel(row.chains) }}</td>
            <td class="col-num cell-mono">{{ formatBytes(row.upload) }}</td>
            <td class="col-num cell-mono">{{ formatBytes(row.download) }}</td>
            <td class="cell-mono">{{ formatClock(row.start) }}</td>
            <td class="actions">
              <button
                class="btn btn-sm"
                type="button"
                :disabled="closing === row.id"
                @click="disconnect(row.id)"
              >
                {{ closing === row.id ? t('conn.closing') : t('conn.close') }}
              </button>
            </td>
          </tr>
        </tbody>
      </table>
    </section>

    <p v-if="closeError" class="alert">{{ closeError }}</p>

    <p class="muted small">
      {{
        t('conn.footer', {
          engine: engine.running ? engineLabel : t('common.none'),
        })
      }}
    </p>
  </div>
</template>

<style scoped>
.conn-toolbar {
  gap: 10px;
}

.input-search {
  width: 220px;
}

.select-sort {
  width: 150px;
}

.rates {
  display: flex;
  flex-wrap: wrap;
  gap: 28px;
  padding: 14px 18px;
}

.rate {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.rate-label {
  color: var(--text-secondary);
}

.rate-value {
  font-family: var(--font-mono);
  font-size: 15px;
  font-weight: 600;
}

.rate-total {
  font-size: 12px;
}

.col-num {
  text-align: right;
  white-space: nowrap;
}

.ellipsis {
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.small {
  font-size: 12px;
}
</style>
