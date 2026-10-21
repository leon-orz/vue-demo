import { computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'

/** 配置入口可直接链接；切换保留其他 query，浏览器返回也能还原标签。 */
export function useModuleTab() {
  const route = useRoute()
  const router = useRouter()
  return computed({
    get: () => (route.query.tab === 'config' ? 'config' : 'main'),
    set: (tab: string) => {
      const query = { ...route.query }
      if (tab === 'config') query.tab = 'config'
      else delete query.tab
      void router.replace({ query })
    },
  })
}
