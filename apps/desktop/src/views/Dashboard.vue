<script setup lang="ts">
import { ArrowRight, WandSparkles } from '@lucide/vue'
import { computed, onMounted } from 'vue'
import { RouterLink, useRouter } from 'vue-router'

import CacheHitBadge from '@/components/CacheHitBadge.vue'
import LoadErrorBanner from '@/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatMoney, formatTokens, inputTotal } from '@/lib/format'
import { RULE_MODE_LABELS, useProxyStore } from '@/stores/proxy'
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
const proxy = useProxyStore()
const router = useRouter()

/** 代理卡当前选中节点摘要：第一个 selector 组的 now；兜底用记住的选择 */
const proxyCurrentNode = computed(() => {
  const selector = proxy.status?.proxies?.groups.find((g) => g.group_type === 'selector')
  if (selector) return `${selector.name} · ${selector.now}`
  const remembered = proxy.status?.remembered_selection
  return remembered ? `${remembered.group} · ${remembered.node}` : ''
})

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

// Dashboard 只读各模块缓存数据；用量 store 未初始化时补一次（后台扫描同用量页），代理状态拉一次快照
onMounted(() => {
  void usage.init(settings.kimiSessionsDir || undefined)
  void proxy.refresh()
})
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

      <!-- DB-01：代理状态摘要（当前模式 + 选中节点 + 系统代理），不可用显示占位不报错 -->
      <RouterLink to="/proxy" class="block transition-transform hover:-translate-y-0.5">
        <Card class="h-full">
          <CardHeader class="flex-row items-center justify-between space-y-0 pb-2">
            <CardTitle class="text-base">代理状态</CardTitle>
            <WandSparkles class="size-4 text-muted-foreground" />
          </CardHeader>
          <CardContent class="flex flex-col gap-2">
            <template v-if="proxy.status && proxy.status.sidecar_alive">
              <div class="flex items-center gap-2">
                <Badge variant="secondary">
                  {{
                    proxy.status.rule_mode ? RULE_MODE_LABELS[proxy.status.rule_mode] : '未知模式'
                  }}
                </Badge>
                <Badge :variant="proxy.status.system_proxy?.enabled ? 'default' : 'outline'">
                  系统代理{{ proxy.status.system_proxy?.enabled ? '开' : '关' }}
                </Badge>
              </div>
              <p class="truncate text-sm text-muted-foreground">
                {{ proxyCurrentNode || '未选择节点' }}
              </p>
            </template>
            <template v-else>
              <p class="text-sm text-muted-foreground">
                {{
                  proxy.status?.startup_error
                    ? 'sidecar 启动失败，详见魔法工具页'
                    : 'mihomo sidecar 未运行'
                }}
              </p>
            </template>
          </CardContent>
        </Card>
      </RouterLink>
    </section>
  </div>
</template>
