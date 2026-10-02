<script setup lang="ts">
import { onMounted, reactive, ref } from 'vue'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { toCommandError } from '@/api/errors'
import { useSettingsStore, type ThemeMode } from '@/stores/settings'
import { useUsageStore } from '@/stores/usage'
import type { Platform } from '@toogl/core-types'

const usage = useUsageStore()
const settings = useSettingsStore()

/**
 * 带 API Key 的平台（Kimi / DeepSeek）共用一份表单状态；
 * key 永不回显，只显示「已配置/未配置」，留空保存表示沿用 keyring 已有 key
 */
const keyForms = reactive({
  kimi: { name: '', apiBase: '', apiKey: '' },
  deepseek: { name: '', apiBase: '', apiKey: '' },
})
const keySaving = reactive<Record<'kimi' | 'deepseek', boolean>>({ kimi: false, deepseek: false })
const keyMessages = reactive<Record<'kimi' | 'deepseek', string>>({ kimi: '', deepseek: '' })

const KEY_PLATFORM_META: {
  platform: 'kimi' | 'deepseek'
  title: string
  apiBasePlaceholder: string
  description: string
}[] = [
  {
    platform: 'kimi',
    title: 'Kimi 开放平台',
    apiBasePlaceholder: 'https://api.moonshot.cn',
    description: '用于查询开放平台余额与套餐用量。API Key 只写入系统钥匙串，界面不回显。',
  },
  {
    platform: 'deepseek',
    title: 'DeepSeek',
    apiBasePlaceholder: 'https://api.deepseek.com',
    description: '用于查询 DeepSeek 按量余额。API Key 只写入系统钥匙串，界面不回显。',
  },
]

// MiMo 套餐手录表单（无公开 API，总额/周期人工录入，消耗由本地日志推算）
const mimoQuotaTotal = ref('')
const mimoQuotaCycle = ref('monthly')
const mimoSaving = ref(false)
const mimoMessage = ref('')

const themeOptions: { value: ThemeMode; label: string }[] = [
  { value: 'light', label: '亮色' },
  { value: 'dark', label: '暗色' },
  { value: 'system', label: '跟随系统' },
]

onMounted(async () => {
  await usage.loadAccounts()
  for (const meta of KEY_PLATFORM_META) {
    const account = usage.accounts[meta.platform]
    if (account) {
      keyForms[meta.platform].name = account.name
      keyForms[meta.platform].apiBase = account.api_base ?? ''
    }
  }
  const mimo = usage.accounts.mimo
  if (mimo) {
    mimoQuotaTotal.value = mimo.quota_total != null ? String(mimo.quota_total) : ''
    mimoQuotaCycle.value = mimo.quota_cycle ?? 'monthly'
  }
})

async function saveKeyAccount(platform: 'kimi' | 'deepseek') {
  const form = keyForms[platform]
  keySaving[platform] = true
  keyMessages[platform] = ''
  try {
    await usage.saveAccount(
      platform,
      {
        platform,
        name: form.name || '默认账户',
        api_base: form.apiBase || null,
        quota_total: null,
        quota_cycle: null,
        enabled: true,
      },
      // apiKey 留空表示沿用 keyring 中已有 key（见 platform_account_set 约定）
      form.apiKey || undefined,
    )
    form.apiKey = ''
    keyMessages[platform] = '已保存（API Key 已写入系统钥匙串）'
  } catch (err) {
    keyMessages[platform] = `保存失败：${toCommandError(err).message}`
  } finally {
    keySaving[platform] = false
  }
}

async function saveMimoAccount() {
  mimoSaving.value = true
  mimoMessage.value = ''
  try {
    const total = mimoQuotaTotal.value.trim()
    await usage.saveAccount('mimo', {
      platform: 'mimo',
      name: 'MiMo 套餐',
      api_base: null,
      quota_total: total === '' ? null : Number(total),
      quota_cycle: mimoQuotaCycle.value,
      enabled: true,
    })
    mimoMessage.value = '已保存，正在按本地日志推算套餐进度…'
    // 总额变更后立即重算进度（纯本地推算，不发网络请求）
    await usage.refreshQuota('mimo')
    mimoMessage.value = '已保存，套餐进度已按本地日志重新推算'
  } catch (err) {
    mimoMessage.value = `保存失败：${toCommandError(err).message}`
  } finally {
    mimoSaving.value = false
  }
}

/** 各平台「已配置」标记（徽章用）：kimi/deepseek 看 keyring 凭据，mimo 看手录总额 */
function configuredBadge(platform: Platform): boolean {
  if (platform === 'mimo') return usage.accounts[platform]?.quota_total != null
  return usage.accounts[platform]?.has_credential === true
}
</script>

<template>
  <div class="flex flex-col gap-6 p-6">
    <header>
      <h1 class="text-2xl font-bold tracking-tight">设置</h1>
      <p class="mt-1 text-sm text-muted-foreground">AI 用量平台与通用偏好配置。</p>
    </header>

    <!-- M6-01：AI 用量平台账户（Kimi / DeepSeek） -->
    <Card v-for="meta in KEY_PLATFORM_META" :key="meta.platform">
      <CardHeader>
        <div class="flex items-center gap-2">
          <CardTitle class="text-base">{{ meta.title }}</CardTitle>
          <Badge :variant="configuredBadge(meta.platform) ? 'default' : 'secondary'">
            {{ configuredBadge(meta.platform) ? '已配置' : '未配置' }}
          </Badge>
        </div>
        <CardDescription>{{ meta.description }}</CardDescription>
      </CardHeader>
      <CardContent class="flex max-w-lg flex-col gap-4">
        <div class="flex flex-col gap-1.5">
          <Label :for="`${meta.platform}-account-name`">账户名称</Label>
          <Input
            :id="`${meta.platform}-account-name`"
            v-model="keyForms[meta.platform].name"
            placeholder="默认账户"
          />
        </div>
        <div class="flex flex-col gap-1.5">
          <Label :for="`${meta.platform}-api-base`">API 地址</Label>
          <Input
            :id="`${meta.platform}-api-base`"
            v-model="keyForms[meta.platform].apiBase"
            :placeholder="meta.apiBasePlaceholder"
          />
        </div>
        <div class="flex flex-col gap-1.5">
          <Label :for="`${meta.platform}-api-key`">API Key</Label>
          <Input
            :id="`${meta.platform}-api-key`"
            v-model="keyForms[meta.platform].apiKey"
            type="password"
            :placeholder="
              usage.accounts[meta.platform]?.has_credential ? '已保存，留空则不修改' : 'sk-...'
            "
          />
        </div>
        <div class="flex items-center gap-3">
          <Button :disabled="keySaving[meta.platform]" @click="saveKeyAccount(meta.platform)">
            {{ keySaving[meta.platform] ? '保存中…' : '保存' }}
          </Button>
          <span v-if="keyMessages[meta.platform]" class="text-sm text-muted-foreground">
            {{ keyMessages[meta.platform] }}
          </span>
        </div>
      </CardContent>
    </Card>

    <!-- M6-01：MiMo 套餐（无公开 API，手录总额 + 本地日志推算消耗） -->
    <Card>
      <CardHeader>
        <div class="flex items-center gap-2">
          <CardTitle class="text-base">MiMo 套餐</CardTitle>
          <Badge :variant="configuredBadge('mimo') ? 'default' : 'secondary'">
            {{ configuredBadge('mimo') ? '已配置' : '未配置' }}
          </Badge>
        </div>
        <CardDescription>
          MiMo 无公开配额 API：手录套餐月度 Credit 总额与计费周期，消耗进度由本地日志推算。
        </CardDescription>
      </CardHeader>
      <CardContent class="flex max-w-lg flex-col gap-4">
        <div class="flex flex-col gap-1.5">
          <Label for="mimo-quota-total">月度 Credit 总额</Label>
          <Input
            id="mimo-quota-total"
            v-model="mimoQuotaTotal"
            type="number"
            min="0"
            placeholder="如 1000"
          />
        </div>
        <div class="flex flex-col gap-1.5">
          <Label>计费周期</Label>
          <Select v-model="mimoQuotaCycle">
            <SelectTrigger class="w-48">
              <SelectValue placeholder="选择计费周期" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="monthly">每月（自然月）</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <div class="flex items-center gap-3">
          <Button :disabled="mimoSaving" @click="saveMimoAccount">
            {{ mimoSaving ? '保存中…' : '保存' }}
          </Button>
          <span v-if="mimoMessage" class="text-sm text-muted-foreground">{{ mimoMessage }}</span>
        </div>
      </CardContent>
    </Card>

    <!-- M6-05：外观 -->
    <Card>
      <CardHeader>
        <CardTitle class="text-base">外观</CardTitle>
        <CardDescription>主题偏好保存在本机，立即生效。</CardDescription>
      </CardHeader>
      <CardContent class="max-w-lg">
        <div class="flex flex-col gap-1.5">
          <Label>主题</Label>
          <Select v-model="settings.theme">
            <SelectTrigger class="w-48">
              <SelectValue placeholder="选择主题" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="opt in themeOptions" :key="opt.value" :value="opt.value">
                {{ opt.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      </CardContent>
    </Card>

    <!-- 各平台日志目录覆盖（当前仅 kimi 支持，其余平台用默认目录） -->
    <Card>
      <CardHeader>
        <CardTitle class="text-base">日志目录覆盖</CardTitle>
        <CardDescription>
          Kimi：留空 = 自动发现（~/.kimi-code/sessions），修改后手动重新扫描生效。MiMo / omp
          暂不支持覆盖，使用默认目录。
        </CardDescription>
      </CardHeader>
      <CardContent class="flex max-w-lg flex-col gap-1.5">
        <Label for="kimi-sessions-dir">Kimi 日志目录</Label>
        <Input
          id="kimi-sessions-dir"
          v-model="settings.kimiSessionsDir"
          placeholder="留空使用默认目录"
        />
      </CardContent>
    </Card>

    <Separator />
    <p class="text-xs text-muted-foreground">
      数据源模式：{{ settings.datasourceMode }}（S2-2 可切换）
    </p>
  </div>
</template>
