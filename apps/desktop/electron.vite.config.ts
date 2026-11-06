import { defineConfig } from 'electron-vite'
import vue from '@vitejs/plugin-vue'

/**
 * electron-vite 6:main/preload 段必须显式声明(可为空对象),
 * 入口按约定取 src/main/index.ts 与 src/preload/index.ts
 */
export default defineConfig({
  main: {},
  preload: {},
  renderer: {
    plugins: [vue()]
  }
})
