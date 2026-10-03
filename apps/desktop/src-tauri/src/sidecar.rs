//! mihomo sidecar 进程生命周期管理（S1-3 项2 落地）。
//!
//! 职责：
//! - 应用 setup 时拉起 mihomo 子进程（-d 工作目录 -f 配置文件），退出时（RunEvent::Exit）回收，
//!   确保不留孤儿进程；
//! - external-controller 端口冲突顺延：9090 被占则探测 9091…9100，实际端口写入
//!   proxy_setting 表（toogl_core::proxy_setting，与 UsageService 同一库文件）；
//! - 首次运行生成最小可用配置（app_data_dir/mihomo/config.yaml），已有配置不覆盖；
//! - 拉起失败（二进制缺失 / 进程秒退 / 就绪超时）降级为错误态：proxy 模块不可用，
//!   应用其余部分照常（参照 commands/usage.rs UsageState 的降级模式）。
//!
//! 状态经 [`ProxyCoreState`] 暴露给项3命令层（节点切换 / 系统代理开关 / 规则组切换）；
//! 系统代理启动自检（sysproxy startup_check）的装配也在项3做，本模块只提供 sidecar 侧。

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt as _;
use toogl_core::mihomo::MihomoClient;
use toogl_core::models::CommandError;
use toogl_core::proxy_setting::{ProxySettingStore, KEY_CONTROLLER_PORT};

/// external-controller 探测区间：默认 9090，被占顺延，9100 封顶
pub const CONTROLLER_PORT_START: u16 = 9090;
pub const CONTROLLER_PORT_END: u16 = 9100;
/// 初始配置模板中的本地混合代理端口（HTTP + SOCKS5 入口）
pub const MIXED_PORT: u16 = 7890;
/// 拉起后就绪等待上限（轮询 mihomo /version）
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// 就绪轮询间隔
const READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// 失败原因中附带的 mihomo 输出截断长度（防超长日志）
const OUTPUT_SNIPPET_MAX: usize = 500;

/// 代理核心（M5）共享状态。sidecar 拉起失败时降级为错误态（存原因），
/// 命令层统一返回可读错误，不影响应用其余模块。
pub struct ProxyCoreState {
    inner: Result<ProxyCore, String>,
}

/// sidecar 拉起成功后的运行态
pub struct ProxyCore {
    /// external-controller 实际端口（顺延后的真实值）
    port: u16,
    client: MihomoClient,
    /// 子进程句柄：退出时 take 出来 kill，确保不留孤儿进程
    child: Mutex<Option<CommandChild>>,
}

impl ProxyCoreState {
    /// 构造降级错误态（拉起失败原因）
    pub fn unavailable(reason: String) -> Self {
        Self { inner: Err(reason) }
    }

    /// mihomo RESTful 客户端（sidecar 存活时可用；错误态返回 unavailable）
    pub fn client(&self) -> Result<&MihomoClient, CommandError> {
        self.inner
            .as_ref()
            .map(|core| &core.client)
            .map_err(|e| CommandError::unavailable(format!("mihomo sidecar 不可用: {e}")))
    }

    /// external-controller 实际端口（错误态为 None）
    pub fn port(&self) -> Option<u16> {
        self.inner.as_ref().ok().map(|core| core.port)
    }

    /// 拉起失败原因（正常态为 None），供命令层/前端展示降级提示
    pub fn startup_error(&self) -> Option<&str> {
        self.inner.as_ref().err().map(String::as_str)
    }

    /// 实时存活探测（打 mihomo /version；错误态直接 false）
    pub async fn alive(&self) -> bool {
        match &self.inner {
            Ok(core) => core.client.alive().await,
            Err(_) => false,
        }
    }

    /// 回收 mihomo 子进程（应用退出时调用；错误态/已回收为 no-op）
    pub fn shutdown(&self) {
        let child = self
            .inner
            .as_ref()
            .ok()
            .and_then(|core| core.child.lock().ok().and_then(|mut g| g.take()));
        if let Some(child) = child {
            // 进程可能已自行退出（如配置错误秒退后残留句柄），kill 失败属正常路径
            if let Err(e) = child.kill() {
                eprintln!("[sidecar] 回收 mihomo 子进程失败（可能已退出）: {e}");
            }
        }
    }
}

/// 应用 setup 时拉起 mihomo sidecar；任何失败都收敛为 [`ProxyCoreState`] 错误态，不 panic。
pub async fn start(app: &AppHandle, app_data_dir: PathBuf) -> ProxyCoreState {
    match start_inner(app, &app_data_dir).await {
        Ok(core) => ProxyCoreState { inner: Ok(core) },
        Err(e) => {
            eprintln!("[sidecar] mihomo 拉起失败，proxy 模块降级不可用: {e}");
            ProxyCoreState::unavailable(e)
        }
    }
}

async fn start_inner(app: &AppHandle, app_data_dir: &Path) -> Result<ProxyCore, String> {
    let work_dir = app_data_dir.join("mihomo");
    std::fs::create_dir_all(&work_dir).map_err(|e| format!("创建 mihomo 工作目录失败: {e}"))?;

    // 端口冲突顺延：9090 被占（如用户本机已有 clash/mihomo 实例）则探测 9091…9100
    let port = pick_port_in(CONTROLLER_PORT_START, CONTROLLER_PORT_END, port_free)
        .ok_or_else(|| format!("{CONTROLLER_PORT_START}~{CONTROLLER_PORT_END} 端口全部被占用"))?;

    // 首跑生成最小可用配置；已有配置（用户手工改过）不覆盖。
    // 注意：顺延后的端口只写进新建的配置模板；既有配置的端口以文件为准，
    // 若与本次探测端口不一致会在就绪等待处超时并报出可读原因。
    let (config_path, created) =
        ensure_config(&work_dir, port).map_err(|e| format!("准备 mihomo 配置失败: {e}"))?;
    if created {
        eprintln!(
            "[sidecar] 已生成初始 mihomo 配置: {}",
            config_path.display()
        );
    }

    // 拉起 sidecar（二进制缺失 / 未打包时 sidecar() 或 spawn() 报错）
    let command = app
        .shell()
        .sidecar("mihomo")
        .map_err(|e| format!("mihomo sidecar 不可用（二进制缺失或未打包）: {e}"))?;
    let (mut rx, child) = command
        .args([
            "-d",
            &*work_dir.to_string_lossy(),
            "-f",
            &*config_path.to_string_lossy(),
        ])
        .spawn()
        .map_err(|e| format!("mihomo 进程启动失败: {e}"))?;

    let client = MihomoClient::new(format!("http://127.0.0.1:{port}"));
    if let Err(e) = wait_ready(&client, &mut rx).await {
        // 就绪失败：回收半成品进程（进程已秒退时 kill 是 no-op），不残留孤儿
        let _ = child.kill();
        return Err(e);
    }

    // 后台持续 drain 子进程输出：不读会导致管道缓冲写满后 mihomo 日志阻塞
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let CommandEvent::Terminated(payload) = event {
                eprintln!("[sidecar] mihomo 进程意外退出: {:?}", payload.code);
            }
        }
    });

    // 实际端口写入设置表（与 UsageService 同一 toogl.db；WAL + busy_timeout 多连接安全）。
    // 持久化失败不视为拉起失败——本轮运行端口已在内存里，仅影响下次启动前的外部查询。
    match ProxySettingStore::connect(app_data_dir.join("toogl.db")).await {
        Ok(store) => {
            if let Err(e) = store.set(KEY_CONTROLLER_PORT, &port.to_string()).await {
                eprintln!("[sidecar] 实际端口写库失败（不影响本轮运行）: {e}");
            }
        }
        Err(e) => eprintln!("[sidecar] 设置库打开失败（不影响本轮运行）: {e}"),
    }

    Ok(ProxyCore {
        port,
        client,
        child: Mutex::new(Some(child)),
    })
}

/// 拉起后就绪等待：轮询 /version 直到存活或超时；期间进程退出（秒退）立即失败，
/// 失败原因附带 mihomo 的 stderr/stdout 片段便于定位（配置错误、端口被抢等）。
async fn wait_ready(
    client: &MihomoClient,
    rx: &mut tauri::async_runtime::Receiver<CommandEvent>,
) -> Result<(), String> {
    let deadline = Instant::now() + READY_TIMEOUT;
    let mut output = String::new();
    loop {
        // 先排空已到达的进程事件：秒退能第一时间感知，无需等满超时
        while let Ok(event) = rx.try_recv() {
            match event {
                CommandEvent::Terminated(payload) => {
                    return Err(format!(
                        "mihomo 进程启动后立即退出（exit code: {:?}）: {}",
                        payload.code,
                        snippet(&output)
                    ));
                }
                CommandEvent::Error(line) => push_output(&mut output, &line),
                CommandEvent::Stdout(bytes) | CommandEvent::Stderr(bytes) => {
                    push_output(&mut output, &String::from_utf8_lossy(&bytes));
                }
                _ => {}
            }
        }
        if client.alive().await {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "mihomo 就绪等待超时（{READY_TIMEOUT:?} 内 /version 未应答）: {}",
                snippet(&output)
            ));
        }
        tokio::time::sleep(READY_POLL_INTERVAL).await;
    }
}

fn push_output(buf: &mut String, s: &str) {
    buf.push_str(s);
    if !s.ends_with('\n') {
        buf.push('\n');
    }
}

fn snippet(output: &str) -> String {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return "（无输出）".to_string();
    }
    trimmed.chars().take(OUTPUT_SNIPPET_MAX).collect()
}

/// 探测端口在 127.0.0.1 上是否空闲（可绑定即空闲；external-controller 只绑回环，
/// 探测同样只绑回环，语义一致）
fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// 在闭区间 [start, end] 内找第一个空闲端口；probe 可注入便于单测
fn pick_port_in(start: u16, end: u16, probe: impl Fn(u16) -> bool) -> Option<u16> {
    (start..=end).find(|&p| probe(p))
}

/// 首跑最小可用配置模板（纯函数便于单测）。
/// 分流规则模板与节点导入是后续任务（《02-开发计划》M5 节），这里只保证内核可启动、
/// RESTful API 可用、全部流量直连兜底。
fn render_initial_config(controller_port: u16) -> String {
    format!(
        r#"# ============================================================
# Toogl（图格）mihomo 初始配置模板
# 本文件由 Toogl 首次运行自动生成，可手工修改；已存在时不会被覆盖。
# 分流规则模板与节点导入是后续功能，当前仅一个 Selector 骨架组兜底直连。
# ============================================================

# 本地混合代理端口（HTTP + SOCKS5 入口），仅监听本机回环，不暴露到局域网
mixed-port: {MIXED_PORT}
bind-address: 127.0.0.1
allow-lan: false

# RESTful API 只绑回环；端口由 Toogl 启动时在 9090~9100 探测顺延，
# 手工改配置后需与本文件端口一致，否则软件侧会连不上
external-controller: 127.0.0.1:{controller_port}

mode: rule
log-level: warning

# 节点列表：后续经「节点导入」功能填充（分享链接 / 订阅）
proxies: []

# 代理组骨架：导入节点后把节点名加入本组 proxies 列表即可在软件里切换
proxy-groups:
  - name: 节点选择
    type: select
    proxies:
      - DIRECT

# 规则骨架：默认全部走「节点选择」组（当前即直连）
rules:
  - MATCH,节点选择
"#
    )
}

/// 确保 config.yaml 存在：不存在则写初始模板。
/// 返回（配置文件路径，是否本次新建）。
fn ensure_config(work_dir: &Path, controller_port: u16) -> std::io::Result<(PathBuf, bool)> {
    let path = work_dir.join("config.yaml");
    if path.exists() {
        return Ok((path, false));
    }
    std::fs::write(&path, render_initial_config(controller_port))?;
    Ok((path, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // ---- 端口顺延 ----

    #[test]
    fn pick_port_returns_first_free() {
        // 9090~9092 被占 → 顺延到 9093
        let occupied: HashSet<u16> = [9090, 9091, 9092].into_iter().collect();
        let port = pick_port_in(9090, 9100, |p| !occupied.contains(&p));
        assert_eq!(port, Some(9093));
    }

    #[test]
    fn pick_port_prefers_default() {
        let port = pick_port_in(9090, 9100, |_| true);
        assert_eq!(port, Some(9090));
    }

    #[test]
    fn pick_port_returns_none_when_all_occupied() {
        let port = pick_port_in(9090, 9100, |_| false);
        assert_eq!(port, None);
    }

    #[test]
    fn port_free_detects_bound_port() {
        // 真实绑定语义：已占用的端口报不可用，放开后可用
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(!port_free(port));
        drop(listener);
        assert!(port_free(port));
    }

    // ---- 配置模板 ----

    #[test]
    fn initial_config_contains_port_and_skeleton() {
        let yaml = render_initial_config(9093);
        assert!(yaml.contains("mixed-port: 7890"));
        assert!(yaml.contains("external-controller: 127.0.0.1:9093"));
        assert!(yaml.contains("allow-lan: false"));
        assert!(yaml.contains("proxies: []"));
        assert!(yaml.contains("type: select"));
        assert!(yaml.contains("- DIRECT"));
        assert!(yaml.contains("- MATCH,节点选择"));
    }

    // ---- 配置生成（不覆盖已有） ----

    #[test]
    fn ensure_config_creates_template_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let (path, created) = ensure_config(tmp.path(), 9091).unwrap();
        assert!(created);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("external-controller: 127.0.0.1:9091"));
    }

    #[test]
    fn ensure_config_never_overwrites_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.yaml");
        std::fs::write(
            &path,
            "# 用户手工配置\nexternal-controller: 127.0.0.1:9000\n",
        )
        .unwrap();
        let (returned, created) = ensure_config(tmp.path(), 9095).unwrap();
        assert_eq!(returned, path);
        assert!(!created);
        // 内容原样保留（哪怕端口与本次探测不一致）
        let content = std::fs::read_to_string(tmp.path().join("config.yaml")).unwrap();
        assert!(content.contains("用户手工配置"));
        assert!(content.contains("127.0.0.1:9000"));
    }

    // ---- 降级态 ----

    #[tokio::test]
    async fn unavailable_state_degrades_readably() {
        let state = ProxyCoreState::unavailable("二进制缺失".to_string());
        assert_eq!(state.startup_error(), Some("二进制缺失"));
        assert_eq!(state.port(), None);
        assert!(!state.alive().await);
        let err = state.client().unwrap_err();
        assert_eq!(err.kind, "unavailable");
        assert!(err.message.contains("二进制缺失"));
        // 错误态回收为 no-op，不 panic
        state.shutdown();
    }
}
