// ESLint 9 flat config：ESLint 只管规则，格式化交给 Prettier（见 .prettierrc.json）
import { defineConfigWithVueTs, vueTsConfigs } from '@vue/eslint-config-typescript'
import eslintConfigPrettier from 'eslint-config-prettier'

export default defineConfigWithVueTs(
  {
    ignores: ['**/node_modules/**', '**/dist/**', '**/target/**'],
  },
  vueTsConfigs.recommended,
  eslintConfigPrettier,
  {
    rules: {
      // 约定：下划线前缀表示有意未使用
      '@typescript-eslint/no-unused-vars': [
        'error',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_' },
      ],
    },
  },
)
