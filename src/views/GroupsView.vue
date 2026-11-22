<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import GroupEditor from '../components/GroupEditor.vue'
import ModalDialog from '../components/ModalDialog.vue'
import { t } from '../i18n'
import { useEngineStore } from '../stores/engine'
import { useGroupsStore } from '../stores/groups'
import { useNodesStore } from '../stores/nodes'
import { describeError, type ProxyGroup_Deserialize, type ProxyGroup_Serialize } from '../services/contract'

const store = useGroupsStore()
const nodes = useNodesStore()
const engine = useEngineStore()

const editorOpen = ref(false)
const editing = ref<ProxyGroup_Serialize | null>(null)
const pendingDelete = ref<ProxyGroup_Serialize | null>(null)
const busy = ref(false)
const actionError = ref<string | null>(null)

onMounted(async () => {
  await Promise.all([store.load(), nodes.load(), engine.refreshBinaries()])
})

const isEmpty = computed(() => store.loaded && store.items.length === 0)
const enabledCount = computed(() => store.items.filter((group) => group.enabled).length)
const xraySelected = computed(() => engine.kind === 'xray')

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
  } catch (cause) {
    actionError.value = describeError(cause)
  }
}

async function toggle(group: ProxyGroup_Serialize) {
  try {
    await store.toggle(group.id)
  } catch (cause) {
    actionError.value = describeError(cause)
  }
}

async function confirmDelete() {
  const target = pendingDelete.value
  if (!target) return
  busy.value = true
  try {
    await store.remove(target.id)
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

    <EmptyState
      v-if="isEmpty"
      icon="layers"
      :title="t('groups.empty.title')"
      :body="t('groups.empty.body')"
    >
      <button class="btn btn-primary" type="button" @click="openNew">{{ t('groups.new') }}</button>
    </EmptyState>

    <div v-else-if="store.items.length > 0" class="surface-plain">
      <table class="data-table">
        <thead>
          <tr>
            <th scope="col">{{ t('groups.col.name') }}</th>
            <th scope="col">{{ t('groups.col.kind') }}</th>
            <th scope="col">{{ t('groups.col.members') }}</th>
            <th scope="col">{{ t('groups.col.current') }}</th>
            <th scope="col">{{ t('groups.col.status') }}</th>
            <th scope="col" class="col-actions">{{ t('groups.col.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="group in store.items" :key="group.id" :class="{ dimmed: !group.enabled }">
            <td>{{ group.name }}</td>
            <td>
              <span class="badge">{{
                t(group.kind === 'select' ? 'groups.kind.select' : 'groups.kind.urlTest')
              }}</span>
            </td>
            <td>
              <div class="chips">
                <button
                  v-for="member in group.members"
                  :key="member"
                  type="button"
                  class="chip"
                  :class="{ 'chip-active': group.kind === 'select' && isChosen(group, member) }"
                  :disabled="group.kind !== 'select'"
                  :title="group.kind === 'select' ? t('groups.pick') : t('groups.autoHint')"
                  @click="pick(group, member)"
                >
                  {{ nodeName(member) }}
                </button>
              </div>
            </td>
            <td>
              <span v-if="group.kind === 'select'" class="mono">{{ chosenName(group) }}</span>
              <span v-else class="muted">{{ t('groups.autoHint') }}</span>
            </td>
            <td>
              <span class="badge" :class="group.enabled ? 'badge-success' : ''">
                {{ group.enabled ? t('common.enabled') : t('common.disabled') }}
              </span>
            </td>
            <td class="col-actions">
              <button class="btn btn-sm btn-ghost" type="button" @click="toggle(group)">
                {{ group.enabled ? t('common.disabled') : t('common.enabled') }}
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

.col-actions {
  text-align: right;
  white-space: nowrap;
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

.chip:hover:not(:disabled) {
  border-color: var(--accent);
  color: var(--text-primary);
}

.chip-active {
  border-color: var(--accent);
  background: var(--accent-soft);
  color: var(--accent);
}
</style>
