import { invoke } from '@tauri-apps/api/core'
import type { NodeDelayResult, ProxyStatus, RuleMode } from '@toogl/core-types'

/** 与 src-tauri/src/commands/proxy.rs 一一对应；参数/返回类型一律来自 ts-rs 生成物 */

/**
 * 聚合状态：sidecar 存活 + mihomo /proxies 摘要 + 规则模式 + 系统代理实时状态 + 残留自检。
 * 永远成功返回，各字段独立降级（mihomo 不可达时 proxies/rule_mode 为 null）。
 */
export function proxyStatus(): Promise<ProxyStatus> {
  return invoke('proxy_status')
}

/** 切换代理组选中节点（PUT /proxies/{group}）；mihomo 不可达时抛 network 类错误 */
export function selectProxy(group: string, node: string): Promise<void> {
  return invoke('select_proxy', { group, node })
}

/** 延迟测试：group 传 null/省略表示全部组；单节点失败只记入自身 error，不中断整批 */
export function testDelay(group?: string | null): Promise<NodeDelayResult[]> {
  return invoke('test_delay', { group: group ?? null })
}

/** 开/关系统代理（写 Windows 注册表；关闭时还原到开启前保存的旧值） */
export function setSystemProxy(enabled: boolean): Promise<void> {
  return invoke('set_system_proxy', { enabled })
}

/** 切换规则模式（rule / global / direct） */
export function switchRuleMode(mode: RuleMode): Promise<void> {
  return invoke('switch_rule_mode', { mode })
}

/** 残留恢复：上次异常退出导致系统代理残留开启时，还原到开启前保存的旧值 */
export function recoverSystemProxy(): Promise<void> {
  return invoke('recover_system_proxy')
}
