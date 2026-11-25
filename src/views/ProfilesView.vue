<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import ModalDialog from '../components/ModalDialog.vue'
import { t } from '../i18n'
import { useProfilesStore } from '../stores/profiles'
import { describeError, type ProfileView, type SwitchReport_Serialize } from '../services/contract'

const store = useProfilesStore()

/// Which row a dialog is about. `null` closes it.
const pendingDelete = ref<ProfileView | null>(null)
const pendingRename = ref<ProfileView | null>(null)
const createOpen = ref(false)

const name = ref('')
const copyCurrent = ref(true)
const formError = ref<string | null>(null)

const busy = ref(false)
const busyId = ref<string | null>(null)
const actionError = ref<string | null>(null)
/// The sentence shown after a switch. The backend's own note wins: it knows
/// whether the kernel was restarted, left stopped, or why the restart failed.
const result = ref<string | null>(null)

/// The overlay editor is inline, under its own row, and only one row is open
/// at a time: a second textarea would push the table around for no reason.
const mergeOpenId = ref<string | null>(null)
const mergeText = ref('')
const mergeError = ref<string | null>(null)
/// Shown only after the backend accepted the text; the overlay takes effect on
/// the next kernel start, so there is nothing else to report here.
const mergeSaved = ref<string | null>(null)
const mergeBusy = ref(false)

onMounted(async () => {
  await store.load()
})

const active = computed(() => store.items.find((profile) => profile.active) ?? null)
const isEmpty = computed(() => store.loaded && store.items.length === 0)

/// A switch says three different things and the UI must not flatten them into
/// "done": the kernel may have restarted, may have stayed stopped because the
/// new profile has no node, or may have failed to start with the backend's own
/// reason. `note` covers the last two, verbatim.
function describeSwitch(report: SwitchReport_Serialize): string {
  if (report.note) return report.note
  if (report.restarted) return t('profiles.restarted', { name: report.active.name })
  if (report.stopped) return t('profiles.stoppedNoNode')
  return t('profiles.switched', { name: report.active.name })
}

function openCreate() {
  name.value = ''
  copyCurrent.value = true
  formError.value = null
  actionError.value = null
  createOpen.value = true
}

function openRename(profile: ProfileView) {
  name.value = profile.name
  formError.value = null
  actionError.value = null
  pendingRename.value = profile
}

function checked(value: string): string | null {
  return value.trim() === '' ? t('profiles.nameEmpty') : null
}

async function submitCreate() {
  const invalid = checked(name.value)
  formError.value = invalid
  if (invalid) return
  busy.value = true
  try {
    await store.create(name.value.trim(), copyCurrent.value ? (active.value?.id ?? null) : null)
    createOpen.value = false
  } catch (cause) {
    formError.value = describeError(cause)
  } finally {
    busy.value = false
  }
}

async function submitRename() {
  const target = pendingRename.value
  if (!target) return
  const invalid = checked(name.value)
  formError.value = invalid
  if (invalid) return
  busy.value = true
  try {
    await store.rename(target.id, name.value.trim())
    pendingRename.value = null
  } catch (cause) {
    formError.value = describeError(cause)
  } finally {
    busy.value = false
  }
}

async function use(profile: ProfileView) {
  busyId.value = profile.id
  actionError.value = null
  result.value = null
  try {
    result.value = describeSwitch(await store.activate(profile.id))
    // The overlay follows the profile that owns it, so an editor left open on
    // the row that just became inactive would show the wrong text.
    mergeOpenId.value = null
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    busyId.value = null
  }
}

function toggleMerge(profile: ProfileView) {
  actionError.value = null
  result.value = null
  mergeError.value = null
  mergeSaved.value = null
  if (mergeOpenId.value === profile.id) {
    mergeOpenId.value = null
    return
  }
  mergeOpenId.value = profile.id
  // The list carries the whole text, so the editor opens with what is stored.
  mergeText.value = profile.merge
}

async function submitMerge() {
  const id = mergeOpenId.value
  if (!id) return
  mergeBusy.value = true
  mergeError.value = null
  mergeSaved.value = null
  try {
    const updated = await store.setMerge(id, mergeText.value)
    mergeText.value = updated.merge
    mergeSaved.value = t('profiles.mergeSaved')
  } catch (cause) {
    // The backend names what it refused (not YAML, or not a mapping) in its own
    // words; replacing that with a generic sentence would lose the reason.
    mergeError.value = describeError(cause)
  } finally {
    mergeBusy.value = false
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
        <h1 class="page-title">{{ t('page.profiles.title') }}</h1>
        <p class="page-subtitle">{{ t('page.profiles.subtitle') }}</p>
      </div>
      <div class="row">
        <button class="btn" type="button" :disabled="store.loading" @click="store.load(true)">
          <AppIcon name="refresh" :size="16" />
          {{ store.loading ? t('common.refreshing') : t('common.refresh') }}
        </button>
        <button class="btn btn-primary" type="button" @click="openCreate">
          <AppIcon name="plus" :size="16" />
          {{ t('profiles.new') }}
        </button>
      </div>
    </header>

    <div class="notice">
      <p>{{ t('profiles.hint') }}</p>
      <p class="muted small">{{ t('profiles.globalNote') }}</p>
    </div>

    <p v-if="result" class="alert alert-info">{{ result }}</p>
    <p v-if="actionError" class="alert">{{ actionError }}</p>
    <p v-if="store.error" class="alert">{{ store.error }}</p>

    <EmptyState
      v-if="isEmpty"
      icon="copy"
      :title="t('profiles.empty')"
      :body="t('profiles.hint')"
    >
      <template #actions>
        <button class="btn btn-primary" type="button" @click="openCreate">
          {{ t('profiles.new') }}
        </button>
      </template>
    </EmptyState>

    <div v-else-if="store.items.length > 0" class="surface-plain">
      <div class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th scope="col">{{ t('profiles.col.name') }}</th>
              <th scope="col">{{ t('profiles.col.nodes') }}</th>
              <th scope="col" class="actions">{{ t('profiles.col.actions') }}</th>
            </tr>
          </thead>
          <tbody>
            <template v-for="profile in store.items" :key="profile.id">
              <tr>
                <td>
                  <div class="name-cell">
                    <span>{{ profile.name }}</span>
                    <span v-if="profile.active" class="badge badge-success">
                      {{ t('profiles.active') }}
                    </span>
                  </div>
                </td>
                <td class="mono">{{ profile.nodes ?? 0 }}</td>
                <td class="actions">
                  <button
                    class="btn btn-sm btn-primary"
                    type="button"
                    :disabled="profile.active || busyId !== null"
                    @click="use(profile)"
                  >
                    <AppIcon name="check" :size="14" />
                    {{ t('profiles.use') }}
                  </button>
                  <button class="btn btn-sm btn-ghost" type="button" @click="openRename(profile)">
                    <AppIcon name="edit" :size="14" />
                    {{ t('profiles.rename') }}
                  </button>
                  <button
                    class="btn btn-sm btn-ghost"
                    type="button"
                    :aria-expanded="mergeOpenId === profile.id"
                    @click="toggleMerge(profile)"
                  >
                    <AppIcon name="layers" :size="14" />
                    {{ t('profiles.merge') }}
                  </button>
                  <button
                    class="btn btn-sm btn-ghost"
                    type="button"
                    :disabled="profile.active"
                    @click="pendingDelete = profile"
                  >
                    <AppIcon name="trash" :size="14" />
                    {{ t('profiles.delete') }}
                  </button>
                </td>
              </tr>
              <tr v-if="mergeOpenId === profile.id" class="merge-row">
                <td colspan="3">
                  <div class="merge-editor">
                    <label class="field">
                      <span class="field-label">{{ t('profiles.merge') }}</span>
                      <textarea
                        v-model="mergeText"
                        class="input input-mono merge-input"
                        rows="6"
                        spellcheck="false"
                        :placeholder="t('profiles.mergePlaceholder')"
                      ></textarea>
                    </label>
                    <p class="field-hint">{{ t('profiles.mergeHint') }}</p>
                    <div class="row merge-actions">
                      <button
                        class="btn btn-sm btn-primary"
                        type="button"
                        :disabled="mergeBusy"
                        @click="submitMerge"
                      >
                        {{ mergeBusy ? t('common.saving') : t('profiles.mergeSave') }}
                      </button>
                      <span v-if="mergeSaved" class="merge-saved">{{ mergeSaved }}</span>
                    </div>
                    <p v-if="mergeError" class="field-error">{{ mergeError }}</p>
                  </div>
                </td>
              </tr>
            </template>
          </tbody>
        </table>
      </div>
    </div>

    <ModalDialog :open="createOpen" :title="t('profiles.new')" :width="440" @close="createOpen = false">
      <label class="field">
        <span class="field-label">{{ t('profiles.col.name') }}</span>
        <input
          v-model="name"
          class="input"
          type="text"
          maxlength="40"
          :placeholder="t('profiles.namePlaceholder')"
        />
      </label>
      <label class="row option">
        <input v-model="copyCurrent" type="checkbox" />
        <span>{{ t('profiles.copyCurrent') }}</span>
      </label>
      <p v-if="formError" class="field-error">{{ formError }}</p>
      <template #footer>
        <button class="btn" type="button" @click="createOpen = false">{{ t('common.cancel') }}</button>
        <button class="btn btn-primary" type="button" :disabled="busy" @click="submitCreate">
          {{ busy ? t('common.saving') : t('common.confirm') }}
        </button>
      </template>
    </ModalDialog>

    <ModalDialog
      :open="pendingRename !== null"
      :title="t('profiles.rename')"
      :width="440"
      @close="pendingRename = null"
    >
      <label class="field">
        <span class="field-label">{{ t('profiles.col.name') }}</span>
        <input
          v-model="name"
          class="input"
          type="text"
          maxlength="40"
          :placeholder="t('profiles.namePlaceholder')"
        />
      </label>
      <p v-if="formError" class="field-error">{{ formError }}</p>
      <template #footer>
        <button class="btn" type="button" @click="pendingRename = null">
          {{ t('common.cancel') }}
        </button>
        <button class="btn btn-primary" type="button" :disabled="busy" @click="submitRename">
          {{ busy ? t('common.saving') : t('common.confirm') }}
        </button>
      </template>
    </ModalDialog>

    <ModalDialog
      :open="pendingDelete !== null"
      :title="t('profiles.delete')"
      :width="460"
      @close="pendingDelete = null"
    >
      <p class="mono">{{ pendingDelete?.name }}</p>
      <p class="muted">{{ t('profiles.deleteConfirm') }}</p>
      <template #footer>
        <button class="btn" type="button" @click="pendingDelete = null">
          {{ t('common.cancel') }}
        </button>
        <button class="btn btn-danger" type="button" :disabled="busy" @click="confirmDelete">
          {{ busy ? t('common.deleting') : t('profiles.delete') }}
        </button>
      </template>
    </ModalDialog>
  </section>
</template>

<style scoped>
/* Name and the "in use" badge share one line: the badge belongs to the name,
   not to a column of its own. */
.name-cell {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

/* The two-line explanation above the table. Not `.alert`: nothing went wrong,
   this is how the page works. */
.notice p {
  margin: 0;
}

.notice p + p {
  margin-top: 4px;
}

.option {
  gap: 8px;
  margin-top: 12px;
}

.option input {
  accent-color: var(--accent);
}

/* The overlay editor sits under the row it belongs to: a dialog would cover the
   row, and the overlay is a page of YAML, not a one-line answer. */
.merge-row > td {
  background: var(--surface-sunken);
}

.merge-editor {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 2px 0 6px;
}

/* `.input` is a 34px control; a YAML box has to grow with its content and the
   user drags it taller when the overlay gets long. */
.merge-input {
  height: auto;
  min-height: 120px;
  resize: vertical;
  line-height: 1.55;
}

.merge-actions {
  align-items: center;
  gap: 10px;
}

.merge-saved {
  font-size: 12px;
  color: var(--text-secondary);
}
</style>