<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import { echarts } from '@/lib/echarts'
import { useSettingsStore } from '@/stores/settings'
import type { KimiUsageSeries } from '@toogl/core-types'

const props = defineProps<{ series: KimiUsageSeries | null }>()

const settings = useSettingsStore()
const el = ref<HTMLDivElement | null>(null)
let chart: echarts.ECharts | null = null

/** 暗色模式下坐标轴/提示框跟随主题 */
const isDark = computed(
  () =>
    settings.theme === 'dark' ||
    (settings.theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches),
)

function render() {
  if (!chart || !props.series) return
  const axisColor = isDark.value ? '#a3a3a3' : '#525252'
  chart.setOption({
    backgroundColor: 'transparent',
    tooltip: { trigger: 'axis' },
    legend: { data: ['输入（含缓存）', '输出'], textStyle: { color: axisColor } },
    grid: { left: 8, right: 16, top: 32, bottom: 8, containLabel: true },
    xAxis: {
      type: 'category',
      data: props.series.points.map((p) => p.bucket),
      axisLabel: { color: axisColor },
    },
    yAxis: { type: 'value', axisLabel: { color: axisColor } },
    series: [
      {
        name: '输入（含缓存）',
        type: 'line',
        smooth: true,
        areaStyle: { opacity: 0.15 },
        data: props.series.points.map(
          (p) => p.totals.input_other + p.totals.cache_read + p.totals.cache_creation,
        ),
      },
      {
        name: '输出',
        type: 'line',
        smooth: true,
        areaStyle: { opacity: 0.15 },
        data: props.series.points.map((p) => p.totals.output),
      },
    ],
  })
}

const onResize = () => chart?.resize()

onMounted(() => {
  chart = echarts.init(el.value!)
  render()
  window.addEventListener('resize', onResize)
})
onBeforeUnmount(() => {
  window.removeEventListener('resize', onResize)
  chart?.dispose()
  chart = null
})
watch(() => [props.series, isDark.value], render, { deep: true })
</script>

<template>
  <div ref="el" class="h-64 w-full" />
</template>
