<script setup lang="ts">
import { computed } from 'vue'

import { Badge } from '@/components/ui/badge'
import { formatPercent } from '@/lib/format'

const props = defineProps<{ rate: number | null }>()

// 命中率着色：>=90% 绿，>=70% 黄，其余红；无数据（null）灰色占位
const tone = computed(() => {
  if (props.rate === null) return 'bg-muted text-muted-foreground'
  if (props.rate >= 0.9) return 'bg-green-600/15 text-green-600 dark:text-green-400'
  if (props.rate >= 0.7) return 'bg-yellow-600/15 text-yellow-600 dark:text-yellow-400'
  return 'bg-red-600/15 text-red-600 dark:text-red-400'
})
</script>

<template>
  <Badge variant="outline" class="border-transparent" :class="tone">
    缓存命中率 {{ formatPercent(rate) }}
  </Badge>
</template>
