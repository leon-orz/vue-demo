<script setup lang="ts">
import { computed } from 'vue'

import { formatTokens, formatPercent, inputTotal } from '@/lib/format'
import type { KimiUsageGroup } from '@toogl/core-types'

const props = defineProps<{ items: KimiUsageGroup[]; emptyText: string }>()

/** 条宽按输入总量占比计算（相对最大值） */
const rows = computed(() => {
  const max = Math.max(0, ...props.items.map((g) => inputTotal(g.totals)))
  return props.items.map((g) => ({
    key: g.key,
    total: inputTotal(g.totals),
    output: g.totals.output,
    rate: g.totals.cache_hit_rate,
    width: max > 0 ? (inputTotal(g.totals) / max) * 100 : 0,
  }))
})
</script>

<template>
  <div v-if="rows.length" class="flex flex-col gap-3">
    <div v-for="row in rows" :key="row.key" class="flex flex-col gap-1">
      <div class="flex items-baseline justify-between gap-2 text-sm">
        <span class="truncate font-medium" :title="row.key">{{ row.key }}</span>
        <span class="shrink-0 text-muted-foreground">
          {{ formatTokens(row.total) }} · 命中率 {{ formatPercent(row.rate) }}
        </span>
      </div>
      <div class="h-2 w-full overflow-hidden rounded-full bg-muted">
        <div class="h-full rounded-full bg-primary/70" :style="{ width: `${row.width}%` }" />
      </div>
    </div>
  </div>
  <p v-else class="py-6 text-center text-sm text-muted-foreground">{{ emptyText }}</p>
</template>
