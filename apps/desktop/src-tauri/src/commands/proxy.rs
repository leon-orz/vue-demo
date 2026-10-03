//! M5 魔法工具命令层（S1-3 项3）：薄壳，业务逻辑在 toogl_core（mihomo / sysproxy /
//! proxy_setting）。前端对应封装见 apps/desktop/src/api/proxy.ts。
//! 所有命令统一返回 Result<T, CommandError>，前端 invoke 抛出的即 CommandError JSON。
//!
//! 桌面端仅 Windows（NF-14）：系统代理直接装配 WinRegIo 真实注册表实现，
//! 本模块不做跨平台 cfg。写注册表只发生在用户显式开关与退出/恢复路径，
//! 测试一律走 core 侧 mock（本文件不写触碰真实注册表的测试）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::State;
use toogl_core::mihomo::{
    collect_test_node_names, MihomoError, DEFAULT_DELAY_TIMEOUT_MS, DEFAULT_TEST_URL,
};
use toogl_core::models::{
    CommandError, NodeDelayResult, ProxySelection, ProxyStatus, RuleMode, SystemProxyCheck,
    SystemProxyStatus,
};
use toogl_core::proxy_setting::{
    ProxySettingStore, SqliteProxyStateStore, KEY_RULE_MODE, KEY_SELECTION,
};
use toogl_core::sysproxy::{SystemProxyManager, WinRegIo};

use crate::sidecar::{self, ProxyCoreState};

/// 桌面端系统代理管理器装配类型：真实注册表 + SQLite 备份存储
type DesktopSysProxyManager = SystemProxyManager<WinRegIo, SqliteProxyStateStore>;

/// proxy 命令层共享状态。初始化失败（设置库打不开）降级：sysproxy/settings 为 None，
/// 写操作命令统一返回 unavailable，proxy_status 仍可返回 sidecar 侧字段。
pub struct ProxyCommandState {
    /// proxy_setting KV（记住节点选择/规则模式）
    settings: Option<ProxySettingStore>,
    /// 系统代理管理器（注册表 IO + SQLite 备份）；初始化失败为 None
    sysproxy: Option<Arc<DesktopSysProxyManager>>,
    /// 启动自检结果（首个 proxy_status / set_system_proxy 调用时惰性执行一次并缓存；
    /// recover/set_system_proxy(false) 成功后清除）。
    /// 自检有副作用（静默清理失效备份），不适合在 proxy_status 里反复跑，故缓存。
    startup_check: Mutex<Option<SystemProxyCheck>>,
    /// 保证启动自检全局只跑一次（见 ensure_startup_check）
    startup_check_once: tokio::sync::OnceCell<()>,
    /// 系统代理开关指向的本地 mixed 端口（取值优先级见 sidecar::resolve_mixed_port：
    /// proxy_setting 持久化值 → config.yaml → 常量 7890）
    mixed_port: u16,
    /// 初始化失败原因（降级态）
    init_error: Option<String>,
}

impl ProxyCommandState {
    /// 应用 setup 时构造：打开 proxy_setting 库、装配系统代理管理器。
    /// 任何失败收敛为降级态，不 panic（参照 UsageState 的降级模式）。
    ///
    /// 纪律：本函数在 setup 里经 `tauri::async_runtime::block_on` 驱动，处于 runtime
    /// 上下文，**禁止在此直接调用 manager 的同步方法**——SqliteProxyStateStore 内部
    /// 的专用 runtime block_on 会 panic（Cannot start a runtime from within a runtime）。
    /// 启动自检因此挪到首个 proxy_status / set_system_proxy 调用时经 spawn_blocking
    /// 惰性执行（[`Self::ensure_startup_check`]）。
    pub async fn new(app_data_dir: PathBuf) -> Self {
        let mixed_port = sidecar::resolve_mixed_port(&app_data_dir).await;
        match ProxySettingStore::connect(app_data_dir.join("toogl.db")).await {
            Ok(settings) => {
                let manager: DesktopSysProxyManager = SystemProxyManager::with_system_registry(
                    SqliteProxyStateStore::new(settings.clone()),
                );
                Self {
                    settings: Some(settings),
                    sysproxy: Some(Arc::new(manager)),
                    startup_check: Mutex::new(None),
                    startup_check_once: tokio::sync::OnceCell::new(),
                    mixed_port,
                    init_error: None,
                }
            }
            Err(e) => Self::degraded(format!("proxy 设置库打开失败: {e}"), mixed_port),
        }
    }

    /// 降级态（连 mixed 端口都读不到时用常量兜底）
    pub fn unavailable(reason: String) -> Self {
        Self::degraded(reason, sidecar::MIXED_PORT)
    }

    fn degraded(reason: String, mixed_port: u16) -> Self {
        Self {
            settings: None,
            sysproxy: None,
            startup_check: Mutex::new(None),
            startup_check_once: tokio::sync::OnceCell::new(),
            mixed_port,
            init_error: Some(reason),
        }
    }

    fn settings(&self) -> Option<&ProxySettingStore> {
        self.settings.as_ref()
    }

    fn require_sysproxy(&self) -> Result<Arc<DesktopSysProxyManager>, CommandError> {
        self.sysproxy.clone().ok_or_else(|| {
            CommandError::unavailable(format!(
                "系统代理模块初始化失败: {}",
                self.init_error.as_deref().unwrap_or("未知原因")
            ))
        })
    }

    /// 惰性启动自检：首个 proxy_status / set_system_proxy 调用时执行一次并缓存结果
    ///（OnceCell 保证只跑一次；自检有清失效备份的副作用，不能反复跑）。
    /// 识别"上次异常退出残留的系统代理开启"，结果经 proxy_status 暴露给前端；
    /// 恢复由前端确认后调 recover_system_proxy。
    /// 必须在异步命令上下文调用：manager 同步方法经 spawn_blocking 进 blocking 线程池，
    /// 该线程不在 runtime 上下文，SqliteProxyStateStore 内部 block_on 才安全。
    async fn ensure_startup_check(&self) {
        let Some(manager) = self.sysproxy.clone() else {
            return;
        };
        self.startup_check_once
            .get_or_init(|| async move {
                let check = tauri::async_runtime::spawn_blocking(move || manager.startup_check())
                    .await
                    .map_err(|e| eprintln!("[proxy] 系统代理启动自检任务失败: {e}"))
                    .ok()
                    .and_then(|r| match r {
                        Ok(check) => Some(check),
                        Err(e) => {
                            eprintln!("[proxy] 系统代理启动自检失败（按无残留处理）: {e}");
                            None
                        }
                    });
                if check.as_ref().is_some_and(|c| c.leftover) {
                    eprintln!("[proxy] 检测到上次异常退出的系统代理残留，等待前端确认恢复");
                }
                if let Ok(mut g) = self.startup_check.lock() {
                    *g = check;
                }
            })
            .await;
    }

    /// 缓存的启动自检残留标记（proxy_status 暴露用）
    fn leftover_check(&self) -> Option<SystemProxyCheck> {
        self.startup_check.lock().ok().and_then(|g| g.clone())
    }

    /// 残留已处理（recover / 手动关闭成功）后清除提示状态
    fn clear_leftover(&self) {
        if let Ok(mut g) = self.startup_check.lock() {
            *g = None;
        }
    }

    /// 退出钩子（RunEvent::Exit，同步上下文）：若本次运行由我们开启了系统代理
    /// （备份存在且注册表仍开着 = startup_check 的 leftover 语义），退出前无损恢复，
    /// 保证正常退出不留残留；异常退出由下次启动自检兜底。
    /// 要求快速：一次注册表读 + 必要时一次还原写；失败只记录，绝不阻塞退出。
    pub fn cleanup_system_proxy_on_exit(&self) {
        let Some(manager) = &self.sysproxy else {
            return;
        };
        match manager.startup_check() {
            Ok(check) if check.leftover => {
                if let Err(e) = manager.recover() {
                    eprintln!("[proxy] 退出时恢复系统代理失败（下次启动自检兜底）: {e}");
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("[proxy] 退出时系统代理自检失败: {e}"),
        }
    }
}

/// MihomoError → CommandError：不可达归 network，其余归 internal
fn mihomo_err(e: MihomoError) -> CommandError {
    match e {
        MihomoError::Unreachable(m) => CommandError::network(m),
        MihomoError::Api(m) | MihomoError::Parse(m) => CommandError::internal(m),
    }
}

/// M5 状态总览：sidecar 存活/端口/启动错误 + mihomo /proxies 摘要 + 规则模式 +
/// 系统代理状态 + 启动自检残留标记 + 记住的节点选择。各字段独立降级，整体不报错。
#[tauri::command]
pub async fn proxy_status(
    core: State<'_, ProxyCoreState>,
    state: State<'_, ProxyCommandState>,
) -> Result<ProxyStatus, CommandError> {
    let alive = core.alive().await;

    // 惰性启动自检（只跑一次，见 ensure_startup_check）
    state.ensure_startup_check().await;

    // mihomo 侧字段：sidecar 不可用/不可达时降级为 None，不算错误
    let (proxies, rule_mode) = match (alive, core.client().ok()) {
        (true, Some(client)) => {
            let proxies = client
                .get_proxies()
                .await
                .map_err(|e| eprintln!("[proxy] /proxies 读取失败: {e}"))
                .ok();
            let mode = client
                .get_mode()
                .await
                .map_err(|e| eprintln!("[proxy] /configs 读取失败: {e}"))
                .ok();
            (proxies, mode)
        }
        _ => (None, None),
    };

    // 规则模式降级：mihomo 读不到时用上次切换记住的模式
    let rule_mode = match rule_mode {
        Some(mode) => Some(mode),
        None => match state.settings() {
            Some(settings) => settings
                .get(KEY_RULE_MODE)
                .await
                .ok()
                .flatten()
                .and_then(|s| RuleMode::parse(&s)),
            None => None,
        },
    };

    // 系统代理状态：注册表实时读（同步 IO，扔进 blocking 线程池）
    let system_proxy: Option<SystemProxyStatus> = match state.sysproxy.clone() {
        Some(manager) => tauri::async_runtime::spawn_blocking(move || {
            manager
                .status()
                .map_err(|e| eprintln!("[proxy] 系统代理状态读取失败: {e}"))
                .ok()
        })
        .await
        .ok()
        .flatten(),
        None => None,
    };

    // 记住的节点选择（UI 初始展示兜底）
    let remembered_selection: Option<ProxySelection> = match state.settings() {
        Some(settings) => settings
            .get(KEY_SELECTION)
            .await
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok()),
        None => None,
    };

    let check = state.leftover_check();
    Ok(ProxyStatus {
        sidecar_alive: alive,
        controller_port: core.port(),
        mixed_port: state.mixed_port,
        startup_error: core.startup_error().map(str::to_string),
        proxies,
        rule_mode,
        system_proxy,
        system_proxy_leftover: check.as_ref().is_some_and(|c| c.leftover),
        leftover_previous: check.and_then(|c| c.previous),
        remembered_selection,
    })
}

/// 切换 Selector 组节点；成功后记住选择（写 proxy_setting，供 UI 初始展示兜底）。
/// 记住失败只记录不回滚——mihomo 侧切换已生效，事实源在 mihomo。
#[tauri::command]
pub async fn select_proxy(
    core: State<'_, ProxyCoreState>,
    state: State<'_, ProxyCommandState>,
    group: String,
    node: String,
) -> Result<(), CommandError> {
    core.client()?
        .select_proxy(&group, &node)
        .await
        .map_err(mihomo_err)?;
    if let Some(settings) = state.settings() {
        let selection = ProxySelection { group, node };
        match serde_json::to_string(&selection) {
            Ok(json) => {
                if let Err(e) = settings.set(KEY_SELECTION, &json).await {
                    eprintln!("[proxy] 记住节点选择失败（切换已生效）: {e}");
                }
            }
            Err(e) => eprintln!("[proxy] 节点选择序列化失败（切换已生效）: {e}"),
        }
    }
    Ok(())
}

/// 批量测速：group 不传测所有 Selector/URLTest 组的节点（排除 GLOBAL，去重）；
/// 传组名只测该组。单节点失败只记入自身结果，不中断整批。
#[tauri::command]
pub async fn test_delay(
    core: State<'_, ProxyCoreState>,
    group: Option<String>,
) -> Result<Vec<NodeDelayResult>, CommandError> {
    let client = core.client()?;
    let summary = client.get_proxies().await.map_err(mihomo_err)?;
    let names =
        collect_test_node_names(&summary, group.as_deref()).map_err(CommandError::not_found)?;
    Ok(client
        .test_delays(&names, DEFAULT_DELAY_TIMEOUT_MS, DEFAULT_TEST_URL)
        .await)
}

/// 系统代理开关。开启指向 127.0.0.1:<mixed-port>（mihomo 本地混合入口，见
/// sidecar::resolve_mixed_port 的取值优先级——注意不是 external-controller 端口）。
/// 语义（core::sysproxy）：开启仅在无备份时保存旧现场；关闭有备份精确还原、
/// 无备份只清 ProxyEnable。注册表为同步 IO，经 spawn_blocking 执行。
/// 开启前先确保启动自检已跑：其副作用会清理失效备份，避免旧备份挡住本次现场保存。
#[tauri::command]
pub async fn set_system_proxy(
    state: State<'_, ProxyCommandState>,
    enabled: bool,
) -> Result<(), CommandError> {
    state.ensure_startup_check().await;
    let manager = state.require_sysproxy()?;
    let server = format!("127.0.0.1:{}", state.mixed_port);
    tauri::async_runtime::spawn_blocking(move || {
        if enabled {
            manager.enable(&server)
        } else {
            manager.disable()
        }
    })
    .await
    .map_err(|e| CommandError::internal(format!("系统代理任务执行失败: {e}")))?
    .map_err(CommandError::from)?;
    if !enabled {
        // 关闭成功即无残留（disable 已还原/清除备份），清掉提示状态
        state.clear_leftover();
    }
    Ok(())
}

/// 规则模式切换（rule/global/direct）；成功后记住模式（mihomo 不可达时的降级展示用）。
#[tauri::command]
pub async fn switch_rule_mode(
    core: State<'_, ProxyCoreState>,
    state: State<'_, ProxyCommandState>,
    mode: RuleMode,
) -> Result<(), CommandError> {
    core.client()?.set_mode(mode).await.map_err(mihomo_err)?;
    if let Some(settings) = state.settings() {
        if let Err(e) = settings.set(KEY_RULE_MODE, mode.as_str()).await {
            eprintln!("[proxy] 记住规则模式失败（切换已生效）: {e}");
        }
    }
    Ok(())
}

/// 残留恢复：前端确认后调用，按备份无损还原系统代理并清除残留标记。
#[tauri::command]
pub async fn recover_system_proxy(state: State<'_, ProxyCommandState>) -> Result<(), CommandError> {
    let manager = state.require_sysproxy()?;
    tauri::async_runtime::spawn_blocking(move || manager.recover())
        .await
        .map_err(|e| CommandError::internal(format!("系统代理恢复任务执行失败: {e}")))?
        .map_err(CommandError::from)?;
    state.clear_leftover();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mihomo_error_maps_to_command_error() {
        assert_eq!(
            mihomo_err(MihomoError::Unreachable("拒连".into())).kind,
            "network"
        );
        assert_eq!(mihomo_err(MihomoError::Api("404".into())).kind, "internal");
        assert_eq!(
            mihomo_err(MihomoError::Parse("坏包".into())).kind,
            "internal"
        );
    }

    #[test]
    fn degraded_state_reports_unavailable() {
        let state = ProxyCommandState::unavailable("库打不开".into());
        let err = state.require_sysproxy().err().unwrap();
        assert_eq!(err.kind, "unavailable");
        assert!(err.message.contains("库打不开"));
        assert!(state.settings().is_none());
        assert!(state.leftover_check().is_none());
        // 降级态退出钩子为 no-op，不 panic
        state.cleanup_system_proxy_on_exit();
    }

    /// 回归锁（修复 setup 嵌套 block_on panic）：ProxyCommandState::new 在 runtime
    /// 上下文（tauri::async_runtime::block_on，即生产 setup 的驱动方式）里构造不再
    /// panic——启动自检已挪出构造路径，改为首个命令调用时经 spawn_blocking 惰性执行。
    /// 构造只碰 SQLite（临时目录），不读注册表。
    #[test]
    fn new_inside_block_on_context_does_not_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let state =
            tauri::async_runtime::block_on(ProxyCommandState::new(tmp.path().to_path_buf()));
        assert!(state.sysproxy.is_some());
        // 构造完成时自检尚未执行（惰性），无残留提示
        assert!(state.leftover_check().is_none());
    }

    #[test]
    fn selection_json_roundtrip() {
        // 记住选择的持久化格式（KEY_SELECTION 的值）前后端契约自测
        let selection = ProxySelection {
            group: "节点选择".into(),
            node: "香港 01".into(),
        };
        let json = serde_json::to_string(&selection).unwrap();
        let parsed: ProxySelection = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, selection);
    }
}
