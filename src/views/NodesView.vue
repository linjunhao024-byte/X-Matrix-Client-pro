<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import ImportDialog from '../components/ImportDialog.vue'
import ModalDialog from '../components/ModalDialog.vue'
import NodeEditor from '../components/NodeEditor.vue'
import { t } from '../i18n'
import {
  api,
  describeError,
  type EngineKind,
  type ImportOutcome_Serialize,
  type Node_Deserialize,
  type Node_Serialize,
} from '../services/contract'
import { useEngineStore } from '../stores/engine'
import { useNodesStore } from '../stores/nodes'
import { useSettingsStore } from '../stores/settings'
import { useSubscriptionsStore } from '../stores/subscriptions'

const store = useNodesStore()
const subscriptions = useSubscriptionsStore()
const engine = useEngineStore()
const settings = useSettingsStore()

const editorOpen = ref(false)
const editing = ref<Node_Serialize | null>(null)
const importOpen = ref(false)
const importNote = ref<string | null>(null)
const actionError = ref<string | null>(null)
const pendingDelete = ref<Node_Serialize | null>(null)
const deleting = ref(false)
const startingId = ref<string | null>(null)
const testingId = ref<string | null>(null)
const testingAll = ref(false)
const search = ref('')
const selecting = ref(false)
const selected = ref<string[]>([])
const copying = ref(false)
const copyNote = ref<string | null>(null)
const bulkPending = ref(false)
const tagFilter = ref('')
const tagPending = ref(false)
const tagDraft = ref('')
const tagRemove = ref(false)
const tagBusy = ref(false)
/** id -> milliseconds, or 'timeout' when the handshake never completed. */
const latency = ref<Record<string, number | 'timeout' | 'quic'>>({})

type SortKey = 'manual' | 'name' | 'latency' | 'protocol'
const sortBy = ref<SortKey>('manual')
const sortOptions: { value: SortKey; labelKey: string }[] = [
  // Manual first: the order the user arranged is the one they expect to see
  // when they open the page, and every other sort is a lens on top of it.
  { value: 'manual', labelKey: 'nodes.sort.manual' },
  { value: 'name', labelKey: 'nodes.sort.name' },
  { value: 'latency', labelKey: 'nodes.sort.latency' },
  { value: 'protocol', labelKey: 'nodes.sort.protocol' },
]

const kinds: { value: EngineKind; label: string }[] = [
  { value: 'xray', label: 'Xray' },
  { value: 'sing-box', label: 'sing-box' },
]

const isEmpty = computed(() => store.loaded && store.items.length === 0)

/** Every tag in use, so the filter never offers a tag that matches nothing. */
/**
 * Tags are machine names; `sub:<id>` is the subscription's own tag.
 *
 * Showing `sub:sub-3f2a…` on a card tells the user nothing, so the id is
 * swapped for the subscription's name when we still have that subscription.
 * Unknown ids fall back to the raw tag instead of disappearing — a node whose
 * subscription was deleted should still say where it came from.
 */
function tagLabel(tag: string): string {
  if (!tag.startsWith('sub:')) return tag
  const id = tag.slice('sub:'.length)
  const name = subscriptions.items.find((item) => item.id === id)?.name
  return name ? t('nodes.fromSubscription', { name }) : tag
}

const allTags = computed(() => {
  const seen = new Set<string>()
  for (const node of store.items) for (const tag of node.tags) seen.add(tag)
  return [...seen].sort((left, right) => left.localeCompare(right))
})

/** Search matches what is printed on the card, so nobody has to guess. */
const filteredNodes = computed(() => {
  const needle = search.value.trim().toLowerCase()
  return store.items.filter((node) => {
    if (tagFilter.value && !node.tags.includes(tagFilter.value)) return false
    if (!needle) return true
    const haystack = [node.name, node.server, String(node.port), node.protocol.kind, ...node.tags]
      .join(' ')
      .toLowerCase()
    return haystack.includes(needle)
  })
})

/**
 * Clash sorts by latency and keeps the untested ones at the bottom: a node you
 * have not measured is not "fast", it is unknown, and mixing the two makes the
 * list jump around while the probes land.
 */
const sortedNodes = computed(() => {
  const items = [...filteredNodes.value]
  if (sortBy.value === 'manual') {
    // No sort: the stored order is the answer. Copying first keeps Vue from
    // mutating the store's array when a filter is active.
    return items
  }
  if (sortBy.value === 'latency') {
    const rank = (node: Node_Serialize): number => {
      const value = latency.value[node.id]
      if (typeof value === 'number') return value
      return Number.POSITIVE_INFINITY
    }
    return items.sort((left, right) => {
      const delta = rank(left) - rank(right)
      if (delta !== 0) return delta
      return left.name.localeCompare(right.name)
    })
  }
  if (sortBy.value === 'protocol') {
    return items.sort((left, right) => {
      const delta = left.protocol.kind.localeCompare(right.protocol.kind)
      return delta !== 0 ? delta : left.name.localeCompare(right.name)
    })
  }
  return items.sort((left, right) => left.name.localeCompare(right.name))
})

/**
 * Periodic latency refresh, driven by `settings.auto_test_minutes`.
 *
 * One timer for the whole page, re-armed whenever the interval changes, and it
 * stays quiet while a manual test is already running — two overlapping sweeps
 * would measure each other's queue rather than the nodes.
 */
let autoTestTimer: ReturnType<typeof setInterval> | undefined

function stopAutoTest() {
  if (!autoTestTimer) return
  clearInterval(autoTestTimer)
  autoTestTimer = undefined
}

function armAutoTest(minutes: number | undefined) {
  stopAutoTest()
  if (!minutes || minutes <= 0) return
  autoTestTimer = setInterval(() => {
    if (testingAll.value || store.items.length === 0) return
    void testAll()
  }, minutes * 60_000)
}

watch(
  () => settings.settings?.auto_test_minutes,
  (minutes) => armAutoTest(minutes),
  { immediate: true },
)

onMounted(() => {
  void store.load()
  void settings.load()
  // Only for the tag labels: a node's `sub:` tag means nothing until we can
  // name the subscription it came from.
  void subscriptions.load()
})

onUnmounted(stopAutoTest)

function isSelected(id: string): boolean {
  return selected.value.includes(id)
}

function toggleSelect(id: string) {
  selected.value = isSelected(id)
    ? selected.value.filter((item) => item !== id)
    : [...selected.value, id]
}

function toggleSelectAll() {
  const visible = filteredNodes.value.map((node) => node.id)
  const allSelected = visible.length > 0 && visible.every(isSelected)
  selected.value = allSelected ? [] : visible
}

function leaveSelection() {
  selecting.value = false
  selected.value = []
}

/** Copy needs a writable clipboard; WebView2 refuses it in some profiles, so a
 *  failed copy must say so instead of pretending. */
/**
 * Whether a node can move in that direction *in the current view*.
 *
 * The list may be filtered or sorted, so "one slot up" is only meaningful in
 * manual order with no filter narrowing the list — otherwise the button would
 * swap the node with something the user cannot see.
 */
function canMove(node: Node_Serialize, delta: number): boolean {
  if (sortBy.value !== 'manual') return false
  const list = sortedNodes.value
  const index = list.findIndex((item) => item.id === node.id)
  if (index < 0) return false
  return index + delta >= 0 && index + delta < list.length
}

async function moveNode(node: Node_Serialize, delta: number): Promise<void> {
  try {
    await store.move(node.id, delta)
  } catch (problem) {
    store.error = describeError(problem)
  }
}

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    return true
  } catch {
    return false
  }
}

let copyTimer: ReturnType<typeof setTimeout> | null = null

async function copyLinks(ids: string[]) {
  if (ids.length === 0) return
  copying.value = true
  copyNote.value = null
  actionError.value = null
  try {
    const text = await store.exportLinks(ids)
    copyNote.value = (await copyText(text))
      ? t('nodes.copied', { count: ids.length })
      : t('nodes.copyFailed')
    // The confirmation refers to a moment, not to a state: leaving it up makes
    // the page look like it is still reporting something.
    if (copyTimer) clearTimeout(copyTimer)
    copyTimer = setTimeout(() => {
      copyNote.value = null
    }, 4000)
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    copying.value = false
  }
}

/**
 * Save the selected links as a text file.
 *
 * The clipboard is fine for one link; a whole set is something people keep, and
 * every other client offers a file. Same payload as `copyLinks`, only it lands
 * on disk where it can be edited before sharing.
 */
async function saveLinks(ids: string[]) {
  if (ids.length === 0) return
  copying.value = true
  copyNote.value = null
  actionError.value = null
  try {
    const text = await store.exportLinks(ids)
    const stamp = new Date().toISOString().slice(0, 10)
    const path = await api.saveTextFile(`x-matrix-nodes-${stamp}.txt`, text)
    if (path === null) return
    copyNote.value = t('nodes.savedFile', { path })
    if (copyTimer) clearTimeout(copyTimer)
    copyTimer = setTimeout(() => {
      copyNote.value = null
    }, 6000)
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    copying.value = false
  }
}

async function testSelected() {
  const targets = filteredNodes.value.filter((node) => isSelected(node.id))
  testingAll.value = true
  actionError.value = null
  try {
    for (const node of targets) {
      testingId.value = node.id
      await measure(node)
    }
  } finally {
    testingId.value = null
    testingAll.value = false
  }
}

function openTagDialog() {
  tagDraft.value = ''
  tagRemove.value = false
  tagPending.value = true
}

async function confirmTag() {
  const tag = tagDraft.value.trim()
  if (!tag) return
  tagBusy.value = true
  actionError.value = null
  try {
    await store.tagMany([...selected.value], tag, tagRemove.value)
    tagPending.value = false
    // Removing the tag the filter is pinned to would empty the page with no
    // explanation, so the filter follows the change instead.
    if (tagRemove.value && tagFilter.value === tag) tagFilter.value = ''
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    tagBusy.value = false
  }
}

async function confirmBulkDelete() {
  deleting.value = true
  actionError.value = null
  try {
    await store.removeMany([...selected.value])
    bulkPending.value = false
    leaveSelection()
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    deleting.value = false
  }
}

function openCreate() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(node: Node_Serialize) {
  editing.value = node
  editorOpen.value = true
}

async function submit(node: Node_Deserialize) {
  await store.save(node)
  editorOpen.value = false
}

async function confirmDelete() {
  const target = pendingDelete.value
  if (!target) return
  deleting.value = true
  actionError.value = null
  try {
    await store.remove(target.id)
    pendingDelete.value = null
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    deleting.value = false
  }
}

async function startNode(node: Node_Serialize) {
  startingId.value = node.id
  actionError.value = null
  try {
    await engine.start(node.id)
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    startingId.value = null
  }
}

/**
 * A QUIC node has no TCP handshake to time. Sending the probe anyway would
 * either time out (which reads as "slow node") or reach a UDP port with a TCP
 * connect and come back with a number that measured nothing.
 */
function speaksQuic(node: Node_Serialize): boolean {
  return node.protocol.kind === 'Hysteria2' || node.protocol.kind === 'Tuic'
}

async function measure(node: Node_Serialize) {
  if (speaksQuic(node)) {
    latency.value = { ...latency.value, [node.id]: 'quic' }
    return
  }
  try {
    const ms = await api.testNodeLatency(node.id)
    latency.value = { ...latency.value, [node.id]: ms }
  } catch {
    // A node that cannot be reached is a fact about the node, not an error
    // worth an alert bar: show it on the card instead.
    latency.value = { ...latency.value, [node.id]: 'timeout' }
  }
}

async function testLatency(node: Node_Serialize) {
  testingId.value = node.id
  actionError.value = null
  try {
    await measure(node)
  } finally {
    testingId.value = null
  }
}

/** Sequential on purpose: a burst of parallel handshakes measures the queue,
 *  not the nodes. */
async function testAll() {
  testingAll.value = true
  actionError.value = null
  try {
    for (const node of store.items) {
      testingId.value = node.id
      await measure(node)
    }
  } finally {
    testingId.value = null
    testingAll.value = false
  }
}

function latencyLabel(node: Node_Serialize): string {
  const value = latency.value[node.id]
  if (value === undefined) return t('nodes.latency.none')
  if (value === 'quic') return t('nodes.latency.quic')
  return value === 'timeout' ? t('common.timeout') : t('nodes.latency.ms', { value })
}

function latencyTone(node: Node_Serialize): string {
  const value = latency.value[node.id]
  if (value === undefined) return 'latency-unknown'
  if (value === 'timeout') return 'latency-bad'
  if (value === 'quic') return 'latency-unknown'
  if (value < 150) return 'latency-good'
  if (value < 400) return 'latency-mid'
  return 'latency-bad'
}

function isRunning(node: Node_Serialize): boolean {
  return engine.running && engine.status?.node_name === node.name
}

function onImported(outcome: ImportOutcome_Serialize) {
  const parts = [t('nodes.imported', { imported: outcome.imported ?? 0 })]
  if ((outcome.groups ?? 0) > 0)
    parts.push(t('nodes.importedGroups', { count: outcome.groups ?? 0 }))
  if ((outcome.rules ?? 0) > 0)
    parts.push(t('nodes.importedRules', { count: outcome.rules ?? 0 }))
  if ((outcome.duplicates ?? 0) > 0)
    parts.push(t('nodes.importedDup', { count: outcome.duplicates ?? 0 }))
  if (outcome.skipped.length > 0)
    parts.push(t('nodes.importedSkipped', { count: outcome.skipped.length }))
  importNote.value = parts.join(' · ')
}

function transportLabel(node: Node_Serialize): string {
  const transport = node.transport
  if (transport.kind === 'Tcp') return 'TCP'
  if (transport.kind === 'Ws') return `WS ${transport.path}`
  return `gRPC ${transport.service_name}`
}

function securityLabel(node: Node_Serialize): string {
  const security = node.security
  if (security.kind === 'None') return t('common.none')
  if (security.kind === 'Tls') return security.sni ? `TLS · ${security.sni}` : 'TLS'
  return `Reality · ${security.server_name}`
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.nodes.title') }}</h1>
        <p class="page-subtitle">{{ t('page.nodes.subtitle') }}</p>
      </div>
      <div class="row">
        <div class="segmented" :title="t('nodes.engineHint')">
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
        <button class="btn" type="button" :disabled="store.loading" @click="store.load(true)">
          <AppIcon name="refresh" :size="15" />
          {{ t('common.refresh') }}
        </button>
        <button class="btn" type="button" @click="importOpen = true">
          <AppIcon name="arrow-down" :size="15" />
          {{ t('nodes.import') }}
        </button>
        <button class="btn btn-primary" type="button" @click="openCreate">
          <AppIcon name="plus" :size="15" />
          {{ t('nodes.new') }}
        </button>
      </div>
    </header>

    <p v-if="store.error" class="alert">{{ store.error }}</p>
    <p v-if="actionError" class="alert">{{ actionError }}</p>
    <p v-if="importNote" class="notice">{{ importNote }}</p>

    <div v-if="store.loading && !store.loaded" class="empty-state">
      <div class="empty-state-title">{{ t('common.loading') }}</div>
    </div>

    <EmptyState
      v-else-if="isEmpty"
      icon="nodes"
      :title="t('nodes.empty.title')"
      :body="t('nodes.empty.body')"
    >
      <template #actions>
        <button class="btn btn-primary" type="button" @click="openCreate">{{ t('nodes.new') }}</button>
        <button class="btn" type="button" @click="importOpen = true">{{ t('nodes.importLinks') }}</button>
      </template>
    </EmptyState>

    <template v-else>
      <div class="toolbar">
        <div class="row">
          <span class="muted">{{ t('nodes.sort') }}</span>
          <div class="segmented">
            <button
              v-for="option in sortOptions"
              :key="option.value"
              type="button"
              :class="{ active: sortBy === option.value }"
              :aria-pressed="sortBy === option.value"
              @click="sortBy = option.value"
            >
              {{ t(option.labelKey) }}
            </button>
          </div>
          <span v-if="sortBy === 'latency'" class="muted small">{{ t('nodes.sortHint') }}</span>
          <span v-else-if="sortBy === 'manual'" class="muted small">{{ t('nodes.manualHint') }}</span>
        </div>
        <div class="row">
          <select
            v-if="allTags.length > 0"
            v-model="tagFilter"
            class="select select-tag"
            :aria-label="t('nodes.tagFilter')"
          >
            <option value="">{{ t('nodes.tagFilterAll') }}</option>
            <option v-for="tag in allTags" :key="tag" :value="tag">{{ tagLabel(tag) }}</option>
          </select>
          <input
            v-model="search"
            class="input input-search"
            type="search"
            :placeholder="t('nodes.search')"
            :aria-label="t('nodes.search')"
          />
          <button
            class="btn btn-sm"
            type="button"
            :disabled="testingAll || testingId !== null"
            @click="testAll"
          >
            <AppIcon name="refresh" :size="14" />
            {{ testingAll ? t('common.testing') : t('nodes.testAll') }}
          </button>
          <button
            class="btn btn-sm"
            type="button"
            :class="{ 'btn-primary': selecting }"
            @click="selecting ? leaveSelection() : (selecting = true)"
          >
            <AppIcon name="check" :size="14" />
            {{ selecting ? t('nodes.selectDone') : t('nodes.select') }}
          </button>
        </div>
      </div>
      <p class="muted small latency-hint">{{ t('nodes.latencyHint') }}</p>

      <div v-if="selecting" class="select-bar glass">
        <span class="muted">{{ t('nodes.selected', { count: selected.length }) }}</span>
        <div class="row">
          <button class="btn btn-sm" type="button" @click="toggleSelectAll">
            {{ t('nodes.selectAll') }}
          </button>
          <button
            class="btn btn-sm"
            type="button"
            :disabled="selected.length === 0 || copying"
            @click="copyLinks(selected)"
          >
            <AppIcon name="copy" :size="14" />
            {{ t('nodes.copyLink') }}
          </button>
          <button
            class="btn btn-sm"
            type="button"
            :disabled="selected.length === 0 || copying"
            @click="saveLinks(selected)"
          >
            <AppIcon name="file" :size="14" />
            {{ t('nodes.saveToFile') }}
          </button>
          <button
            class="btn btn-sm"
            type="button"
            :disabled="selected.length === 0 || testingAll"
            @click="testSelected"
          >
            <AppIcon name="refresh" :size="14" />
            {{ t('nodes.testSelected') }}
          </button>
          <button
            class="btn btn-sm"
            type="button"
            :disabled="selected.length === 0"
            @click="openTagDialog"
          >
            <AppIcon name="layers" :size="14" />
            {{ t('nodes.tag.action') }}
          </button>
          <button
            class="btn btn-sm btn-danger"
            type="button"
            :disabled="selected.length === 0"
            @click="bulkPending = true"
          >
            <AppIcon name="trash" :size="14" />
            {{ t('nodes.deleteSelected') }}
          </button>
        </div>
      </div>

      <p v-if="copyNote" class="notice">{{ copyNote }}</p>

      <div v-if="filteredNodes.length === 0" class="empty-state">
        <div class="empty-state-title">{{ t('nodes.searchEmpty') }}</div>
        <button class="btn btn-sm" type="button" @click="search = ''">
          {{ t('nodes.clearSearch') }}
        </button>
      </div>

      <div class="node-grid">
        <article
          v-for="node in sortedNodes"
          :key="node.id"
          class="node-card glass"
          :class="{ 'is-running': isRunning(node), 'is-selected': isSelected(node.id) }"
          @click="selecting && toggleSelect(node.id)"
        >
          <header class="node-head">
            <input
              v-if="selecting"
              class="node-check"
              type="checkbox"
              :checked="isSelected(node.id)"
              :aria-label="node.name"
              @click.stop
              @change="toggleSelect(node.id)"
            />
            <span class="node-name" :title="node.name">{{ node.name }}</span>
            <span class="badge badge-accent">{{ node.protocol.kind }}</span>
            <span v-if="isRunning(node)" class="badge badge-success">
              {{ t('common.running') }}
            </span>
          </header>

          <div class="node-addr mono">{{ node.server }}:{{ node.port }}</div>

          <div class="node-meta">
            <span class="mono">{{ transportLabel(node) }}</span>
            <span class="dot">·</span>
            <span class="mono">{{ securityLabel(node) }}</span>
          </div>

          <div v-if="node.tags.length > 0" class="node-tags">
            <span v-for="tag in node.tags" :key="tag" class="tag-chip" :title="tag">{{ tagLabel(tag) }}</span>
          </div>

          <footer class="node-foot">
            <span class="latency" :class="latencyTone(node)">{{ latencyLabel(node) }}</span>
            <div class="row">
              <button
                v-if="isRunning(node)"
                class="btn btn-sm btn-running"
                type="button"
                disabled
              >
                <AppIcon name="check" :size="14" />
                {{ t('common.running') }}
              </button>
              <button
                v-else
                class="btn btn-sm btn-primary"
                type="button"
                :disabled="engine.busy || startingId !== null"
                @click.stop="startNode(node)"
              >
                <AppIcon name="play" :size="14" />
                {{ startingId === node.id ? t('common.starting') : t('common.start') }}
              </button>
              <button
                class="btn btn-ghost btn-sm"
                type="button"
                :disabled="testingId !== null"
                :title="t('common.test')"
                :aria-label="t('common.test')"
                @click.stop="testLatency(node)"
              >
                <AppIcon name="refresh" :size="14" />
              </button>
              <template v-if="sortBy === 'manual'">
                <button
                  class="btn btn-ghost btn-sm"
                  type="button"
                  :disabled="!canMove(node, -1)"
                  :title="t('nodes.moveUp')"
                  :aria-label="t('nodes.moveUp')"
                  @click.stop="moveNode(node, -1)"
                >
                  <AppIcon name="arrow-up" :size="14" />
                </button>
                <button
                  class="btn btn-ghost btn-sm"
                  type="button"
                  :disabled="!canMove(node, 1)"
                  :title="t('nodes.moveDown')"
                  :aria-label="t('nodes.moveDown')"
                  @click.stop="moveNode(node, 1)"
                >
                  <AppIcon name="arrow-down" :size="14" />
                </button>
              </template>
              <button
                class="btn btn-ghost btn-sm"
                type="button"
                :title="t('common.edit')"
                :aria-label="t('common.edit')"
                @click.stop="openEdit(node)"
              >
                <AppIcon name="edit" :size="14" />
              </button>
              <button
                class="btn btn-ghost btn-sm"
                type="button"
                :disabled="copying"
                :title="t('nodes.copyLink')"
                :aria-label="t('nodes.copyLink')"
                @click.stop="copyLinks([node.id])"
              >
                <AppIcon name="copy" :size="14" />
              </button>
              <button
                class="btn btn-ghost btn-sm"
                type="button"
                :title="t('common.delete')"
                :aria-label="t('common.delete')"
                @click.stop="pendingDelete = node"
              >
                <AppIcon name="trash" :size="14" />
              </button>
            </div>
          </footer>
        </article>
      </div>
    </template>

    <NodeEditor
      :open="editorOpen"
      :node="editing"
      :on-submit="submit"
      @close="editorOpen = false"
    />

    <ImportDialog :open="importOpen" @close="importOpen = false" @imported="onImported" />

    <ModalDialog
      :open="pendingDelete !== null"
      :title="t('nodes.delete.title')"
      :width="420"
      @close="pendingDelete = null"
    >
      <p class="muted">
        {{ t('nodes.delete.body', { name: pendingDelete?.name ?? '' }) }}
      </p>
      <template #footer>
        <button class="btn" type="button" @click="pendingDelete = null">{{ t('common.cancel') }}</button>
        <button class="btn btn-primary" type="button" :disabled="deleting" @click="confirmDelete">
          {{ deleting ? t('common.deleting') : t('common.confirm') }}
        </button>
      </template>
    </ModalDialog>

    <ModalDialog
      :open="tagPending"
      :title="t('nodes.tag.title')"
      :width="440"
      @close="tagPending = false"
    >
      <label class="field">
        <span class="field-label">{{ t('nodes.tag.name') }}</span>
        <input
          v-model="tagDraft"
          class="input"
          :placeholder="t('nodes.tag.placeholder')"
          @keyup.enter="confirmTag"
        />
        <span class="field-hint">{{ t('nodes.tag.hint', { count: selected.length }) }}</span>
      </label>
      <div class="segmented">
        <button type="button" :class="{ active: !tagRemove }"
              :aria-pressed="!tagRemove" @click="tagRemove = false">
          {{ t('nodes.tag.add') }}
        </button>
        <button type="button" :class="{ active: tagRemove }"
              :aria-pressed="tagRemove" @click="tagRemove = true">
          {{ t('nodes.tag.remove') }}
        </button>
      </div>
      <template #footer>
        <button class="btn" type="button" @click="tagPending = false">{{ t('common.cancel') }}</button>
        <button
          class="btn btn-primary"
          type="button"
          :disabled="tagBusy || tagDraft.trim() === ''"
          @click="confirmTag"
        >
          {{ tagBusy ? t('common.saving') : t('common.confirm') }}
        </button>
      </template>
    </ModalDialog>

    <ModalDialog
      :open="bulkPending"
      :title="t('nodes.bulkDelete.title')"
      :width="420"
      @close="bulkPending = false"
    >
      <p class="muted">{{ t('nodes.bulkDelete.body', { count: selected.length }) }}</p>
      <template #footer>
        <button class="btn" type="button" @click="bulkPending = false">{{ t('common.cancel') }}</button>
        <button class="btn btn-primary" type="button" :disabled="deleting" @click="confirmBulkDelete">
          {{ deleting ? t('common.deleting') : t('common.confirm') }}
        </button>
      </template>
    </ModalDialog>
  </div>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

/* Sits directly under the toolbar: a green latency badge must not be read as
   "this node works", so the caveat lives next to the button that produces it. */
.latency-hint {
  margin: 6px 0 0;
  line-height: 1.5;
}

.small {
  font-size: 12px;
}

.node-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 12px;
}

.node-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 14px;
  border-radius: var(--radius-card);
  transition:
    border-color var(--duration-base) var(--ease-out),
    transform var(--duration-base) var(--ease-out);
}

.node-card:hover {
  transform: translateY(-1px);
}

.is-running {
  border-color: color-mix(in srgb, var(--success) 45%, var(--glass-border));
}

/* Selecting is a mode, so the selected card has to look picked even before the
   pointer hovers it. */
.is-selected {
  border-color: color-mix(in srgb, var(--accent) 60%, var(--glass-border));
  background: color-mix(in srgb, var(--accent-soft) 55%, transparent);
}

.node-check {
  flex: 0 0 auto;
  width: 15px;
  height: 15px;
  accent-color: var(--accent);
  cursor: pointer;
}

.input-search {
  width: 200px;
}

.select-tag {
  width: 150px;
}

/* Tags are how a 40-node subscription becomes findable, so they get their own
   row instead of being crammed into the mono meta line. */
.node-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.tag-chip {
  font-size: 10.5px;
  line-height: 1.6;
  padding: 0 6px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--border-subtle);
  background: var(--surface-sunken);
  color: var(--text-secondary);
  max-width: 120px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* The bulk bar only exists while selecting, so it can afford to be loud. */
.select-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
  padding: 8px 12px;
  border-radius: var(--radius-card);
  margin-top: 4px;
}

.node-head {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.node-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
  color: var(--text-primary);
}

.node-addr {
  font-size: 12px;
  color: var(--text-secondary);
}

.node-meta > span {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
}

.node-meta > .dot {
  flex: none;
}

.node-meta {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  color: var(--text-tertiary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.dot {
  opacity: 0.6;
}

.node-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  /* Manual order adds two arrows next to start/test/edit/copy/delete; without
     wrapping those seven controls run past the card and delete hangs outside it. */
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 2px;
}

.btn-running {
  color: var(--success);
  border-color: color-mix(in srgb, var(--success) 40%, transparent);
}

.latency {
  font-size: 11px;
  padding: 1px 6px;
  border-radius: var(--radius-pill);
  border: 1px solid var(--border-subtle);
  white-space: nowrap;
}

.latency-unknown {
  color: var(--text-tertiary);
}

.latency-good {
  color: var(--success);
  border-color: var(--success);
}

.latency-mid {
  color: var(--warning);
  border-color: var(--warning);
}

.latency-bad {
  color: var(--danger);
  border-color: var(--danger);
}
</style>
