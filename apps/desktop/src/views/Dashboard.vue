<script setup lang="ts">
import { ArrowRight, WandSparkles } from '@lucide/vue'
import { onMounted } from 'vue'
import { RouterLink, useRouter } from 'vue-router'

import CacheHitBadge from '@/components/CacheHitBadge.vue'
import LoadErrorBanner from '@/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatMoney, formatTokens, inputTotal } from '@/lib/format'
import {
  parseMimoQuotaDetail,
  PLATFORM_LABELS,
  USAGE_PLATFORMS,
  useUsageStore,
} from '@/stores/usage'
import { useSettingsStore } from '@/stores/settings'
import type { Platform } from '@toogl/core-types'

const usage = useUsageStore()
const settings = useSettingsStore()
const router = useRouter()

/** 各平台配额/余额的一行摘要；未配置/不可达时返回引导文案而非报错 */
function quotaSummary(platform: Platform): string {
  const account = usage.accounts[platform]
  const snapshot = usage.quotas[platform]
  switch (platform) {
    case 'omp':
      return '仅本地日志统计，无配额数据源'
    case 'mimo': {
      if (account?.quota_total == null) return '配额未配置，去设置页手录套餐总额'
      const progress = parseMimoQuotaDetail(snapshot)?.progress
      if (progress == null) return '套餐进度暂无数据，去用量页刷新'
      return `套餐已用 ${(progress * 100).toFixed(1)}%${progress > 1 ? '（已超额）' : ''}`
    }
    default: {
      // kimi / deepseek：按量余额
      if (!account?.has_credential) return '余额未配置，去设置页配置 API Key'
      if (snapshot?.remaining == null) return '余额暂无数据，去用量页刷新'
      return `余额 ${formatMoney(snapshot.remaining)}`
    }
  }
}

/** 是否需要「去设置页配置」引导（未配置时卡片显示引导入口，不报错） */
function needsSetup(platform: Platform): boolean {
  if (platform === 'omp') return false
  if (platform === 'mimo') return usage.accounts[platform]?.quota_total == null
  return !usage.accounts[platform]?.has_credential
}

// Dashboard 只读各模块缓存数据；用量 store 未初始化时补一次（后台扫描同用量页）
onMounted(() => usage.init(settings.kimiSessionsDir || undefined))
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">Dashboard</h1>
      <p class="mt-1 text-sm text-muted-foreground">各模块状态摘要，点击卡片进入对应模块。</p>
    </header>

    <!-- 用量数据加载失败时在此提示（与用量页同一横幅） -->
    <LoadErrorBanner
      v-if="usage.loadError"
      :error="usage.loadError"
      @retry="usage.retry(settings.kimiSessionsDir || undefined)"
    />

    <!-- DB-01：各平台今日用量 + 配额摘要；DB-02：点击跳转用量页 -->
    <section class="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
      <RouterLink
        v-for="platform in USAGE_PLATFORMS"
        :key="platform"
        to="/usage"
        class="block transition-transform hover:-translate-y-0.5"
      >
        <Card class="h-full">
          <CardHeader class="flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle class="text-base">{{ PLATFORM_LABELS[platform] }} 今日用量</CardTitle>
            <ArrowRight class="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent class="flex flex-col gap-2">
            <div class="flex items-baseline gap-6">
              <div>
                <p class="text-xs text-muted-foreground">输入</p>
                <p class="text-xl font-bold">
                  {{
                    usage.todayByFilter[platform]
                      ? formatTokens(inputTotal(usage.todayByFilter[platform]!.totals))
                      : '—'
                  }}
                </p>
              </div>
              <div>
                <p class="text-xs text-muted-foreground">输出</p>
                <p class="text-xl font-bold">
                  {{
                    usage.todayByFilter[platform]
                      ? formatTokens(usage.todayByFilter[platform]!.totals.output)
                      : '—'
                  }}
                </p>
              </div>
            </div>
            <CacheHitBadge :rate="usage.todayByFilter[platform]?.totals.cache_hit_rate ?? null" />
            <p class="text-xs text-muted-foreground">
              {{ quotaSummary(platform) }}
              <span
                v-if="needsSetup(platform)"
                class="cursor-pointer text-primary underline-offset-4 hover:underline"
                @click.prevent.stop="router.push('/settings')"
                >去配置</span
              >
            </p>
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
