<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import ModalDialog from './ModalDialog.vue'
import ToggleSwitch from './ToggleSwitch.vue'
import { t } from '../i18n'
import { useGroupsStore } from '../stores/groups'
import { useRuleSetsStore } from '../stores/rulesets'
import type { RoutingRule, RuleAction, RuleMatcher } from '../services/contract'

const props = defineProps<{ open: boolean; rule: RoutingRule | null }>()
const emit = defineEmits<{ close: []; submit: [RoutingRule] }>()

const groups = useGroupsStore()
const ruleSets = useRuleSetsStore()

const matcherKinds: { value: RuleMatcher['kind']; labelKey: string; placeholder: string }[] = [
  { value: 'domain_suffix', labelKey: 'match.domain_suffix', placeholder: 'example.com' },
  { value: 'domain_keyword', labelKey: 'match.domain_keyword', placeholder: 'ads' },
  { value: 'domain_full', labelKey: 'match.domain_full', placeholder: 'www.example.com' },
  // Both are what Clash subscriptions keep reaching for and the kernels
  // support: a regex over the domain, and a named geosite list.
  { value: 'domain_regex', labelKey: 'match.domain_regex', placeholder: '^ads?\\d+\\.' },
  { value: 'geo_site', labelKey: 'match.geosite', placeholder: 'category-ads-all' },
  { value: 'ip_cidr', labelKey: 'match.ip_cidr', placeholder: '192.168.0.0/16' },
  { value: 'geo_ip', labelKey: 'match.geo_ip', placeholder: 'cn' },
  { value: 'port', labelKey: 'match.port', placeholder: '443' },
  { value: 'process_name', labelKey: 'match.process_name', placeholder: 'chrome.exe' },
  { value: 'rule_set', labelKey: 'match.rule_set', placeholder: '' },
]

type ActionKind = RuleAction['kind']

const actionKinds: { value: ActionKind; labelKey: string; hintKey: string }[] = [
  { value: 'proxy', labelKey: 'action.proxy', hintKey: 'action.proxy.hint' },
  { value: 'direct', labelKey: 'action.direct', hintKey: 'action.direct.hint' },
  { value: 'block', labelKey: 'action.block', hintKey: 'action.block.hint' },
  { value: 'group', labelKey: 'action.group', hintKey: 'action.group.select.hint' },
]

const draft = reactive({
  name: '',
  kind: 'domain_suffix' as RuleMatcher['kind'],
  value: '',
  ruleSetId: '',
  actionKind: 'proxy' as ActionKind,
  groupId: '',
  enabled: true,
})

const error = ref<string | null>(null)

/** Only enabled groups are offered: a rule pointing at a disabled group would
 *  render an outbound the kernel never receives. */
const usableGroups = computed(() => groups.items.filter((group) => group.enabled))

/** Same rule for rule sets: a disabled set is not a legal target. */
const usableRuleSets = computed(() => ruleSets.items.filter((set) => set.enabled))

watch(
  () => props.open,
  async (open) => {
    if (!open) return
    error.value = null
    await Promise.all([groups.load(), ruleSets.load()])
    const source = props.rule
    if (source) {
      draft.name = source.name
      draft.kind = source.matcher.kind
      draft.ruleSetId = source.matcher.kind === 'rule_set' ? source.matcher.value : ''
      draft.value = source.matcher.kind === 'rule_set' ? '' : String(source.matcher.value ?? '')
      draft.actionKind = source.action.kind
      draft.groupId = source.action.kind === 'group' ? source.action.group_id : ''
      draft.enabled = source.enabled
    } else {
      draft.name = ''
      draft.kind = 'domain_suffix'
      draft.value = ''
      draft.ruleSetId = usableRuleSets.value[0]?.id ?? ''
      draft.actionKind = 'proxy'
      draft.groupId = usableGroups.value[0]?.id ?? ''
      draft.enabled = true
    }
  },
)

const placeholder = () =>
  matcherKinds.find((item) => item.value === draft.kind)?.placeholder ?? ''

const actionHint = (): string => {
  const hintKey = actionKinds.find((item) => item.value === draft.actionKind)?.hintKey
  if (draft.actionKind !== 'group') return hintKey ? t(hintKey) : ''
  const group = usableGroups.value.find((item) => item.id === draft.groupId)
  return t('action.group.hint', { name: group?.name ?? '—' })
}

function buildMatcher(): RuleMatcher {
  if (draft.kind === 'port') {
    return { kind: 'port', value: Number(draft.value) || 0 }
  }
  if (draft.kind === 'rule_set') {
    // The payload is the set id, not its name: renaming a set must not break
    // the rules that point at it.
    return { kind: 'rule_set', value: draft.ruleSetId }
  }
  // The remaining matchers all share the same string payload; the tagged union
  // is what keeps them apart on the Rust side.
  const value = draft.value.trim()
  switch (draft.kind) {
    case 'domain_suffix':
      return { kind: 'domain_suffix', value }
    case 'domain_keyword':
      return { kind: 'domain_keyword', value }
    case 'domain_full':
      return { kind: 'domain_full', value }
    case 'domain_regex':
      return { kind: 'domain_regex', value }
    case 'geo_site':
      return { kind: 'geo_site', value }
    case 'ip_cidr':
      return { kind: 'ip_cidr', value }
    case 'geo_ip':
      return { kind: 'geo_ip', value }
    case 'process_name':
      return { kind: 'process_name', value }
  }
}

function buildAction(): RuleAction {
  if (draft.actionKind === 'group') return { kind: 'group', group_id: draft.groupId }
  if (draft.actionKind === 'direct') return { kind: 'direct' }
  if (draft.actionKind === 'block') return { kind: 'block' }
  return { kind: 'proxy' }
}

function submit() {
  error.value = null
  if (!draft.name.trim()) {
    error.value = t('ruleEditor.err.name')
    return
  }
  if (draft.kind === 'rule_set') {
    if (!draft.ruleSetId) {
      error.value = t('ruleEditor.err.ruleSet')
      return
    }
  } else if (!String(draft.value).trim()) {
    error.value = t('ruleEditor.err.value')
    return
  }
  if (draft.kind === 'port' && !(Number(draft.value) > 0 && Number(draft.value) <= 65535)) {
    error.value = t('ruleEditor.err.port')
    return
  }
  if (draft.actionKind === 'group' && !draft.groupId) {
    error.value = t('ruleEditor.err.group')
    return
  }

  emit('submit', {
    id: props.rule?.id ?? crypto.randomUUID(),
    name: draft.name.trim(),
    enabled: draft.enabled,
    matcher: buildMatcher(),
    action: buildAction(),
  })
}
</script>

<template>
  <ModalDialog
    :open="open"
    :title="rule ? t('ruleEditor.edit') : t('ruleEditor.new')"
    :width="560"
    :close-on-overlay="false"
    @close="emit('close')"
  >
    <div class="field">
      <label class="field-label" for="rule-name">{{ t('ruleEditor.name') }}</label>
      <input
        id="rule-name"
        v-model="draft.name"
        class="input"
        :placeholder="t('ruleEditor.namePlaceholder')"
      />
    </div>

    <div class="field">
      <label class="field-label" for="rule-kind">{{ t('ruleEditor.matcher') }}</label>
      <select id="rule-kind" v-model="draft.kind" class="select">
        <option v-for="item in matcherKinds" :key="item.value" :value="item.value">
          {{ t(item.labelKey) }}
        </option>
      </select>
    </div>

    <div v-if="draft.kind === 'rule_set'" class="field">
      <label class="field-label" for="rule-set">{{ t('ruleEditor.ruleSet') }}</label>
      <select
        v-if="usableRuleSets.length > 0"
        id="rule-set"
        v-model="draft.ruleSetId"
        class="select"
      >
        <option v-for="set in usableRuleSets" :key="set.id" :value="set.id">
          {{ set.name }} · {{ set.entry_count ?? 0 }}
        </option>
      </select>
      <p v-else class="field-hint">{{ t('ruleEditor.ruleSetEmpty') }}</p>
    </div>

    <div v-else class="field">
      <label class="field-label" for="rule-value">{{ t('ruleEditor.value') }}</label>
      <input
        id="rule-value"
        v-model="draft.value"
        class="input input-mono"
        :placeholder="placeholder()"
      />
      <p class="field-hint">{{ t('ruleEditor.hint') }}</p>
    </div>

    <div class="field">
      <label class="field-label">{{ t('ruleEditor.action') }}</label>
      <div class="segmented">
        <button
          v-for="item in actionKinds"
          :key="item.value"
          type="button"
          :class="{ active: draft.actionKind === item.value }"
              :aria-pressed="draft.actionKind === item.value"
          @click="draft.actionKind = item.value"
        >
          {{ t(item.labelKey) }}
        </button>
      </div>
      <p class="field-hint">{{ actionHint() }}</p>
    </div>

    <div v-if="draft.actionKind === 'group'" class="field">
      <label class="field-label" for="rule-group">{{ t('ruleEditor.group') }}</label>
      <select v-if="usableGroups.length > 0" id="rule-group" v-model="draft.groupId" class="select">
        <option v-for="group in usableGroups" :key="group.id" :value="group.id">
          {{ group.name }}
        </option>
      </select>
      <p v-else class="field-hint">{{ t('ruleEditor.groupEmpty') }}</p>
    </div>

    <ToggleSwitch v-model="draft.enabled" :label="t('ruleEditor.enabled')" />

    <p v-if="error" class="alert">{{ error }}</p>

    <template #footer>
      <button class="btn" type="button" @click="emit('close')">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary" type="button" @click="submit">{{ t('common.save') }}</button>
    </template>
  </ModalDialog>
</template>
