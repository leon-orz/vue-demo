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
    rollupOptions: {
      output: {
        // Vite 8（Rolldown）：ECharts 单独分包，避免拖慢首屏（Kimi 页已做路由懒加载）
        advancedChunks: {
          groups: [{ name: 'echarts', test: /node_modules[\\/]echarts/ }],
        },
      },
    },
    // TODO(S1-3)：mihomo 相关模块接入后视体积再评估分包
  },
})
