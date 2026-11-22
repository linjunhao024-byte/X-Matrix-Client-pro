<script setup lang="ts">
import { ref, watch } from 'vue'
import AppIcon from './AppIcon.vue'
import ModalDialog from './ModalDialog.vue'
import { t } from '../i18n'
import { api, describeError, type ImportOutcome_Serialize, type ImportPreview_Serialize } from '../services/contract'
import { useNodesStore } from '../stores/nodes'

const props = defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: []; imported: [ImportOutcome_Serialize] }>()

const store = useNodesStore()

const text = ref('')
const preview = ref<ImportPreview_Serialize | null>(null)
const error = ref<string | null>(null)
const parsing = ref(false)
const applying = ref(false)

watch(
  () => props.open,
  (open) => {
    if (!open) return
    text.value = ''
    preview.value = null
    error.value = null
    parsing.value = false
    applying.value = false
  },
)

/**
 * Read a file instead of pasting.
 *
 * Subscription providers hand out a link, but every tutorial and every other
 * client also accepts "here is a .txt on my disk", and that is the path people
 * take when the provider's link is behind a login.
 */
async function loadFromFile() {
  error.value = null
  try {
    const text_ = await api.openTextFile()
    if (text_ === null) return
    text.value = text_
    await parse()
  } catch (cause) {
    error.value = describeError(cause)
  }
}

async function parse() {
  parsing.value = true
  error.value = null
  try {
    preview.value = await api.importPreview(text.value)
    if (preview.value.nodes.length === 0 && preview.value.skipped.length === 0) {
      error.value = t('import.err.none')
    }
  } catch (cause) {
    error.value = describeError(cause)
    preview.value = null
  } finally {
    parsing.value = false
  }
}

async function apply() {
  applying.value = true
  error.value = null
  try {
    const outcome = await store.importFromText(text.value)
    emit('imported', outcome)
    emit('close')
  } catch (cause) {
    error.value = describeError(cause)
  } finally {
    applying.value = false
  }
}

const canApply = () => (preview.value?.nodes.length ?? 0) > 0 && !applying.value
</script>

<template>
  <ModalDialog
    :open="open"
    :title="t('import.title')"
    :width="620"
    :close-on-overlay="false"
    @close="emit('close')"
  >
    <p class="muted">
      {{ t('import.body') }}
      {{ t('import.hint') }}
    </p>

    <div class="field">
      <div class="row import-label-row">
        <label class="field-label" for="import-text">{{ t('import.label') }}</label>
        <button class="btn btn-sm" type="button" @click="loadFromFile">
          <AppIcon name="file" :size="14" />
          {{ t('import.fromFile') }}
        </button>
      </div>
      <textarea
        id="import-text"
        v-model="text"
        class="input input-mono import-area"
        rows="8"
        spellcheck="false"
        :placeholder="t('import.placeholder')"
      />
    </div>

    <p v-if="error" class="alert">{{ error }}</p>

    <div v-if="preview" class="preview">
      <div class="row preview-head">
        <span class="badge badge-success">{{ t('import.found', { count: preview.nodes.length }) }}</span>
        <span v-if="preview.skipped.length" class="badge badge-warning">
          {{ t('import.skipped', { count: preview.skipped.length }) }}
        </span>
        <span class="spacer" />
        <button class="btn btn-sm" type="button" :disabled="parsing" @click="parse">
          <AppIcon name="refresh" :size="14" />
          {{ t('import.reparse') }}
        </button>
      </div>

      <ul v-if="preview.nodes.length" class="preview-list">
        <li v-for="node in preview.nodes.slice(0, 8)" :key="node.id">
          <span class="preview-name">{{ node.name }}</span>
          <span class="cell-mono muted">{{ node.server }}:{{ node.port }}</span>
          <span class="badge badge-accent">{{ node.protocol.kind }}</span>
        </li>
        <li v-if="preview.nodes.length > 8" class="muted">
          {{ t('import.andMore', { count: preview.nodes.length - 8 }) }}
        </li>
      </ul>

      <ul v-if="preview.skipped.length" class="preview-skipped">
        <li v-for="line in preview.skipped.slice(0, 5)" :key="line">{{ line }}</li>
        <li v-if="preview.skipped.length > 5" class="muted">
          {{ t('import.moreLines', { count: preview.skipped.length - 5 }) }}
        </li>
      </ul>
    </div>

    <template #footer>
      <button class="btn" type="button" @click="emit('close')">{{ t('common.cancel') }}</button>
      <button class="btn" type="button" :disabled="parsing || !text.trim()" @click="parse">
        {{ parsing ? t('import.parsing') : t('import.preview') }}
      </button>
      <button class="btn btn-primary" type="button" :disabled="!canApply()" @click="apply">
        {{ applying ? t('import.applying') : t('import.apply') }}
      </button>
    </template>
  </ModalDialog>
</template>

<style scoped>
.import-area {
  resize: vertical;
  min-height: 150px;
  line-height: 1.6;
}

.preview {
  margin-top: 14px;
  padding: 12px 14px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-card);
  background: var(--surface-sunken);
}

.preview-head {
  margin-bottom: 10px;
}

.preview-list,
.preview-skipped {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}

.preview-list li {
  display: flex;
  align-items: center;
  gap: 10px;
}

.preview-name {
  min-width: 120px;
}

.preview-skipped {
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px dashed var(--border-subtle);
  color: var(--warning);
}
</style>
