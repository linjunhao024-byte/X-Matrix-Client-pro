<script setup lang="ts">
import type { IconName } from './AppIcon.vue'
import AppIcon from './AppIcon.vue'

defineProps<{ icon?: IconName; title: string; body: string }>()
</script>

<template>
  <div class="empty-state">
    <div class="empty-state-icon">
      <AppIcon :name="icon ?? 'inbox'" :size="22" />
    </div>
    <div class="empty-state-title">{{ title }}</div>
    <p class="empty-state-body">{{ body }}</p>
    <div v-if="$slots.actions" class="empty-state-actions">
      <slot name="actions" />
    </div>
  </div>
</template>
