<script setup lang="ts">
import { Gauge, Power, TriangleAlert } from '@lucide/vue'
import { computed, onMounted, onUnmounted } from 'vue'

import LoadErrorBanner from '@/components/LoadErrorBanner.vue'
import ProxyGroupCard from '@/components/ProxyGroupCard.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { RULE_MODE_LABELS, useProxyStore } from '@/stores/proxy'
import type { RuleMode } from '@toogl/core-types'

const proxy = useProxyStore()

// 进入页面开始轮询（mihomo 侧是事实源），离开页面停止
onMounted(() => proxy.startPolling())
onUnmounted(() => proxy.stopPolling())

const modeTabs: { value: RuleMode; label: string }[] = [
  { value: 'rule', label: RULE_MODE_LABELS.rule },
  { value: 'global', label: RULE_MODE_LABELS.global },
  { value: 'direct', label: RULE_MODE_LABELS.direct },
]

/** sidecar 存活探测（实时 /version）；启动失败时 startup_error 非空 */
const sidecarAlive = computed(() => proxy.status?.sidecar_alive ?? false)

const systemProxyEnabled = computed(() => proxy.status?.system_proxy?.enabled ?? false)

/** 残留提示中「将恢复到的状态」文案 */
const leftoverRestoreText = computed(() => {
  const prev = proxy.status?.leftover_previous
  if (!prev) return '将恢复到系统代理关闭'
  return prev.enabled ? `将恢复到开启（${prev.server ?? '未知地址'}）` : '将恢复到关闭'
})
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header class="flex items-center justify-between">
      <div>
        <h1 class="text-2xl font-bold tracking-tight">魔法工具</h1>
        <p class="mt-1 text-sm text-muted-foreground">
          节点切换、规则模式与系统代理；状态每几秒与 mihomo 自动同步。
        </p>
      </div>
      <Button
        variant="outline"
        :disabled="proxy.testingAll || !sidecarAlive"
        @click="proxy.testDelay()"
      >
        <Gauge class="size-4" :class="proxy.testingAll ? 'animate-pulse' : ''" />
        {{ proxy.testingAll ? '测速中…' : '全部测速' }}
      </Button>
    </header>

    <!-- 残留提示：上次异常退出导致系统代理残留开启 -->
    <div
      v-if="proxy.leftoverVisible"
      class="flex items-center justify-between gap-4 rounded-md border border-yellow-600/50 bg-yellow-600/10 px-4 py-3 text-sm"
      role="alert"
    >
      <span class="flex items-center gap-2 text-yellow-700 dark:text-yellow-400">
        <TriangleAlert class="size-4 shrink-0" />
        检测到上次异常退出，系统代理残留开启。{{ leftoverRestoreText }}。
      </span>
      <span class="flex shrink-0 gap-2">
        <Button
          variant="outline"
          size="sm"
          :disabled="proxy.recovering"
          @click="proxy.recoverLeftover()"
        >
          {{ proxy.recovering ? '恢复中…' : '恢复' }}
        </Button>
        <Button variant="ghost" size="sm" @click="proxy.dismissLeftover()">忽略</Button>
      </span>
    </div>

    <!-- sidecar 启动失败降级横幅 -->
    <div
      v-if="proxy.status?.startup_error"
      class="flex items-center gap-2 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm text-destructive"
      role="alert"
    >
      <TriangleAlert class="size-4 shrink-0" />
      mihomo sidecar 启动失败：{{ proxy.status.startup_error }}
    </div>

    <!-- 状态轮询本身失败（invoke 层），可重试 -->
    <LoadErrorBanner v-if="proxy.loadError" :error="proxy.loadError" @retry="proxy.refresh()" />

    <!-- 动作失败提示（切换/测速/开关），可关闭 -->
    <div
      v-if="proxy.actionError"
      class="flex items-center justify-between gap-4 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm"
      role="alert"
    >
      <span class="text-destructive">操作失败：{{ proxy.actionError.message }}</span>
      <Button variant="ghost" size="sm" class="shrink-0" @click="proxy.actionError = null">
        知道了
      </Button>
    </div>

    <!-- 顶部状态区：sidecar 状态 + 规则模式 + 系统代理 -->
    <Card>
      <CardHeader class="flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle class="text-base">代理状态</CardTitle>
        <Badge :variant="sidecarAlive ? 'secondary' : 'destructive'">
          <Power class="size-3" />
          {{ sidecarAlive ? `运行中 · 端口 ${proxy.status?.controller_port ?? '?'}` : '未运行' }}
        </Badge>
      </CardHeader>
      <CardContent class="flex flex-col gap-4">
        <div class="flex items-center justify-between gap-4">
          <div>
            <p class="text-sm font-medium">规则模式</p>
            <p class="text-xs text-muted-foreground">
              规则按分流配置走代理；全局全部走代理；直连不走代理。
            </p>
          </div>
          <Tabs
            :model-value="proxy.status?.rule_mode ?? undefined"
            @update:model-value="proxy.switchMode($event as RuleMode)"
          >
            <TabsList>
              <TabsTrigger
                v-for="tab in modeTabs"
                :key="tab.value"
                :value="tab.value"
                :disabled="proxy.switchingMode"
              >
                {{ tab.label }}
              </TabsTrigger>
            </TabsList>
          </Tabs>
        </div>
        <div class="flex items-center justify-between gap-4">
          <div>
            <p class="text-sm font-medium">系统代理</p>
            <p class="text-xs text-muted-foreground">
              <template v-if="proxy.status?.system_proxy">
                {{
                  proxy.status.system_proxy.enabled
                    ? `已开启（${proxy.status.system_proxy.server ?? '未知地址'}）`
                    : '已关闭'
                }}
              </template>
              <template v-else>系统代理状态读取失败</template>
            </p>
          </div>
          <Switch
            :model-value="systemProxyEnabled"
            :disabled="proxy.togglingSystemProxy || proxy.status?.system_proxy == null"
            @update:model-value="proxy.setSystemProxy($event)"
          />
        </div>
      </CardContent>
    </Card>

    <!-- 代理组列表：sidecar 不可用时降级为可读提示 -->
    <template v-if="proxy.sidecarOk">
      <section v-if="proxy.groups.length" class="flex flex-col gap-4">
        <ProxyGroupCard v-for="group in proxy.groups" :key="group.name" :group="group" />
      </section>
      <p
        v-else
        class="rounded-md border border-dashed px-4 py-8 text-center text-sm text-muted-foreground"
      >
        暂无代理组。请检查 mihomo 配置是否已导入节点。
      </p>
    </template>
    <p
      v-else-if="proxy.status"
      class="rounded-md border border-dashed px-4 py-8 text-center text-sm text-muted-foreground"
    >
      mihomo sidecar 不可用，代理组与节点信息暂不可读。请排查上方提示的启动问题后稍候自动恢复。
    </p>
  </div>
</template>
