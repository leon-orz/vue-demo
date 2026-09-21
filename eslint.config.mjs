// ESLint 9 flat config：ESLint 只管规则，格式化交给 Prettier（见 .prettierrc.json）
import pluginVue from 'eslint-plugin-vue'
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import eslintConfigPrettier from 'eslint-config-prettier'

export default defineConfigWithVueTs(
  {
    ignores: [
      '**/node_modules/**',
      '**/dist/**',
      '**/target/**',
      '**/src-tauri/gen/**',
      'packages/core-types/bindings/**',
    ],
  },
  pluginVue.configs['flat/recommended'],
  vueTsConfigs.recommended,
  eslintConfigPrettier,
  {
    rules: {
      // 页面级组件允许单词命名（Dashboard / Settings 等）
      'vue/multi-word-component-names': 'off',
      // 约定：下划线前缀表示有意未使用
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
    },
  },
  {
    // shadcn-vue CLI 生成的组件源码：props 缺省值交给 withDefaults/cva，保持生成原样
    files: ['apps/desktop/src/components/ui/**'],
    rules: {
      'vue/require-default-prop': 'off',
    },
  },
)
