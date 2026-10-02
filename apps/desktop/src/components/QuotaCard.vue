<script setup lang="ts">
import { RefreshCw } from '@lucide/vue'
import { computed } from 'vue'
import { useRouter } from 'vue-router'

import type { CommandError } from '@/api/errors'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatDateTime, formatMoney, formatTokens } from '@/lib/format'
import {
  parseDeepseekQuotaDetail,
  parseKimiQuotaDetail,
  parseMimoQuotaDetail,
  PLATFORM_LABELS,
} from '@/stores/usage'
import type { Platform, PlatformAccountInfo, QuotaSnapshot } from '@toogl/core-types'

/**
 * 平台配额/余额卡片（M1）：含「不可用 / 未配置」降级态。
 * - kimi：开放平台余额 + coding 5h 窗口 / 月度池（coding_usages_error 非空时降级提示，不影响余额展示）
 * - mimo：套餐进度条（本地日志推算，不发网络请求；未手录总额时引导去设置页）
 * - deepseek：按量余额（未购买/未配置时引导去设置页）
 * - omp：无配额数据源，只提示「仅本地日志统计」
 */
const props = defineProps<{
  platform: Platform
  snapshot: QuotaSnapshot | null | undefined
  account: PlatformAccountInfo | null | undefined
  loading?: boolean
  error?: CommandError | null
}>()

const emit = defineEmits<{ refresh: [] }>()
const router = useRouter()

const label = computed(() => PLATFORM_LABELS[props.platform])

/** 各平台的「已配置」判定：kimi/deepseek 看 keyring 凭据，mimo 看手录的套餐总额 */
const configured = computed(() => {
  if (props.platform === 'mimo') return props.account?.quota_total != null
  if (props.platform === 'omp') return true
  return props.account?.has_credential === true
})

const kimiDetail = computed(() =>
  props.platform === 'kimi' ? parseKimiQuotaDetail(props.snapshot) : null,
)
const mimoDetail = computed(() =>
  props.platform === 'mimo' ? parseMimoQuotaDetail(props.snapshot) : null,
)
const deepseekDetail = computed(() =>
  props.platform === 'deepseek' ? parseDeepseekQuotaDetail(props.snapshot) : null,
)

/** MiMo 套餐进度百分比（超量时进度条顶满，文案标已超额） */
const mimoProgressPercent = computed(() => {
  const progress = mimoDetail.value?.progress
  if (progress == null) return null
  return Math.min(100, Math.max(0, progress * 100))
})

/** Kimi coding 月度池进度百分比（已用 / 总额） */
const kimiMonthPercent = computed(() => {
  const coding = kimiDetail.value?.coding
  if (coding?.month_total == null || coding.month_total <= 0 || coding.month_used == null) {
    return null
  }
  return Math.min(100, Math.max(0, (coding.month_used / coding.month_total) * 100))
})
</script>

<template>
  <Card>
    <CardHeader class="flex-row items-center justify-between space-y-0">
      <CardTitle class="text-base">{{ label }} 配额</CardTitle>
      <Button
        v-if="platform !== 'omp' && configured"
        variant="ghost"
        size="icon"
        :disabled="loading"
        title="刷新配额"
        @click="emit('refresh')"
      >
        <RefreshCw class="size-4" :class="loading ? 'animate-spin' : ''" />
      </Button>
    </CardHeader>
    <CardContent>
      <!-- omp：无配额/余额数据源 -->
      <p v-if="platform === 'omp'" class="text-sm text-muted-foreground">
        仅本地日志统计，无配额/余额数据源。
      </p>

      <!-- 未配置：引导去设置页 -->
      <div
        v-else-if="!configured"
        class="flex flex-col items-start gap-3 text-sm text-muted-foreground"
      >
        <p v-if="platform === 'mimo'">
          尚未手录 MiMo 套餐月度 Credit 总额，配置后按本地日志推算消耗进度。
        </p>
        <p v-else>尚未配置 {{ label }} API Key，配置后可查询配额/余额。</p>
        <Button variant="outline" size="sm" @click="router.push('/settings')">去设置页配置</Button>
      </div>

      <div v-else class="flex flex-col gap-3">
        <template v-if="snapshot">
          <!-- Kimi：余额 + coding 窗口/月度池 -->
          <template v-if="platform === 'kimi'">
            <div class="flex items-baseline justify-between">
              <span class="text-sm text-muted-foreground">可用余额</span>
              <span class="text-2xl font-bold">
                {{
                  kimiDetail?.balance?.available != null
                    ? formatMoney(kimiDetail.balance.available)
                    : snapshot.remaining != null
                      ? formatMoney(snapshot.remaining)
                      : '—'
                }}
              </span>
            </div>
            <div
              v-if="kimiDetail?.balance?.voucher != null"
              class="flex justify-between text-sm text-muted-foreground"
            >
              <span>赠送额度</span>
              <span>{{ formatMoney(kimiDetail.balance.voucher) }}</span>
            </div>
            <div
              v-if="kimiDetail?.balance?.cash != null"
              class="flex justify-between text-sm text-muted-foreground"
            >
              <span>现金余额</span>
              <span>{{ formatMoney(kimiDetail.balance.cash) }}</span>
            </div>
            <!-- coding usages 是非官方 best-effort 接口：失败时降级提示，余额照常展示 -->
            <p v-if="kimiDetail?.codingError" class="text-xs text-muted-foreground">
              套餐用量暂不可用：{{ kimiDetail.codingError }}
            </p>
            <template v-else-if="kimiDetail?.coding">
              <div
                v-if="kimiDetail.coding.window_used_percent != null"
                class="flex justify-between text-sm text-muted-foreground"
              >
                <span>5 小时滚动窗口已用</span>
                <span>{{ kimiDetail.coding.window_used_percent.toFixed(1) }}%</span>
              </div>
              <div v-if="kimiDetail.coding.month_total != null" class="flex flex-col gap-1">
                <div class="flex justify-between text-sm text-muted-foreground">
                  <span>月度池</span>
                  <span>
                    已用
                    {{
                      kimiDetail.coding.month_used != null
                        ? formatTokens(kimiDetail.coding.month_used)
                        : '—'
                    }}
                    / {{ formatTokens(kimiDetail.coding.month_total) }}
                  </span>
                </div>
                <div
                  v-if="kimiMonthPercent != null"
                  class="h-2 w-full overflow-hidden rounded-full bg-muted"
                >
                  <div
                    class="h-full rounded-full bg-primary/70"
                    :style="{ width: `${kimiMonthPercent}%` }"
                  />
                </div>
              </div>
            </template>
          </template>

          <!-- MiMo：套餐进度条（本地推算） -->
          <template v-else-if="platform === 'mimo'">
            <div class="flex items-baseline justify-between">
              <span class="text-sm text-muted-foreground">本月已消耗</span>
              <span class="text-2xl font-bold">
                {{ snapshot.used != null ? formatTokens(Math.round(snapshot.used)) : '—' }}
                <span class="text-sm font-normal text-muted-foreground">Credit</span>
              </span>
            </div>
            <div v-if="mimoProgressPercent != null" class="flex flex-col gap-1">
              <div class="h-2 w-full overflow-hidden rounded-full bg-muted">
                <div
                  class="h-full rounded-full"
                  :class="
                    mimoDetail?.progress != null && mimoDetail.progress > 1
                      ? 'bg-destructive'
                      : 'bg-primary/70'
                  "
                  :style="{ width: `${mimoProgressPercent}%` }"
                />
              </div>
              <p class="text-xs text-muted-foreground">
                总额
                {{ snapshot.total != null ? formatTokens(Math.round(snapshot.total)) : '—' }} Credit
                · 已用 {{ mimoProgressPercent.toFixed(1) }}%
                <template v-if="mimoDetail?.progress != null && mimoDetail.progress > 1">
                  · 已超额
                </template>
              </p>
            </div>
            <p v-if="mimoDetail?.periodStart" class="text-xs text-muted-foreground">
              计费周期起点：{{ mimoDetail.periodStart }}
            </p>
          </template>

          <!-- DeepSeek：按量余额 + 币种分项 -->
          <template v-else>
            <div class="flex items-baseline justify-between">
              <span class="text-sm text-muted-foreground">账户余额</span>
              <span class="text-2xl font-bold">
                {{ snapshot.remaining != null ? formatMoney(snapshot.remaining) : '—' }}
              </span>
            </div>
            <div
              v-for="item in deepseekDetail?.balances ?? []"
              :key="item.currency ?? 'unknown'"
              class="flex justify-between text-sm text-muted-foreground"
            >
              <span>{{ item.currency }}</span>
              <span>{{ item.total != null ? item.total.toFixed(2) : '—' }}</span>
            </div>
          </template>

          <p class="text-xs text-muted-foreground">
            上次刷新：{{ formatDateTime(snapshot.fetched_at) }}
          </p>
        </template>

        <!-- 刷新失败按错误类别提示：unauthorized 引导去设置页，network 提示重试 -->
        <div v-else-if="error" class="flex flex-col items-start gap-2 text-sm text-destructive">
          <p v-if="error.kind === 'unauthorized'">凭据失效，请去设置页重新配置。</p>
          <p v-else-if="error.kind === 'network'">网络问题，可重试。</p>
          <p v-else>{{ error.message }}</p>
          <Button
            v-if="error.kind === 'unauthorized'"
            variant="outline"
            size="sm"
            @click="router.push('/settings')"
          >
            去设置页配置
          </Button>
        </div>
        <p v-else class="text-sm text-muted-foreground">暂无配额数据，点击右上角刷新拉取。</p>

        <!-- 有旧快照时刷新失败：保留旧数据，追加一行提示 -->
        <p v-if="snapshot && error" class="text-xs text-destructive">
          刷新失败：{{ error.message }}
        </p>
      </div>
    </CardContent>
  </Card>
</template>
