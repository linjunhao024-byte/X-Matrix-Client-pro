<script setup lang="ts">
/**
 * Inline icon set. Keeping the paths in one component avoids an icon-font
 * dependency and guarantees the strokes inherit `currentColor`.
 */
export type IconName =
  | 'home'
  | 'nodes'
  | 'routing'
  | 'connections'
  | 'settings'
  | 'sun'
  | 'moon'
  | 'sidebar'
  | 'plus'
  | 'trash'
  | 'edit'
  | 'inbox'
  | 'cloud'
  | 'layers'
  | 'refresh'
  | 'check'
  | 'power'
  | 'play'
  | 'stop'
  | 'arrow-down'
  | 'arrow-up'
  | 'info'
  | 'copy'
  | 'file'

const props = withDefaults(defineProps<{ name: IconName; size?: number }>(), { size: 18 })

const paths: Record<IconName, string[]> = {
  home: ['M4 10.6 12 4l8 6.6V20a1 1 0 0 1-1 1h-4.5v-6.5h-5V21H5a1 1 0 0 1-1-1z'],
  nodes: ['M4 5.5h16v5H4z', 'M4 13.5h16v5H4z', 'M7.5 8h.01', 'M7.5 16h.01'],
  routing: ['M4 7h4l8 10h4', 'M4 17h4l2-2.5', 'M17 4l3 3-3 3', 'M17 14l3 3-3 3'],
  connections: ['M3 12h3.5l2.5-6 4 12 2.5-6H21'],
  settings: [
    'M4 8h7',
    'M15 8h5',
    'M4 16h5',
    'M13 16h7',
    'M13 8a2 2 0 1 1-4 0 2 2 0 0 1 4 0',
    'M11 16a2 2 0 1 1-4 0 2 2 0 0 1 4 0',
  ],
  sun: [
    'M12 4v2',
    'M12 18v2',
    'M4 12h2',
    'M18 12h2',
    'M6.3 6.3l1.4 1.4',
    'M16.3 16.3l1.4 1.4',
    'M6.3 17.7l1.4-1.4',
    'M16.3 7.7l1.4-1.4',
    'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8',
  ],
  moon: ['M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z'],
  sidebar: ['M4 5h16v14H4z', 'M10 5v14'],
  plus: ['M12 5v14', 'M5 12h14'],
  trash: ['M4 7h16', 'M9 7V5h6v2', 'M6 7l1 13h10l1-13'],
  edit: ['M4 20h4L18 10l-4-4L4 16z', 'M14 6l4 4'],
  inbox: ['M4 13h4l1.5 3h5L16 13h4', 'M4 13 6 5h12l2 8v6H4z'],
  layers: ['M12 3.5 3.5 8 12 12.5 20.5 8z', 'M3.5 12.5 12 17l8.5-4.5', 'M3.5 16.5 12 21l8.5-4.5'],
  cloud: ['M7 18h10a4 4 0 0 0 .6-7.96A6 6 0 0 0 6.1 11.2', 'M7 18a4 4 0 0 1-.4-7.98'],
  refresh: ['M20 12a8 8 0 1 1-2.3-5.6', 'M20 4v5h-5'],
  check: ['M5 12.5l4.5 4.5L19 7'],
  power: ['M12 4v8', 'M7.5 7a7 7 0 1 0 9 0'],
  play: ['M8 5.5v13l11-6.5z'],
  stop: ['M7 7h10v10H7z'],
  'arrow-down': ['M12 5v14', 'M6 13l6 6 6-6'],
  'arrow-up': ['M12 19V5', 'M6 11l6-6 6 6'],
  info: ['M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16', 'M12 11v5', 'M12 8h.01'],
  copy: ['M9 9h10v11H9z', 'M15 9V4H5v11h4'],
  file: ['M6 3h8l4 4v14H6z', 'M14 3v4h4'],
}
</script>

<template>
  <svg
    :width="props.size"
    :height="props.size"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    stroke-width="1.7"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
    focusable="false"
  >
    <path v-for="(d, index) in paths[props.name]" :key="index" :d="d" />
  </svg>
</template>
