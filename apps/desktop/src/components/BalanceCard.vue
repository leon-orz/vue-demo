<script setup lang="ts">
import { RefreshCw } from '@lucide/vue'
import { useRouter } from 'vue-router'

import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { formatDateTime, formatMoney } from '@/lib/format'
import { useKimiStore } from '@/stores/kimi'

const kimi = useKimiStore()
const router = useRouter()
</script>

<template>
  <Card>
    <CardHeader class="flex-row items-center justify-between space-y-0">
      <CardTitle class="text-base">开放平台余额</CardTitle>
      <Button
        v-if="kimi.account?.has_credential"
        variant="ghost"
        size="icon"
        :disabled="kimi.balanceLoading"
        title="刷新余额"
        @click="kimi.refreshBalance()"
      >
        <RefreshCw class="size-4" :class="kimi.balanceLoading ? 'animate-spin' : ''" />
      </Button>
    </CardHeader>
    <CardContent>
      <!-- 未配置 key：引导去设置页 -->
      <div
        v-if="!kimi.account?.has_credential"
        class="flex flex-col items-start gap-3 text-sm text-muted-foreground"
      >
        <p>尚未配置 Kimi 开放平台 API Key，配置后可查询账户余额。</p>
        <Button variant="outline" size="sm" @click="router.push('/settings')">去设置页配置</Button>
      </div>
      <div v-else class="flex flex-col gap-2">
        <template v-if="kimi.balance">
          <div class="flex items-baseline justify-between">
            <span class="text-sm text-muted-foreground">可用余额</span>
            <span class="text-2xl font-bold">{{ formatMoney(kimi.balance.available) }}</span>
          </div>
          <div class="flex justify-between text-sm text-muted-foreground">
            <span>赠送额度</span>
            <span>{{ formatMoney(kimi.balance.voucher) }}</span>
          </div>
          <div class="flex justify-between text-sm text-muted-foreground">
            <span>现金余额</span>
            <span>{{ formatMoney(kimi.balance.cash) }}</span>
          </div>
          <p class="mt-1 text-xs text-muted-foreground">
            上次刷新：{{ formatDateTime(kimi.balance.fetched_at) }}
          </p>
        </template>
        <!-- 刷新失败按错误类别提示：unauthorized 引导去设置页，network 提示重试 -->
        <div
          v-else-if="kimi.balanceError"
          class="flex flex-col items-start gap-2 text-sm text-destructive"
        >
          <p v-if="kimi.balanceError.kind === 'unauthorized'">凭据失效，请去设置页重新配置。</p>
          <p v-else-if="kimi.balanceError.kind === 'network'">网络问题，可重试。</p>
          <p v-else>{{ kimi.balanceError.message }}</p>
          <Button
            v-if="kimi.balanceError.kind === 'unauthorized'"
            variant="outline"
            size="sm"
            @click="router.push('/settings')"
          >
            去设置页配置
          </Button>
        </div>
        <p v-else class="text-sm text-muted-foreground">暂无余额数据，点击右上角刷新拉取。</p>
        <p v-if="kimi.balance && kimi.balanceError" class="text-xs text-destructive">
          刷新失败：{{ kimi.balanceError.message }}
        </p>
      </div>
    </CardContent>
  </Card>
</template>
