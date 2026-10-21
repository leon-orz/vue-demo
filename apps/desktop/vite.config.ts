import { fileURLToPath, URL } from 'node:url'

import tailwindcss from '@tailwindcss/vite'
import vue from '@vitejs/plugin-vue'
import { defineConfig } from 'vite'

const host = process.env.TAURI_DEV_HOST

// Tauri 适配：固定 dev 端口 1420、strictPort、clearScreen: false
// https://v2.tauri.app/start/frontend/vite/
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  clearScreen: false,
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      // src-tauri 变更不触发前端 reload
      ignored: ['**/src-tauri/**'],
    },
  },
  build: {
    // 仅支持 Windows，WebView2 跟随系统 Chromium 内核
    target: 'chrome120',
  },
})
