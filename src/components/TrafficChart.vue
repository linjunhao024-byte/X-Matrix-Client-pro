<script setup lang="ts">
/**
 * Live transfer-rate chart.
 *
 * Two series only (design rule: at most five lines per chart), no symbols, no
 * animation — it updates once a second and must not flicker. Colours come from
 * the theme tokens, so light and dark both look right without a second config.
 */
import { computed } from 'vue'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart } from 'echarts/charts'
import { GridComponent, TooltipComponent } from 'echarts/components'

import { t } from '../i18n'
import { chartTokens } from '../services/chart'
import { formatRate } from '../services/format'
import type { TrafficPoint } from '../stores/engine'
import { useUiStore } from '../stores/ui'

use([CanvasRenderer, LineChart, GridComponent, TooltipComponent])

const props = defineProps<{ points: TrafficPoint[] }>()

const ui = useUiStore()

const option = computed(() => {
  // Depend on the resolved theme so a theme switch re-reads the tokens.
  void ui.resolvedTheme
  const tokens = chartTokens()
  const labels = props.points.map((point) => new Date(point.at).toLocaleTimeString())
  return {
    animation: false,
    grid: { left: 6, right: 6, top: 8, bottom: 2, containLabel: false },
    tooltip: {
      trigger: 'axis' as const,
      backgroundColor: 'transparent',
      borderWidth: 0,
      padding: 0,
      valueFormatter: (value: unknown) => formatRate(Number(value)),
      textStyle: { color: tokens.textPrimary, fontSize: 12 },
    },
    xAxis: {
      type: 'category' as const,
      show: false,
      boundaryGap: false,
      data: labels,
    },
    yAxis: { type: 'value' as const, show: false, min: 0 },
    series: [
      {
        name: t('conn.rate.down'),
        type: 'line' as const,
        smooth: true,
        showSymbol: false,
        data: props.points.map((point) => point.down),
        lineStyle: { width: 2, color: tokens.accent },
        areaStyle: { color: tokens.accent, opacity: 0.14 },
      },
      {
        name: t('conn.rate.up'),
        type: 'line' as const,
        smooth: true,
        showSymbol: false,
        data: props.points.map((point) => point.up),
        lineStyle: { width: 2, color: tokens.success },
      },
    ],
  }
})
</script>

<template>
  <div class="chart-wrap" :class="{ 'is-empty': props.points.length <= 1 }">
    <VChart v-if="props.points.length > 1" class="chart" :option="option" autoresize />
    <div v-else class="chart-empty muted">{{ t('home.chart.empty') }}</div>
  </div>
</template>

<style scoped>
.chart-wrap {
  height: 132px;
  display: flex;
  align-items: center;
  justify-content: center;
}

.chart {
  width: 100%;
  height: 100%;
}

.chart-empty {
  font-size: 13px;
}

/* Before the engine runs there is no curve, so the panel shrinks to a line of
   hint text instead of reserving a full plot area of empty space. */
.chart-wrap.is-empty {
  height: 76px;
}
</style>
