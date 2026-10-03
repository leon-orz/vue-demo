<script setup lang="ts">
import { RefreshCw } from '@lucide/vue'
import { computed } from 'vue'
import { useRouter } from 'vue-router'

import type { CommandError } from '@/api/errors'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatDateTime, formatMoney, formatTokens } from '@/lib/format'
import { parseKimiQuotaDetail, PLATFORM_LABELS } from '@/stores/usage'
import type { Platform, PlatformAccountInfo, QuotaSnapshot } from '@toogl/core-types'

/**
 * 平台配额/余额卡片（M1，当前只有 Kimi）：
 * 开放平台余额 + coding 5h 窗口 / 月度池（coding_usages_error 非空时降级提示，不影响余额展示），
 * 含「未配置」降级态。platform prop 保留泛化，其他平台回归时复用。
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

/** 「已配置」判定：看 keyring 凭据 */
const configured = computed(() => props.account?.has_credential === true)

const kimiDetail = computed(() =>
  props.platform === 'kimi' ? parseKimiQuotaDetail(props.snapshot) : null,
)

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
        v-if="configured"
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
      <!-- 未配置：引导去设置页 -->
      <div v-if="!configured" class="flex flex-col items-start gap-3 text-sm text-muted-foreground">
        <p>尚未配置 {{ label }} API Key，配置后可查询配额/余额。</p>
        <Button variant="outline" size="sm" @click="router.push('/settings')">去设置页配置</Button>
      </div>

      <div v-else class="flex flex-col gap-3">
        <template v-if="snapshot">
          <!-- Kimi：余额 + coding 窗口/月度池 -->
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
