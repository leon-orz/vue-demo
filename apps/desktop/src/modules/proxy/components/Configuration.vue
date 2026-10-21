<script setup lang="ts">
import { TriangleAlert } from '@lucide/vue'
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'

import LoadErrorBanner from '@/shared/components/LoadErrorBanner.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { formatDateTime } from '@/lib/format'
import { useProxyStore } from '../store'
import { useSubscriptionStore } from '../subscription'

const proxy = useProxyStore()
const subscription = useSubscriptionStore()
// 配置页只同步状态，24h 订阅更新在 Rust 后台独立执行。
onMounted(() => subscription.startPolling())
onUnmounted(() => subscription.stopPolling())

// ---- M5-07 节点订阅卡片 ----

/** 订阅链接输入框；首次拉到已配置链接时回填（用户已输入则不覆盖） */
const urlInput = ref('')
watch(
  () => subscription.status?.url,
  (url) => {
    if (url && !urlInput.value) urlInput.value = url
  },
  // 标签切回时 Store 已有缓存，链接不变也需要回填新的表单实例。
  { immediate: true },
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
        <Button variant="ghost" size="sm" class="shrink-0" @click="subscription.actionError = null">
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
        上次更新：{{ lastUpdatedText }} · 节点数：订阅 {{ subscription.status?.node_count ?? 0 }} +
        手动
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
</template>
