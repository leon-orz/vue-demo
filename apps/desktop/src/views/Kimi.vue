<script setup lang="ts">
import { RefreshCw } from '@lucide/vue'
import { onMounted } from 'vue'

import BalanceCard from '@/components/BalanceCard.vue'
import CacheHitBadge from '@/components/CacheHitBadge.vue'
import GroupBarList from '@/components/GroupBarList.vue'
import LoadErrorBanner from '@/components/LoadErrorBanner.vue'
import UsageTrendChart from '@/components/UsageTrendChart.vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { formatDateTime, formatTokens, inputTotal } from '@/lib/format'
import { useKimiStore, type TrendRange } from '@/stores/kimi'
import { useSettingsStore } from '@/stores/settings'

const kimi = useKimiStore()
const settings = useSettingsStore()

const trendTabs: { value: TrendRange; label: string }[] = [
  { value: 'Day', label: '天' },
  { value: 'Week', label: '周' },
  { value: 'Month', label: '月' },
]

// 打开页面：先读 SQLite 缓存渲染，后台增量扫描完成后自动刷新
onMounted(() => kimi.init(settings.kimiSessionsDir || undefined))
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold tracking-tight">Kimi 统计</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          <template v-if="kimi.lastScanAt">
            上次扫描：{{ formatDateTime(kimi.lastScanAt) }}
            <template v-if="kimi.lastScanReport">
              · 本次新增 {{ kimi.lastScanReport.events_added }} 条事件
            </template>
          </template>
          <template v-else-if="kimi.scanError">扫描失败：{{ kimi.scanError }}</template>
          <template v-else>正在后台扫描日志…</template>
        </p>
      </div>
      <Button
        variant="outline"
        :disabled="kimi.scanning"
        @click="kimi.scan(settings.kimiSessionsDir || undefined)"
      >
        <RefreshCw class="size-4" :class="kimi.scanning ? 'animate-spin' : ''" />
        {{ kimi.scanning ? '扫描中…' : '重新扫描' }}
      </Button>
    </header>

    <!-- 初始化/聚合加载失败：分类提示，可重试或去设置页 -->
    <LoadErrorBanner
      v-if="kimi.loadError"
      :error="kimi.loadError"
      @retry="kimi.retry(settings.kimiSessionsDir || undefined)"
    />

    <!-- 概览卡片 -->
    <section class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
      <Card>
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">今日 Token</CardTitle>
        </CardHeader>
        <CardContent class="flex flex-col gap-1">
          <div class="flex items-baseline justify-between">
            <span class="text-sm text-muted-foreground">输入</span>
            <span class="text-xl font-bold">
              {{ kimi.todaySummary ? formatTokens(inputTotal(kimi.todaySummary.totals)) : '—' }}
            </span>
          </div>
          <div class="flex items-baseline justify-between">
            <span class="text-sm text-muted-foreground">输出</span>
            <span class="text-xl font-bold">
              {{ kimi.todaySummary ? formatTokens(kimi.todaySummary.totals.output) : '—' }}
            </span>
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">今日缓存命中率</CardTitle>
        </CardHeader>
        <CardContent>
          <CacheHitBadge :rate="kimi.todaySummary?.totals.cache_hit_rate ?? null" />
          <p class="mt-2 text-xs text-muted-foreground">
            缓存读取
            {{ kimi.todaySummary ? formatTokens(kimi.todaySummary.totals.cache_read) : '—' }}
          </p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">最近 5 小时</CardTitle>
        </CardHeader>
        <CardContent class="flex flex-col gap-1">
          <span class="text-xl font-bold">
            {{ kimi.last5Summary ? formatTokens(inputTotal(kimi.last5Summary.totals)) : '—' }}
          </span>
          <p class="text-xs text-muted-foreground">输入（含缓存）· 滚动窗口</p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">本月累计</CardTitle>
        </CardHeader>
        <CardContent class="flex flex-col gap-1">
          <span class="text-xl font-bold">
            {{ kimi.monthSummary ? formatTokens(inputTotal(kimi.monthSummary.totals)) : '—' }}
          </span>
          <p class="text-xs text-muted-foreground">
            输出
            {{ kimi.monthSummary ? formatTokens(kimi.monthSummary.totals.output) : '—' }}
          </p>
        </CardContent>
      </Card>
    </section>

    <!-- 趋势图 -->
    <Card>
      <CardHeader class="flex-row items-center justify-between space-y-0">
        <CardTitle class="text-base">用量趋势</CardTitle>
        <Tabs
          :model-value="kimi.trendRange"
          @update:model-value="kimi.setTrendRange($event as TrendRange)"
        >
          <TabsList>
            <TabsTrigger v-for="tab in trendTabs" :key="tab.value" :value="tab.value">
              {{ tab.label }}
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </CardHeader>
      <CardContent>
        <UsageTrendChart v-if="kimi.series?.points.length" :series="kimi.series" />
        <p v-else class="py-10 text-center text-sm text-muted-foreground">暂无数据</p>
      </CardContent>
    </Card>

    <!-- 分布 -->
    <section class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <Card>
        <CardHeader>
          <CardTitle class="text-base">按项目分布</CardTitle>
        </CardHeader>
        <CardContent>
          <GroupBarList :items="kimi.byProject" empty-text="该时间范围内暂无用量" />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle class="text-base">按模型分布</CardTitle>
        </CardHeader>
        <CardContent>
          <GroupBarList :items="kimi.byModel" empty-text="该时间范围内暂无用量" />
        </CardContent>
      </Card>
    </section>

    <Separator />

    <!-- 余额 -->
    <BalanceCard />
  </div>
</template>
