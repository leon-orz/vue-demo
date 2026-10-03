<script setup lang="ts">
import { RefreshCw } from '@lucide/vue'
import { onMounted } from 'vue'

import CacheHitBadge from '@/components/CacheHitBadge.vue'
import GroupBarList from '@/components/GroupBarList.vue'
import LoadErrorBanner from '@/components/LoadErrorBanner.vue'
import QuotaCard from '@/components/QuotaCard.vue'
import UsageTrendChart from '@/components/UsageTrendChart.vue'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Separator } from '@/components/ui/separator'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { formatDateTime, formatTokens, inputTotal } from '@/lib/format'
import { PLATFORM_LABELS, USAGE_PLATFORMS, useUsageStore, type TrendRange } from '@/stores/usage'
import { useSettingsStore } from '@/stores/settings'
import type { PlatformFilter } from '@toogl/core-types'

const usage = useUsageStore()
const settings = useSettingsStore()

/** 平台切换 Tab：总览（all = 跨平台合计）+ 四个平台 */
const platformTabs: { value: PlatformFilter; label: string }[] = [
  { value: 'all', label: '总览' },
  ...USAGE_PLATFORMS.map((p) => ({ value: p as PlatformFilter, label: PLATFORM_LABELS[p] })),
]

const trendTabs: { value: TrendRange; label: string }[] = [
  { value: 'Day', label: '天' },
  { value: 'Week', label: '周' },
  { value: 'Month', label: '月' },
]

// 打开页面：先读 SQLite 缓存渲染，后台增量扫描完成后自动刷新
onMounted(() => usage.init(settings.kimiSessionsDir || undefined))
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold tracking-tight">AI 用量</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          <template v-if="usage.lastScanAt">
            上次扫描：{{ formatDateTime(usage.lastScanAt) }} · 本次新增
            {{ usage.lastScanEventsAdded }} 条事件
            <template v-if="usage.lastScanFailures.length">
              · 失败：
              {{
                usage.lastScanFailures
                  .map((r) => `${PLATFORM_LABELS[r.platform]}（${r.error}）`)
                  .join('；')
              }}
            </template>
          </template>
          <template v-else>正在后台扫描日志…</template>
        </p>
      </div>
      <Button
        variant="outline"
        :disabled="usage.scanning"
        @click="usage.scan(settings.kimiSessionsDir || undefined)"
      >
        <RefreshCw class="size-4" :class="usage.scanning ? 'animate-spin' : ''" />
        {{ usage.scanning ? '扫描中…' : '重新扫描' }}
      </Button>
    </header>

    <!-- 初始化/聚合加载失败：分类提示，可重试或去设置页 -->
    <LoadErrorBanner
      v-if="usage.loadError"
      :error="usage.loadError"
      @retry="usage.retry(settings.kimiSessionsDir || undefined)"
    />

    <!-- 平台切换 Tab：控制下方概览 / 趋势 / 分布的过滤维度 -->
    <Tabs
      :model-value="usage.activeFilter"
      @update:model-value="usage.setActiveFilter($event as PlatformFilter)"
    >
      <TabsList>
        <TabsTrigger v-for="tab in platformTabs" :key="tab.value" :value="tab.value">
          {{ tab.label }}
        </TabsTrigger>
      </TabsList>
    </Tabs>

    <!-- 当前选中过滤的概览卡片 -->
    <section class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
      <Card>
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">今日 Token</CardTitle>
        </CardHeader>
        <CardContent class="flex flex-col gap-1">
          <div class="flex items-baseline justify-between">
            <span class="text-sm text-muted-foreground">输入</span>
            <span class="text-xl font-bold">
              {{ usage.todaySummary ? formatTokens(inputTotal(usage.todaySummary.totals)) : '—' }}
            </span>
          </div>
          <div class="flex items-baseline justify-between">
            <span class="text-sm text-muted-foreground">输出</span>
            <span class="text-xl font-bold">
              {{ usage.todaySummary ? formatTokens(usage.todaySummary.totals.output) : '—' }}
            </span>
          </div>
        </CardContent>
      </Card>
      <Card>
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">今日缓存命中率</CardTitle>
        </CardHeader>
        <CardContent>
          <CacheHitBadge :rate="usage.todaySummary?.totals.cache_hit_rate ?? null" />
          <p class="mt-2 text-xs text-muted-foreground">
            缓存读取
            {{ usage.todaySummary ? formatTokens(usage.todaySummary.totals.cache_read) : '—' }}
          </p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">最近 5 小时</CardTitle>
        </CardHeader>
        <CardContent class="flex flex-col gap-1">
          <span class="text-xl font-bold">
            {{ usage.last5Summary ? formatTokens(inputTotal(usage.last5Summary.totals)) : '—' }}
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
            {{ usage.monthSummary ? formatTokens(inputTotal(usage.monthSummary.totals)) : '—' }}
          </span>
          <p class="text-xs text-muted-foreground">
            输出
            {{ usage.monthSummary ? formatTokens(usage.monthSummary.totals.output) : '—' }}
          </p>
        </CardContent>
      </Card>
    </section>

    <!-- 总览 Tab：各平台今日用量卡片墙 -->
    <section
      v-if="usage.activeFilter === 'all'"
      class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4"
    >
      <Card v-for="platform in USAGE_PLATFORMS" :key="platform">
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-medium text-muted-foreground">
            {{ PLATFORM_LABELS[platform] }} 今日用量
          </CardTitle>
        </CardHeader>
        <CardContent class="flex flex-col gap-2">
          <div class="flex items-baseline justify-between">
            <span class="text-sm text-muted-foreground">输入（含缓存）</span>
            <span class="text-xl font-bold">
              {{
                usage.todayByFilter[platform]
                  ? formatTokens(inputTotal(usage.todayByFilter[platform]!.totals))
                  : '—'
              }}
            </span>
          </div>
          <CacheHitBadge :rate="usage.todayByFilter[platform]?.totals.cache_hit_rate ?? null" />
        </CardContent>
      </Card>
    </section>

    <!-- 趋势图 -->
    <Card>
      <CardHeader class="flex-row items-center justify-between space-y-0">
        <CardTitle class="text-base">用量趋势</CardTitle>
        <Tabs
          :model-value="usage.trendRange"
          @update:model-value="usage.setTrendRange($event as TrendRange)"
        >
          <TabsList>
            <TabsTrigger v-for="tab in trendTabs" :key="tab.value" :value="tab.value">
              {{ tab.label }}
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </CardHeader>
      <CardContent>
        <UsageTrendChart v-if="usage.series?.points.length" :series="usage.series" />
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
          <GroupBarList :items="usage.byProject" empty-text="该时间范围内暂无用量" />
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle class="text-base">按模型分布</CardTitle>
        </CardHeader>
        <CardContent>
          <GroupBarList :items="usage.byModel" empty-text="该时间范围内暂无用量" />
        </CardContent>
      </Card>
    </section>

    <Separator />

    <!-- 配额/余额：总览 Tab 显示全部平台卡片墙，单平台 Tab 只显示该平台 -->
    <section class="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <template v-for="platform in USAGE_PLATFORMS" :key="platform">
        <QuotaCard
          v-if="usage.activeFilter === 'all' || usage.activeFilter === platform"
          :platform="platform"
          :snapshot="usage.quotas[platform]"
          :account="usage.accounts[platform]"
          :loading="usage.quotaLoading[platform] ?? false"
          :error="usage.quotaErrors[platform] ?? null"
          @refresh="usage.refreshQuota(platform)"
        />
      </template>
    </section>
  </div>
</template>
