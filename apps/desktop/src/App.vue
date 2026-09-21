<script setup lang="ts">
import { ref } from 'vue'

import { keystoreDelete, keystoreGet, keystoreSet } from '@/api/keyring'
import { useSettingsStore } from '@/stores/settings'

const settings = useSettingsStore()

const keyInput = ref('toogl.dev.test')
const valueInput = ref('hello-keyring')
const result = ref('')
const busy = ref(false)

async function run(action: () => Promise<string>) {
  busy.value = true
  try {
    result.value = await action()
  } catch (err) {
    result.value = `失败：${String(err)}`
  } finally {
    busy.value = false
  }
}

const set = () =>
  run(async () => {
    await keystoreSet(keyInput.value, valueInput.value)
    return `已写入钥匙串：${keyInput.value}`
  })

const get = () =>
  run(async () => {
    const value = await keystoreGet(keyInput.value)
    return value === null ? '钥匙串中不存在该 key' : `读回：${value}`
  })

const del = () =>
  run(async () => {
    await keystoreDelete(keyInput.value)
    return `已删除：${keyInput.value}`
  })
</script>

<template>
  <main class="flex min-h-screen flex-col items-center justify-center gap-6 bg-background p-8">
    <header class="text-center">
      <h1 class="text-4xl font-bold tracking-tight">Toogl · 图格</h1>
      <p class="mt-2 text-muted-foreground">个人桌面工具箱 —— S1-1 脚手架验证壳</p>
    </header>

    <section class="w-full max-w-md rounded-lg border bg-card p-6 text-card-foreground shadow-sm">
      <h2 class="mb-4 text-lg font-semibold">钥匙串存取验证（keyring-rs）</h2>
      <div class="flex flex-col gap-3">
        <label class="text-sm">
          Key
          <input
            v-model="keyInput"
            class="mt-1 w-full rounded-md border bg-background px-3 py-1.5 text-sm"
          />
        </label>
        <label class="text-sm">
          Value
          <input
            v-model="valueInput"
            class="mt-1 w-full rounded-md border bg-background px-3 py-1.5 text-sm"
          />
        </label>
        <div class="flex gap-2">
          <button
            :disabled="busy"
            class="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground disabled:opacity-50"
            @click="set"
          >
            写入
          </button>
          <button
            :disabled="busy"
            class="rounded-md border px-3 py-1.5 text-sm disabled:opacity-50"
            @click="get"
          >
            读取
          </button>
          <button
            :disabled="busy"
            class="rounded-md border px-3 py-1.5 text-sm text-destructive disabled:opacity-50"
            @click="del"
          >
            删除
          </button>
        </div>
        <p v-if="result" class="rounded-md bg-muted px-3 py-2 text-sm">{{ result }}</p>
      </div>
    </section>

    <footer class="text-xs text-muted-foreground">
      数据源模式：{{ settings.datasourceMode }}（S1-1 占位）
    </footer>
  </main>
</template>
