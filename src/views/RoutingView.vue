<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import AppIcon from '../components/AppIcon.vue'
import EmptyState from '../components/EmptyState.vue'
import ModalDialog from '../components/ModalDialog.vue'
import RuleEditor from '../components/RuleEditor.vue'
import { t } from '../i18n'
import { api, describeError, type RuleAction, type RoutingRule, type RuleTest_Serialize } from '../services/contract'
import { useGroupsStore } from '../stores/groups'
import { useRoutingStore } from '../stores/routing'
import { useRuleSetsStore } from '../stores/rulesets'

const store = useRoutingStore()
const groups = useGroupsStore()
const ruleSets = useRuleSetsStore()

const editorOpen = ref(false)
const editing = ref<RoutingRule | null>(null)
const pendingDelete = ref<RoutingRule | null>(null)
const actionError = ref<string | null>(null)
const busy = ref(false)

const testOpen = ref(false)
const testDomainInput = ref('')
const testResult = ref<RuleTest_Serialize | null>(null)
const testError = ref<string | null>(null)
const testing = ref(false)

function openTest() {
  testResult.value = null
  testError.value = null
  testOpen.value = true
}

/**
 * Dry run the table against one hostname.
 *
 * The kernel evaluates top to bottom and stops at the first hit, so the answer
 * that matters is the first match — showing every rule that would match would
 * mislead. `skipped` counts geoip / port / process rules, which cannot be
 * judged from a name; without that number "no rule matched" would be a lie.
 */
async function runTest() {
  const domain = testDomainInput.value.trim()
  if (!domain) return
  testing.value = true
  testError.value = null
  try {
    testResult.value = await api.testDomain(domain)
  } catch (cause) {
    testResult.value = null
    testError.value = describeError(cause)
  } finally {
    testing.value = false
  }
}

const isEmpty = computed(() => store.loaded && store.items.length === 0)
const enabledCount = computed(() => store.items.filter((rule) => rule.enabled).length)

onMounted(async () => {
  await Promise.all([store.load(), groups.load(), ruleSets.load()])
})

function openCreate() {
  editing.value = null
  editorOpen.value = true
}

function openEdit(rule: RoutingRule) {
  editing.value = rule
  editorOpen.value = true
}

async function submit(rule: RoutingRule) {
  busy.value = true
  actionError.value = null
  try {
    await store.save(rule)
    editorOpen.value = false
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    busy.value = false
  }
}

async function confirmDelete() {
  const target = pendingDelete.value
  if (!target) return
  busy.value = true
  actionError.value = null
  try {
    await store.remove(target.id)
    pendingDelete.value = null
  } catch (cause) {
    actionError.value = describeError(cause)
  } finally {
    busy.value = false
  }
}

async function move(rule: RoutingRule, delta: number) {
  actionError.value = null
  try {
    await store.move(rule.id, delta)
  } catch (cause) {
    actionError.value = describeError(cause)
  }
}

async function toggle(rule: RoutingRule) {
  actionError.value = null
  try {
    await store.toggle(rule.id)
  } catch (cause) {
    actionError.value = describeError(cause)
  }
}

const matcherKeys: Record<RoutingRule['matcher']['kind'], string> = {
  domain_suffix: 'match.domain_suffix',
  domain_keyword: 'match.domain_keyword',
  domain_full: 'match.domain_full',
  ip_cidr: 'match.ip_cidr',
  geo_ip: 'match.geo_ip',
  port: 'match.port',
  process_name: 'match.process_name',
  rule_set: 'match.rule_set',
}

function matcherLabel(rule: RoutingRule): string {
  return t(matcherKeys[rule.matcher.kind])
}

/**
 * A rule set is stored by id, so show its name; if the set is gone the id is
 * still printed rather than an empty cell, because that id is what the rule
 * is actually pointing at.
 */
function matcherValue(rule: RoutingRule): string {
  if (rule.matcher.kind === 'rule_set') {
    return ruleSets.byId(rule.matcher.value)?.name ?? rule.matcher.value
  }
  return String(rule.matcher.value ?? '')
}

/**
 * The action is a tagged union now (`{kind:'group', group_id}`), so a plain
 * string comparison no longer describes it. A group that has since been
 * deleted is named as a missing group rather than silently shown as "proxy".
 */
function actionLabel(rule: RoutingRule): string {
  const action = rule.action
  if (action.kind === 'proxy') return t('action.proxy')
  if (action.kind === 'direct') return t('action.direct')
  if (action.kind === 'block') return t('action.block')
  const group = groups.byId(action.group_id)
  return t('action.group.hint', { name: group?.name ?? action.group_id })
}

/// Same wording for the dry-run result, which has no rule object to pass.
function actionText(action: RuleAction | null): string {
  if (!action) return ''
  return actionLabel({ action } as RoutingRule)
}

function actionClass(rule: RoutingRule): string {
  const kind = rule.action.kind
  if (kind === 'direct') return 'badge badge-success'
  if (kind === 'block') return 'badge badge-danger'
  if (kind === 'group') return 'badge badge-warning'
  return 'badge badge-accent'
}
</script>

<template>
  <div class="page">
    <header class="page-header">
      <div>
        <h1 class="page-title">{{ t('page.routing.title') }}</h1>
        <p class="page-subtitle">
          {{
            t('routing.hint', {
              enabled: enabledCount,
              total: store.items.length,
            })
          }}
        </p>
      </div>
      <div class="row">
        <button class="btn" type="button" :disabled="store.loading" @click="store.load(true)">
          <AppIcon name="refresh" :size="15" />
          {{ t('common.refresh') }}
        </button>
        <button class="btn" type="button" @click="openTest">
          <AppIcon name="check" :size="15" />
          {{ t('routing.test') }}
        </button>
        <button class="btn btn-primary" type="button" @click="openCreate">
          <AppIcon name="plus" :size="15" />
          {{ t('routing.new') }}
        </button>
      </div>
    </header>

    <p v-if="store.error" class="alert">{{ store.error }}</p>
    <p v-if="actionError" class="alert">{{ actionError }}</p>

    <section class="surface-plain table-wrap">
      <div v-if="store.loading && !store.loaded" class="empty-state">
        <div class="empty-state-title">{{ t('common.loading') }}</div>
      </div>

      <EmptyState
        v-else-if="isEmpty"
        icon="routing"
        :title="t('routing.empty.title')"
        :body="t('routing.empty.body')"
      >
        <template #actions>
          <button class="btn btn-primary" type="button" @click="openCreate">{{ t('routing.new') }}</button>
        </template>
      </EmptyState>

      <table v-else class="data-table">
        <thead>
          <tr>
            <th scope="col" class="col-order">{{ t('routing.col.order') }}</th>
            <th scope="col">{{ t('routing.col.name') }}</th>
            <th scope="col">{{ t('routing.col.match') }}</th>
            <th scope="col">{{ t('routing.col.value') }}</th>
            <th scope="col">{{ t('routing.col.action') }}</th>
            <th scope="col">{{ t('routing.col.status') }}</th>
            <th scope="col" class="col-actions">{{ t('routing.col.actions') }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="(rule, index) in store.items" :key="rule.id" :class="{ disabled: !rule.enabled }">
            <td class="col-order muted">{{ index + 1 }}</td>
            <td>{{ rule.name }}</td>
            <td class="muted">{{ matcherLabel(rule) }}</td>
            <td class="cell-mono">{{ matcherValue(rule) }}</td>
            <td><span :class="actionClass(rule)">{{ actionLabel(rule) }}</span></td>
            <td>
              <button
                class="btn btn-ghost btn-sm"
                type="button"
                :title="rule.enabled ? t('routing.disable') : t('routing.enable')"
                @click="toggle(rule)"
              >
                {{ rule.enabled ? t('common.enabled') : t('common.disabled') }}
              </button>
            </td>
            <td class="col-actions">
              <div class="row">
                <button
                  class="btn btn-ghost btn-sm"
                  type="button"
                  :title="t('routing.moveUp')"
                  :disabled="index === 0"
                  @click="move(rule, -1)"
                >
                  <AppIcon name="arrow-up" :size="14" />
                </button>
                <button
                  class="btn btn-ghost btn-sm"
                  type="button"
                  :title="t('routing.moveDown')"
                  :disabled="index === store.items.length - 1"
                  @click="move(rule, 1)"
                >
                  <AppIcon name="arrow-down" :size="14" />
                </button>
                <button class="btn btn-ghost btn-sm" type="button" @click="openEdit(rule)">
                  <AppIcon name="edit" :size="14" />
                  {{ t('common.edit') }}
                </button>
                <button class="btn btn-danger btn-sm" type="button" @click="pendingDelete = rule">
                  <AppIcon name="trash" :size="14" />
                  {{ t('common.delete') }}
                </button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </section>

    <RuleEditor :open="editorOpen" :rule="editing" @close="editorOpen = false" @submit="submit" />

    <ModalDialog :open="testOpen" :title="t('routing.test.title')" :width="520" @close="testOpen = false">
      <p class="muted">{{ t('routing.test.body') }}</p>

      <div class="field">
        <label class="field-label" for="test-domain">{{ t('routing.test.label') }}</label>
        <div class="row">
          <input
            id="test-domain"
            v-model="testDomainInput"
            class="input input-mono"
            type="text"
            placeholder="www.example.com"
            spellcheck="false"
            @keyup.enter="runTest"
          />
          <button class="btn btn-primary" type="button" :disabled="testing || !testDomainInput.trim()" @click="runTest">
            {{ testing ? t('routing.test.running') : t('routing.test.run') }}
          </button>
        </div>
      </div>

      <p v-if="testError" class="alert">{{ testError }}</p>

      <div v-else-if="testResult" class="test-result">
        <template v-if="testResult.rule">
          <p class="test-verdict">
            {{ t('routing.test.hit', { rule: testResult.rule }) }}
          </p>
          <p class="muted">{{ t('routing.test.action', { action: actionText(testResult.action) }) }}</p>
        </template>
        <p v-else class="test-verdict">{{ t('routing.test.miss') }}</p>
        <p v-if="(testResult.skipped ?? 0) > 0" class="muted">
          {{ t('routing.test.skipped', { count: testResult.skipped ?? 0 }) }}
        </p>
      </div>

      <template #footer>
        <button class="btn" type="button" @click="testOpen = false">{{ t('common.close') }}</button>
      </template>
    </ModalDialog>

    <ModalDialog
      :open="pendingDelete !== null"
      :title="t('routing.delete.title')"
      :width="420"
      @close="pendingDelete = null"
    >
      <p class="muted">{{ t('routing.delete.body', { name: pendingDelete?.name ?? '' }) }}</p>
      <template #footer>
        <button class="btn" type="button" @click="pendingDelete = null">{{ t('common.cancel') }}</button>
        <button class="btn btn-primary" type="button" :disabled="busy" @click="confirmDelete">
          {{ busy ? t('common.deleting') : t('common.confirm') }}
        </button>
      </template>
    </ModalDialog>
  </div>
</template>

<style scoped>
.table-wrap {
  /* A narrow window must scroll a wide table instead of clipping its action
     column out of reach. */
  overflow-x: auto;
}

.test-result {
  padding: 10px 12px;
  border-radius: var(--radius-card);
  background: var(--surface-sunken);
}

.test-verdict {
  margin: 0 0 4px;
  font-weight: 600;
}

.col-order {
  width: 1%;
  white-space: nowrap;
}

.col-actions {
  width: 1%;
  white-space: nowrap;
}

tr.disabled td {
  opacity: 0.55;
}
</style>
