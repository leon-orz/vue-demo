<script setup lang="ts">
import { usePreferredDark } from '@vueuse/core'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'

import { echarts } from '@/lib/echarts'
import { useSettingsStore } from '@/stores/settings'
import type { UsageSeries } from '@toogl/core-types'

const props = defineProps<{ series: UsageSeries | null }>()

const settings = useSettingsStore()
const el = ref<HTMLDivElement | null>(null)
let chart: echarts.ECharts | null = null

/** 系统级暗色偏好（响应式，系统切换主题时自动更新） */
const preferredDark = usePreferredDark()

/** 暗色模式下坐标轴/提示框跟随主题；system 时跟随系统，亮/暗时强制 */
const isDark = computed(
  () => settings.theme === 'dark' || (settings.theme === 'system' && preferredDark.value),
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
// store 永远整体替换 series，浅 watch 即可，无需 deep
watch([() => props.series, isDark], render)
</script>

<template>
  <div ref="el" class="h-64 w-full" />
</template>
