<script setup lang="ts">
const model = defineModel<boolean>({ required: true })
defineProps<{ label: string; description?: string; disabled?: boolean }>()
</script>

<template>
  <label class="switch-row" :class="{ disabled }">
    <span class="switch-copy">
      <span class="switch-label">{{ label }}</span>
      <span v-if="description" class="switch-description">{{ description }}</span>
    </span>
    <button
      type="button"
      role="switch"
      class="switch"
      :aria-checked="model"
      :aria-label="label"
      :disabled="disabled"
      @click="model = !model"
    >
      <span class="switch-knob" />
    </button>
  </label>
</template>

<style scoped>
.switch-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 12px 0;
  cursor: pointer;
}

.switch-row.disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.switch-copy {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.switch-label {
  font-size: 13px;
  font-weight: 550;
}

.switch-description {
  font-size: 12px;
  color: var(--text-tertiary);
}

.switch {
  flex: none;
  width: 42px;
  height: 24px;
  padding: 2px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-pill);
  background: var(--surface-sunken);
  cursor: pointer;
  transition:
    background var(--duration-base) var(--ease-out),
    border-color var(--duration-base) var(--ease-out);
}

.switch[aria-checked="true"] {
  background: var(--accent);
  border-color: transparent;
}

.switch-knob {
  display: block;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--surface-solid);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.25);
  transition: transform var(--duration-base) var(--ease-out);
}

.switch[aria-checked="true"] .switch-knob {
  transform: translateX(18px);
}
</style>
