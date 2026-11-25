<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import GroupEditor from '../components/GroupEditor.vue'
import ModalDialog from '../components/ModalDialog.vue'
import StaleConfig from '../components/StaleConfig.vue'
import { t } from '../i18n'
import { useEngineStore } from '../stores/engine'
import { useGroupsStore } from '../stores/groups'
import { useNodesStore } from '../stores/nodes'
import {
  api,
  describeError,
  type GroupKind,
  type ProxyGroup_Deserialize,
  type ProxyGroup_Serialize,
} from '../services/contract'

const store = useGroupsStore()
const nodes = useNodesStore()
const engine = useEngineStore()

const editorOpen = ref(false)
const editing = ref<ProxyGroup_Serialize | null>(null)
const pendingDelete = ref<ProxyGroup_Serialize | null>(null)
const busy = ref(false)
const actionError = ref<string | null>(null)
/**
 * A group edit, a member pick or a toggle only reaches the kernel through the
 * config it rendered at startup. While one runs, the saved list and the live
 * one differ until it is restarted; a stopped kernel has nothing stale.
 */
const stale = ref(false)

function markStale(): void {
  if (engine.running) stale.value = true
}

/// Measured kernel-side delays. Keyed by `group:<id>` and `node:<id>`, because
/// the same node can appear in several groups and the number belongs to the
/// outbound, not to the row.
interface Delay {
  value: number | null
  error: string | null
}
const delays = ref<Record<string, Delay>>({})
const measuring = ref<Record<string, boolean>>({})

function delayKey(scope: 'group' | 'node', id: string): string {
  return `${scope}:${id}`
}

function delayOf(scope: 'group' | 'node', id: string): Delay | undefined {
  return delays.value[delayKey(scope, id)]
}

function isMeasuring(id: string): boolean {
  return measuring.value[id] === true
}

onMounted(async () => {
  await Promise.all([store.load(), nodes.load(), engine.refreshBinaries()])
})

const isEmpty = computed(() => store.loaded && store.items.length === 0)
const enabledCount = computed(() => store.items.filter((group) => group.enabled).length)
const xraySelected = computed(() => engine.kind === 'xray')

/**
 * Badge text per kind. A map, not a ternary: every new kind used to mean
 * editing the template, and a `Record<GroupKind, …>` makes the next one fail
 * to typecheck until it is named here.
 */
const KIND_LABEL: Record<GroupKind, string> = {
  select: 'groups.kind.select',
  'url-test': 'groups.kind.urlTest',
  fallback: 'groups.kind.fallback',
  'load-balance': 'groups.kind.loadBalance',
}

/**
 * The two kinds the app switches itself, by rewriting a selector through the
 * control API. They only work under sing-box — Xray renders them as one static
 * outbound, so the page says so instead of letting the badge imply a switch
 * that will never happen.
 */
const appSwitched = (kind: GroupKind): boolean =>
  kind === 'fallback' || kind === 'load-balance'

const hasAppSwitchedGroup = computed(() => store.items.some((group) => appSwitched(group.kind)))

/** Static chips are all "the kernel picks"; these two deserve their own line. */
function chipTitle(group: ProxyGroup_Serialize): string {
  return appSwitched(group.kind) ? t(`${KIND_LABEL[group.kind]}.hint`) : t('groups.autoHint')
}

function nodeName(id: string): string {
  return nodes.items.find((node) => node.id === id)?.name ?? id
}

function chosenId(group: ProxyGroup_Serialize): string | undefined {
  return group.selected && group.members.includes(group.selected) ? group.selected : group.members[0]
}

function chosenName(group: ProxyGroup_Serialize): string {
  const chosen = chosenId(group)
  return chosen ? nodeName(chosen) : t('common.none')
}

function isChosen(group: ProxyGroup_Serialize, nodeId: string): boolean {
  return chosenId(group) === nodeId
}

function openNew() {
  editing.value = null
  actionError.value = null
  editorOpen.value = true
}

function openEdit(group: ProxyGroup_Serialize) {
  editing.value = group
  actionError.value = null
  editorOpen.value = true
}

async function onSubmit(group: ProxyGroup_Deserialize) {
  busy.value = true
  actionError.value = null
  try {
    await store.save(group)
    markStale()
    editorOpen.value = false
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    busy.value = false
  }
}

async function pick(group: ProxyGroup_Serialize, nodeId: string) {
  if (group.kind !== 'select') return
  try {
    await store.select(group.id, nodeId)
    markStale()
  } catch (cause) {
    actionError.value = describeError(cause)
  }
}

async function toggle(group: ProxyGroup_Serialize) {
  try {
    await store.toggle(group.id)
    markStale()
  } catch (cause) {
    actionError.value = describeError(cause)
  }
}

/// Ask the running kernel to dial through this group and through every member.
///
/// The group number describes what the group would do; the member numbers let
/// you see why a url-test group picked one of them. Both only exist under
/// sing-box — the backend says so in words the UI shows as-is.
async function measureGroup(group: ProxyGroup_Serialize) {
  const key = delayKey('group', group.id)
  measuring.value = { ...measuring.value, [key]: true }
  try {
    const value = await api.groupDelay(group.id)
    delays.value = { ...delays.value, [key]: { value, error: null } }
  } catch (cause) {
    delays.value = { ...delays.value, [key]: { value: null, error: describeError(cause) } }
  } finally {
    measuring.value = { ...measuring.value, [key]: false }
  }
  for (const member of group.members) {
    const nodeKey = delayKey('node', member)
    try {
      const value = await api.nodeDelay(member)
      delays.value = { ...delays.value, [nodeKey]: { value, error: null } }
    } catch (cause) {
      delays.value = { ...delays.value, [nodeKey]: { value: null, error: describeError(cause) } }
    }
  }
}

function latencyTone(value: number | null | undefined): string {
  if (value === null || value === undefined) return 'latency-unknown'
  if (value < 150) return 'latency-good'
  if (value < 400) return 'latency-mid'
  return 'latency-bad'
}

/// Only sing-box can be asked, and only while it runs.
const canMeasure = computed(() => engine.running && engine.kind === 'sing-box')

/// Three different reasons the button can be greyed out, and they are not the
/// same problem: a stopped kernel only needs starting, the wrong kernel needs
/// switching. Saying "sing-box must run" while sing-box is already selected
/// sends the reader to the wrong switch.
const measureHint = computed(() => {
  if (canMeasure.value) return t('groups.delay.hint')
  if (!engine.running) return t('groups.delay.noKernel')
  return t('groups.delay.needsSingBox')
})

async function confirmDelete() {
  const target = pendingDelete.value
  if (!target) return
  busy.value = true
  try {
    await store.remove(target.id)
    markStale()
    pendingDelete.value = null
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <section class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.groups.title') }}</h1>
        <p class="page-subtitle">{{ t('page.groups.subtitle') }}</p>
      </div>
      <div class="row">
        <span class="muted">{{ enabledCount }} / {{ store.items.length }}</span>
        <button class="btn" type="button" :disabled="store.loading" @click="store.load(true)">
          <AppIcon name="refresh" :size="16" />
          {{ store.loading ? t('common.refreshing') : t('common.refresh') }}
        </button>
        <button class="btn btn-primary" type="button" @click="openNew">
          <AppIcon name="plus" :size="16" />
          {{ t('groups.new') }}
        </button>
      </div>
    </header>

    <p v-if="xraySelected" class="alert alert-info">{{ t('groups.xrayHint') }}</p>
    <p v-if="actionError" class="alert">{{ actionError }}</p>
    <p v-if="store.error" class="alert">{{ store.error }}</p>

    <StaleConfig :show="stale" @applied="stale = false" />

    <EmptyState
      v-if="isEmpty"
      icon="layers"
      :title="t('groups.empty.title')"
      :body="t('groups.empty.body')"
    >
      <template #actions>
        <button class="btn btn-primary" type="button" @click="openNew">{{ t('groups.new') }}</button>
      </template>
    </EmptyState>

    <div v-else-if="store.items.length > 0" class="surface-plain">
      <!-- Xray has no selector to rewrite, so fallback and load-balance stay
           whichever single member they were rendered with. -->
      <p v-if="xraySelected && hasAppSwitchedGroup" class="muted small">
        {{ t('groups.xrayStatic') }}
      </p>
      <div class="table-wrap">
        <table class="data-table">
        <thead>
          <tr>
            <th scope="col">{{ t('groups.col.name') }}</th>
            <th scope="col">{{ t('groups.col.kind') }}</th>
            <th scope="col">{{ t('groups.col.members') }}</th>
            <th scope="col">{{ t('groups.col.current') }}</th>
            <th scope="col">{{ t('groups.col.delay') }}</th>
            <th scope="col">{{ t('groups.col.status') }}</th>
            <th scope="col" class="actions">{{ t('groups.col.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="group in store.items" :key="group.id" :class="{ dimmed: !group.enabled }">
            <td>{{ group.name }}</td>
            <td>
              <span class="badge">{{ t(KIND_LABEL[group.kind]) }}</span>
            </td>
            <td>
              <div class="chips">
                <!-- A url-test group is never picked by hand, so its members are
                     plain text: a disabled button says "not right now", which
                     sends people looking for the switch that enables it. -->
                <template v-if="group.kind !== 'select'">
                  <span
                    v-for="member in group.members"
                    :key="member"
                    class="chip chip-static"
                    :title="chipTitle(group)"
                  >
                    {{ nodeName(member) }}
                  </span>
                </template>
                <template v-else>
                  <button
                    v-for="member in group.members"
                    :key="member"
                    type="button"
                    class="chip"
                    :class="{ 'chip-active': isChosen(group, member) }"
                    :title="t('groups.pick')"
                    @click="pick(group, member)"
                  >
                    {{ nodeName(member) }}
                  </button>
                </template>
              </div>
            </td>
            <td>
              <span v-if="group.kind === 'select'" class="mono">{{ chosenName(group) }}</span>
              <span v-else class="muted">{{ t('groups.autoHint') }}</span>
            </td>
            <td>
              <span
                v-if="delayOf('group', group.id)?.error"
                class="latency latency-unknown"
                :title="delayOf('group', group.id)?.error ?? ''"
              >
                {{ t('groups.delay.failed') }}
              </span>
              <span
                v-else-if="delayOf('group', group.id)?.value !== undefined && delayOf('group', group.id)?.value !== null"
                class="latency"
                :class="latencyTone(delayOf('group', group.id)?.value)"
              >
                {{ t('nodes.latency.ms', { value: delayOf('group', group.id)?.value ?? 0 }) }}
              </span>
              <span v-else class="latency latency-unknown">{{ t('nodes.latency.none') }}</span>
              <div
                v-if="appSwitched(group.kind) || group.kind === 'url-test'"
                class="muted small member-delays"
              >
                <span
                  v-for="member in group.members"
                  :key="member"
                  class="member-delay"
                  :class="latencyTone(delayOf('node', member)?.value)"
                  :title="delayOf('node', member)?.error ?? nodeName(member)"
                >
                  {{
                    delayOf('node', member)?.value !== null && delayOf('node', member)?.value !== undefined
                      ? `${nodeName(member)} ${delayOf('node', member)?.value}ms`
                      : nodeName(member)
                  }}
                </span>
              </div>
            </td>
            <td>
              <span class="badge" :class="group.enabled ? 'badge-success' : ''">
                {{ group.enabled ? t('common.enabled') : t('common.disabled') }}
              </span>
            </td>
            <td class="actions">
              <button
                class="btn btn-sm btn-ghost"
                type="button"
                :disabled="!canMeasure || isMeasuring(group.id)"
                :title="measureHint"
                @click="measureGroup(group)"
              >
                <AppIcon name="refresh" :size="14" />
                {{ isMeasuring(group.id) ? t('common.measuring') : t('groups.delay.measure') }}
              </button>
              <button class="btn btn-sm btn-ghost" type="button" @click="toggle(group)">
                {{ group.enabled ? t('common.disable') : t('common.enable') }}
              </button>
              <button class="btn btn-sm btn-ghost" type="button" @click="openEdit(group)">
                <AppIcon name="edit" :size="14" />
                {{ t('common.edit') }}
              </button>
              <button class="btn btn-sm btn-ghost" type="button" @click="pendingDelete = group">
                <AppIcon name="trash" :size="14" />
                {{ t('common.delete') }}
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      </div>
    </div>

    <GroupEditor
      :open="editorOpen"
      :group="editing"
      @close="editorOpen = false"
      @submit="onSubmit"
    />

    <ModalDialog
      :open="pendingDelete !== null"
      :title="t('groups.delete.title')"
      :width="440"
      @close="pendingDelete = null"
    >
      <p class="muted">{{ t('groups.delete.body', { name: pendingDelete?.name ?? '' }) }}</p>
      <template #footer>
        <button class="btn" type="button" @click="pendingDelete = null">{{ t('common.cancel') }}</button>
        <button class="btn btn-danger" type="button" :disabled="busy" @click="confirmDelete">
          {{ busy ? t('common.deleting') : t('common.delete') }}
        </button>
      </template>
    </ModalDialog>
  </section>
</template>

<style scoped>
.dimmed {
  opacity: 0.55;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.chip {
  padding: 2px 8px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-pill);
  background: var(--surface-sunken);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  transition:
    border-color var(--duration-fast) var(--ease-out),
    color var(--duration-fast) var(--ease-out);
}

.chip:disabled {
  cursor: default;
}

/* Read-only member of a url-test group: still a chip so the column lines up,
   but nothing about it suggests it can be pressed. */
.chip-static {
  cursor: default;
  border-style: dashed;
}

.chip:hover:not(:disabled) {
  border-color: var(--accent);
  color: var(--text-primary);
}

.chip-active {
  border-color: var(--accent);
  background: var(--accent-soft);
  color: var(--accent);
}

/* Same tones as the node cards: the number means the same thing on both pages. */
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

.member-delays {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 4px;
  font-size: 11px;
}

.member-delay {
  color: var(--text-tertiary);
  white-space: nowrap;
}

.member-delay.latency-good {
  color: var(--success);
}

.member-delay.latency-mid {
  color: var(--warning);
}

.member-delay.latency-bad {
  color: var(--danger);
}
</style>
