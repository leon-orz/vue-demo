<script setup lang="ts">
import { Gauge, Power, TriangleAlert } from '@lucide/vue'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

import LoadErrorBanner from '@/components/LoadErrorBanner.vue'
import ProxyGroupCard from '@/components/ProxyGroupCard.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import { formatDateTime } from '@/lib/format'
import { RULE_MODE_LABELS, useProxyStore } from '@/stores/proxy'
import { useSubscriptionStore } from '@/stores/subscription'
import type { RuleMode } from '@toogl/core-types'

const proxy = useProxyStore()
const subscription = useSubscriptionStore()

// 进入页面开始轮询（mihomo 侧是事实源），离开页面停止；订阅状态并排轮询
onMounted(() => {
  proxy.startPolling()
  subscription.startPolling()
})
onUnmounted(() => {
  proxy.stopPolling()
  subscription.stopPolling()
})

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

// ---- M5-07 节点订阅卡片 ----

/** 订阅链接输入框；首次拉到已配置链接时回填（用户已输入则不覆盖） */
const urlInput = ref('')
watch(
  () => subscription.status?.url,
  (url) => {
    if (url && !urlInput.value) urlInput.value = url
  },
)

/** 手动粘贴导入的多行文本 */
const importTextInput = ref('')

/** 上次更新时间展示：null（从未成功）显示占位文案 */
const lastUpdatedText = computed(() => {
  const t = subscription.status?.last_updated
  return t ? formatDateTime(t) : '从未更新'
})

/** 导入失败明细的行内容截断，避免超长链接撑爆布局 */
function truncateLine(line: string, max = 80): string {
  return line.length > max ? `${line.slice(0, max)}…` : line
}

// 订阅动作成功后顺带刷新代理组列表（节点已热重载进 mihomo）
async function onSaveUrl() {
  const url = urlInput.value.trim()
  if (!url) return
  await subscription.saveUrl(url)
  if (!subscription.actionError) void proxy.refresh()
}

async function onUpdateNow() {
  await subscription.updateNow()
  if (!subscription.actionError) void proxy.refresh()
}

async function onImportText() {
  const text = importTextInput.value.trim()
  if (!text) return
  await subscription.importText(text)
  if (!subscription.actionError) {
    importTextInput.value = ''
    void proxy.refresh()
  }
}
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

    <!-- 节点订阅（M5-07）：订阅链接管理 + 手动粘贴导入 -->
    <Card>
      <CardHeader class="flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle class="text-base">节点订阅</CardTitle>
        <Badge variant="secondary">
          订阅 {{ subscription.status?.node_count ?? 0 }} · 手动
          {{ subscription.status?.manual_count ?? 0 }}
        </Badge>
      </CardHeader>
      <CardContent class="flex flex-col gap-4">
        <!-- 订阅状态轮询失败（invoke 层），可重试 -->
        <LoadErrorBanner
          v-if="subscription.loadError"
          :error="subscription.loadError"
          @retry="subscription.refresh()"
        />

        <!-- 订阅动作失败（保存链接/更新/导入），可关闭 -->
        <div
          v-if="subscription.actionError"
          class="flex items-center justify-between gap-4 rounded-md border border-destructive/50 bg-destructive/10 px-4 py-3 text-sm"
          role="alert"
        >
          <span class="text-destructive">操作失败：{{ subscription.actionError.message }}</span>
          <Button
            variant="ghost"
            size="sm"
            class="shrink-0"
            @click="subscription.actionError = null"
          >
            知道了
          </Button>
        </div>

        <!-- 上次更新失败告警：后端已保留旧节点，仅提示原因 -->
        <div
          v-if="subscription.status?.last_error"
          class="flex items-center gap-2 rounded-md border border-yellow-600/50 bg-yellow-600/10 px-4 py-3 text-sm text-yellow-700 dark:text-yellow-400"
          role="alert"
        >
          <TriangleAlert class="size-4 shrink-0" />
          订阅更新失败，已保留旧节点：{{ subscription.status.last_error }}
        </div>

        <div class="flex gap-2">
          <Input
            v-model="urlInput"
            placeholder="https://... 订阅链接"
            :disabled="subscription.updating"
          />
          <Button
            class="shrink-0"
            :disabled="subscription.updating || !urlInput.trim()"
            @click="onSaveUrl"
          >
            保存并更新
          </Button>
          <Button
            variant="outline"
            class="shrink-0"
            :disabled="subscription.updating || !subscription.status?.url"
            @click="onUpdateNow"
          >
            {{ subscription.updating ? '更新中…' : '立即更新' }}
          </Button>
        </div>

        <p class="text-xs text-muted-foreground">
          上次更新：{{ lastUpdatedText }} · 节点数：订阅
          {{ subscription.status?.node_count ?? 0 }} + 手动
          {{ subscription.status?.manual_count ?? 0 }}
        </p>

        <!-- 手动粘贴导入（折叠区）：逐行解析分享链接，坏行不拖垮整体 -->
        <details class="rounded-md border px-4 py-3">
          <summary class="cursor-pointer text-sm font-medium select-none">
            手动粘贴导入（vless / ss / trojan / hysteria2 分享链接，每行一条；暂不支持 vmess）
          </summary>
          <div class="mt-3 flex flex-col gap-3">
            <Textarea
              v-model="importTextInput"
              rows="5"
              placeholder="每行粘贴一条分享链接，或整段包含节点列表的文本"
              :disabled="subscription.importing"
            />
            <div class="flex items-center gap-3">
              <Button
                size="sm"
                :disabled="subscription.importing || !importTextInput.trim()"
                @click="onImportText"
              >
                {{ subscription.importing ? '导入中…' : '导入' }}
              </Button>
              <span v-if="subscription.lastImportResult" class="text-sm text-muted-foreground">
                已导入 {{ subscription.lastImportResult.imported }} 条
                <template v-if="subscription.lastImportResult.failures.length">
                  ，{{ subscription.lastImportResult.failures.length }} 条失败
                </template>
              </span>
            </div>
            <!-- 逐条失败明细：行内容截断 + 原因 -->
            <ul
              v-if="subscription.lastImportResult?.failures.length"
              class="flex max-h-40 flex-col gap-1 overflow-y-auto rounded-md border border-destructive/50 bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              <li v-for="(failure, i) in subscription.lastImportResult.failures" :key="i">
                <span class="break-all">{{ truncateLine(failure.line) }}</span>
                <span class="text-muted-foreground"> —— {{ failure.reason }}</span>
              </li>
            </ul>
          </div>
        </details>
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
