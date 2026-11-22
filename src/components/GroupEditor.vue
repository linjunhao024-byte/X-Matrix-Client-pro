<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import ModalDialog from './ModalDialog.vue'
import ToggleSwitch from './ToggleSwitch.vue'
import { t } from '../i18n'
import { useNodesStore } from '../stores/nodes'
import type { GroupKind, ProxyGroup_Deserialize, ProxyGroup_Serialize } from '../services/contract'

const props = defineProps<{ open: boolean; group: ProxyGroup_Serialize | null }>()
const emit = defineEmits<{ close: []; submit: [ProxyGroup_Deserialize] }>()

const nodes = useNodesStore()

/**
 * Only the two kinds the kernels actually implement. Clash has a third
 * (`fallback`) that neither Xray nor sing-box can express natively, so
 * offering it would be a switch that silently does nothing.
 */
const kinds: { value: GroupKind; labelKey: string; hintKey: string }[] = [
  { value: 'select', labelKey: 'groups.kind.select', hintKey: 'groups.kind.select.hint' },
  { value: 'url-test', labelKey: 'groups.kind.urlTest', hintKey: 'groups.kind.urlTest.hint' },
]

const draft = reactive({
  name: '',
  kind: 'select' as GroupKind,
  members: [] as string[],
  enabled: true,
})

const error = ref<string | null>(null)

watch(
  () => props.open,
  async (open) => {
    if (!open) return
    error.value = null
    await nodes.load()
    const source = props.group
    if (source) {
      draft.name = source.name
      draft.kind = source.kind
      draft.members = [...source.members]
      draft.enabled = source.enabled
    } else {
      draft.name = ''
      draft.kind = 'select'
      draft.members = []
      draft.enabled = true
    }
  },
)

const available = computed(() => nodes.items)

function toggleMember(id: string) {
  const index = draft.members.indexOf(id)
  if (index >= 0) draft.members.splice(index, 1)
  else draft.members.push(id)
}

function submit() {
  error.value = null
  if (!draft.name.trim()) {
    error.value = t('groups.editor.err.name')
    return
  }
  if (draft.members.length === 0) {
    error.value = t('groups.editor.err.members')
    return
  }
  emit('submit', {
    id: props.group?.id ?? crypto.randomUUID(),
    name: draft.name.trim(),
    enabled: draft.enabled,
    kind: draft.kind,
    members: [...draft.members],
    // Keep the previous pick if it is still a member, otherwise let the
    // backend fall back to the first member.
    selected: props.group?.selected && draft.members.includes(props.group.selected)
      ? props.group.selected
      : null,
  })
}
</script>

<template>
  <ModalDialog
    :open="open"
    :title="group ? t('groups.editor.edit') : t('groups.editor.new')"
    :width="560"
    :close-on-overlay="false"
    @close="emit('close')"
  >
    <div class="field">
      <label class="field-label" for="group-name">{{ t('groups.editor.name') }}</label>
      <input
        id="group-name"
        v-model="draft.name"
        class="input"
        :placeholder="t('groups.editor.namePlaceholder')"
      />
    </div>

    <div class="field">
      <label class="field-label" for="group-kind">{{ t('groups.editor.kind') }}</label>
      <select id="group-kind" v-model="draft.kind" class="select">
        <option v-for="item in kinds" :key="item.value" :value="item.value">
          {{ t(item.labelKey) }}
        </option>
      </select>
      <p class="field-hint">
        {{ t(kinds.find((item) => item.value === draft.kind)?.hintKey ?? 'groups.kind.select.hint') }}
      </p>
    </div>

    <div class="field">
      <label class="field-label">{{ t('groups.editor.members') }}</label>
      <p v-if="available.length === 0" class="field-hint">{{ t('groups.editor.noNodes') }}</p>
      <div v-else class="member-list">
        <label v-for="node in available" :key="node.id" class="member-row">
          <input
            type="checkbox"
            :checked="draft.members.includes(node.id)"
            @change="toggleMember(node.id)"
          />
          <span class="member-name">{{ node.name }}</span>
          <span class="member-addr mono">{{ node.server }}:{{ node.port }}</span>
        </label>
      </div>
      <p class="field-hint">{{ t('groups.editor.membersHint') }}</p>
    </div>

    <ToggleSwitch v-model="draft.enabled" :label="t('common.enabled')" />

    <p v-if="error" class="alert">{{ error }}</p>

    <template #footer>
      <button class="btn" type="button" @click="emit('close')">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary" type="button" @click="submit">{{ t('common.save') }}</button>
    </template>
  </ModalDialog>
</template>

<style scoped>
.member-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 220px;
  overflow-y: auto;
  padding: 6px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-control);
  background: var(--surface-sunken);
}

.member-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: var(--radius-control);
  font-size: 13px;
  cursor: pointer;
}

.member-row:hover {
  background: var(--surface-hover);
}

.member-name {
  flex: 1;
  color: var(--text-primary);
}

.member-addr {
  font-size: 12px;
  color: var(--text-tertiary);
}
</style>
