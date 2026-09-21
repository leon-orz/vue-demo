<script setup lang="ts">
import { ArrowRight, WandSparkles } from '@lucide/vue'
import { onMounted } from 'vue'
import { RouterLink } from 'vue-router'

import CacheHitBadge from '@/components/CacheHitBadge.vue'
import LoadErrorBanner from '@/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatTokens, inputTotal } from '@/lib/format'
import { useKimiStore } from '@/stores/kimi'
import { useSettingsStore } from '@/stores/settings'

const kimi = useKimiStore()
const settings = useSettingsStore()

// Dashboard 只读各模块缓存数据；Kimi 未初始化时补一次（后台扫描同 Kimi 页）
onMounted(() => kimi.init(settings.kimiSessionsDir || undefined))
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">Dashboard</h1>
      <p class="mt-1 text-sm text-muted-foreground">各模块状态摘要，点击卡片进入对应模块。</p>
    </header>

    <!-- Kimi 数据加载失败时在此提示（与 Kimi 页同一横幅） -->
    <LoadErrorBanner
      v-if="kimi.loadError"
      :error="kimi.loadError"
      @retry="kimi.retry(settings.kimiSessionsDir || undefined)"
    />

    <section class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
      <!-- DB-01：Kimi 今日用量摘要；DB-02：点击跳转 Kimi 页 -->
      <RouterLink to="/kimi" class="block transition-transform hover:-translate-y-0.5">
        <Card class="h-full">
          <CardHeader class="flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle class="text-base">Kimi 今日用量</CardTitle>
            <ArrowRight class="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent class="flex flex-col gap-2">
            <div class="flex items-baseline gap-6">
              <div>
                <p class="text-xs text-muted-foreground">输入</p>
                <p class="text-xl font-bold">
                  {{ kimi.todaySummary ? formatTokens(inputTotal(kimi.todaySummary.totals)) : '—' }}
                </p>
              </div>
              <div>
                <p class="text-xs text-muted-foreground">输出</p>
                <p class="text-xl font-bold">
                  {{ kimi.todaySummary ? formatTokens(kimi.todaySummary.totals.output) : '—' }}
                </p>
              </div>
            </div>
            <CacheHitBadge :rate="kimi.todaySummary?.totals.cache_hit_rate ?? null" />
          </CardContent>
        </Card>
      </RouterLink>

      <!-- DB-01：代理状态占位（S1-3 交付） -->
      <Card class="h-full opacity-70">
        <CardHeader class="flex-row items-center justify-between space-y-0 pb-2">
          <CardTitle class="text-base">代理状态</CardTitle>
          <WandSparkles class="size-4 text-muted-foreground" />
        </CardHeader>
        <CardContent class="flex flex-col gap-2">
          <p class="text-sm text-muted-foreground">魔法工具模块尚未交付。</p>
          <Badge variant="outline" class="w-fit">S1-3 交付</Badge>
        </CardContent>
      </Card>
    </section>
  </div>
</template>
