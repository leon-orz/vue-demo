//! mihomo sidecar 进程生命周期管理（S1-3 项2 落地）。
//!
//! 职责：
//! - 应用 setup 时拉起 mihomo 子进程（-d 工作目录 -f 配置文件），退出时（RunEvent::Exit）回收，
//!   确保不留孤儿进程；
//! - external-controller 端口冲突顺延：9090 被占则探测 9091…9100，实际端口写入
//!   proxy_setting 表（toogl_core::proxy_setting，与 UsageService 同一库文件）；
//! - mixed-port 同样顺延（7890…7900），但只在首跑生成配置模板时生效；实际生效值
//!   （已有配置以文件解析为准）同样写入 proxy_setting；
//! - 首次运行生成最小可用配置（app_data_dir/mihomo/config.yaml，external-controller
//!   带随机 secret 鉴权），已有配置不覆盖；旧版模板缺 secret 时自动补写升级，
//!   用户手写配置（无模板标记）不动；
//! - 拉起失败（二进制缺失 / 进程秒退 / 就绪超时）降级为错误态：proxy 模块不可用，
//!   应用其余部分照常（参照 commands/usage.rs UsageState 的降级模式）。
//!
//! 状态经 [`ProxyCoreState`] 暴露给项3命令层（节点切换 / 系统代理开关 / 规则组切换）；
//! 系统代理启动自检（sysproxy startup_check）的装配也在项3做，本模块只提供 sidecar 侧。

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt as _;
use toogl_core::mihomo::MihomoClient;
use toogl_core::models::CommandError;
use toogl_core::proxy_setting::{
    generate_controller_secret, ProxySettingStore, KEY_CONTROLLER_PORT, KEY_CONTROLLER_SECRET,
    KEY_MIXED_PORT,
};

/// external-controller 探测区间：默认 9090，被占顺延，9100 封顶
pub const CONTROLLER_PORT_START: u16 = 9090;
pub const CONTROLLER_PORT_END: u16 = 9100;
/// 本地混合代理端口（HTTP + SOCKS5 入口）默认值，兼作全链路口径兜底
pub const MIXED_PORT: u16 = 7890;
/// mixed-port 探测区间：默认 7890，被占顺延，7900 封顶（仅首跑生成配置时生效）
pub const MIXED_PORT_START: u16 = 7890;
pub const MIXED_PORT_END: u16 = 7900;
/// 拉起后就绪等待上限（轮询 mihomo /version）
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// 就绪轮询间隔
const READY_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// 单次就绪探测上限：alive() 走 reqwest 默认 10s 超时，不封顶会让总上限失控；
/// 实际单次封顶取本值与总预算剩余时长的较小者
const READY_POLL_TIMEOUT: Duration = Duration::from_secs(2);
/// 失败原因中附带的 mihomo 输出截断长度（防超长日志）
const OUTPUT_SNIPPET_MAX: usize = 500;
/// 本工具生成的配置模板标记（首跑模板头注释）：升级路径据此区分
/// 「我们生成的旧模板（缺 secret 可自动补写）」与「用户手写配置（不动）」
const TEMPLATE_MARKER: &str = "本文件由 Toogl 首次运行自动生成";

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
    /// 主动回收标记：shutdown() 置位后再 kill，drain 任务据此区分
    /// 「应用退出主动回收」与「mihomo 意外退出」，前者不打意外退出日志
    killed_by_us: Arc<AtomicBool>,
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
        if let Ok(core) = self.inner.as_ref() {
            // 先置主动回收标记再 kill：drain 任务看到 Terminated 事件时便知是
            // 应用正常退出回收，不当作意外退出
            core.killed_by_us.store(true, Ordering::SeqCst);
        }
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
    // mixed-port 同样做冲突顺延（7890~7900），但只在首跑生成模板时生效：
    // 已有配置的 mixed-port 以文件为准，若被占 mihomo 会监听失败，
    // 原因经就绪等待的失败信息（proxy_status.startup_error）体现。
    let first_run = !work_dir.join("config.yaml").exists();
    let mixed_port = if first_run {
        pick_port_in(MIXED_PORT_START, MIXED_PORT_END, port_free).unwrap_or_else(|| {
            eprintln!(
                "[sidecar] {MIXED_PORT_START}~{MIXED_PORT_END} 端口全部被占用，回退 {MIXED_PORT}（mihomo 可能监听失败，原因会在启动错误里体现）"
            );
            MIXED_PORT
        })
    } else {
        // 占位值：不写入已有配置，实际值下面从文件解析
        MIXED_PORT
    };
    // 首跑生成配置时一并生成 external-controller 鉴权密钥（随机 hex），
    // 防止本机任意进程随意控制内核；secret 仅在新建配置时写入模板，
    // 已有配置不覆盖（secret 升级走下面的 resolve_existing_secret）
    let new_secret = generate_controller_secret();
    let (config_path, created) = ensure_config(&work_dir, port, mixed_port, &new_secret)
        .map_err(|e| format!("准备 mihomo 配置失败: {e}"))?;
    if created {
        eprintln!(
            "[sidecar] 已生成初始 mihomo 配置: {}",
            config_path.display()
        );
    }
    let content = std::fs::read_to_string(&config_path).ok();
    // 实际生效的 mixed 端口：新建即探测值；已有配置以文件内容为准（用户可手改）
    let effective_mixed_port = content
        .as_deref()
        .and_then(parse_mixed_port)
        .unwrap_or(mixed_port);

    // 实际生效的 external-controller secret：
    // - 本次新建配置 → 刚生成的密钥；
    // - 已有配置且已带 secret → 沿用；
    // - 已有配置缺 secret 且为本工具生成的旧版模板（带模板标记）→ 自动补写升级；
    // - 用户手写配置（无标记）→ 不动配置、不带鉴权头，尊重用户配置。
    let effective_secret: Option<String> = if created {
        Some(new_secret)
    } else {
        match content.as_deref().map(resolve_existing_secret) {
            Some((secret @ Some(_), None)) => secret,
            Some((secret, Some(patched))) => match std::fs::write(&config_path, patched) {
                Ok(()) => {
                    eprintln!(
                        "[sidecar] 检测到旧版配置模板缺 external-controller secret，已自动补写"
                    );
                    secret
                }
                // 补写失败：mihomo 将按无 secret 配置启动，client 同样不带鉴权，保持一致
                Err(e) => {
                    eprintln!(
                        "[sidecar] 补写 external-controller secret 失败，本轮不启用鉴权: {e}"
                    );
                    None
                }
            },
            // 用户手写配置缺 secret，或配置读取失败：不启用鉴权
            _ => None,
        }
    };

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

    let client = MihomoClient::new(format!("http://127.0.0.1:{port}"), effective_secret.clone());
    if let Err(e) = wait_ready(&client, &mut rx).await {
        // 就绪失败：回收半成品进程（进程已秒退时 kill 是 no-op），不残留孤儿
        let _ = child.kill();
        return Err(e);
    }

    // 后台持续 drain 子进程输出：不读会导致管道缓冲写满后 mihomo 日志阻塞
    let killed_by_us = Arc::new(AtomicBool::new(false));
    let drain_flag = Arc::clone(&killed_by_us);
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let CommandEvent::Terminated(payload) = event {
                // 应用正常退出由 shutdown() 置标记后主动 kill，不属意外退出，不打告警日志
                if !drain_flag.load(Ordering::SeqCst) {
                    eprintln!("[sidecar] mihomo 进程意外退出: {:?}", payload.code);
                }
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
            if let Err(e) = store
                .set(KEY_MIXED_PORT, &effective_mixed_port.to_string())
                .await
            {
                eprintln!("[sidecar] mixed 端口写库失败（不影响本轮运行）: {e}");
            }
            // 鉴权密钥持久化：供外部查询与升级路径核对；失败同样不影响本轮运行
            if let Some(secret) = &effective_secret {
                if let Err(e) = store.set(KEY_CONTROLLER_SECRET, secret).await {
                    eprintln!("[sidecar] controller secret 写库失败（不影响本轮运行）: {e}");
                }
            }
        }
        Err(e) => eprintln!("[sidecar] 设置库打开失败（不影响本轮运行）: {e}"),
    }

    Ok(ProxyCore {
        port,
        client,
        child: Mutex::new(Some(child)),
        killed_by_us,
    })
}

/// 拉起后就绪等待：轮询 /version 直到存活或超时；期间进程退出（秒退）立即失败，
/// 失败原因附带 mihomo 的 stderr/stdout 片段便于定位（配置错误、端口被抢等）。
/// 单次探测用 timeout 封顶（单次上限与总预算剩余取小），保证总耗时不超过 READY_TIMEOUT。
async fn wait_ready(
    client: &MihomoClient,
    rx: &mut tauri::async_runtime::Receiver<CommandEvent>,
) -> Result<(), String> {
    wait_ready_within(
        client,
        rx,
        READY_TIMEOUT,
        READY_POLL_INTERVAL,
        READY_POLL_TIMEOUT,
    )
    .await
}

/// [`wait_ready`] 的参数化实现，便于单测覆盖超时边界
async fn wait_ready_within(
    client: &MihomoClient,
    rx: &mut tauri::async_runtime::Receiver<CommandEvent>,
    total_timeout: Duration,
    poll_interval: Duration,
    per_poll_timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + total_timeout;
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
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "mihomo 就绪等待超时（{total_timeout:?} 内 /version 未应答）: {}",
                snippet(&output)
            ));
        }
        // 单次探测封顶：alive() 内部是 reqwest 默认 10s 超时，不封顶的话
        // 一次挂死的请求就会让总上限失控；取单次上限与剩余预算的较小者
        let budget = per_poll_timeout.min(deadline - now);
        if matches!(tokio::time::timeout(budget, client.alive()).await, Ok(true)) {
            return Ok(());
        }
        tokio::time::sleep(poll_interval).await;
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
/// external-controller 带随机 secret 鉴权：本机其他进程不知道密钥即无法控制内核。
fn render_initial_config(controller_port: u16, mixed_port: u16, secret: &str) -> String {
    format!(
        r#"# ============================================================
# Toogl（图格）mihomo 初始配置模板
# 本文件由 Toogl 首次运行自动生成，可手工修改；已存在时不会被覆盖。
# 分流规则模板与节点导入是后续功能，当前仅一个 Selector 骨架组兜底直连。
# ============================================================

# 本地混合代理端口（HTTP + SOCKS5 入口），仅监听本机回环，不暴露到局域网；
# 端口由 Toogl 首跑时在 7890~7900 探测顺延，被占会顺延到空闲端口
mixed-port: {mixed_port}
bind-address: 127.0.0.1
allow-lan: false

# RESTful API 只绑回环；端口由 Toogl 启动时在 9090~9100 探测顺延，
# 手工改配置后需与本文件端口一致，否则软件侧会连不上
external-controller: 127.0.0.1:{controller_port}

# RESTful API 鉴权密钥：Toogl 首跑随机生成，同时持久化到 proxy_setting 表
# （KEY_CONTROLLER_SECRET）；本机不知道密钥的进程无法控制内核
secret: {secret}

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

/// 确保 config.yaml 存在：不存在则以给定 secret 写初始模板。
/// 返回（配置文件路径，是否本次新建）。
fn ensure_config(
    work_dir: &Path,
    controller_port: u16,
    mixed_port: u16,
    secret: &str,
) -> std::io::Result<(PathBuf, bool)> {
    let path = work_dir.join("config.yaml");
    if path.exists() {
        return Ok((path, false));
    }
    std::fs::write(
        &path,
        render_initial_config(controller_port, mixed_port, secret),
    )?;
    Ok((path, true))
}

/// 纯解析：取顶层 `secret: <v>` 行的密钥（忽略注释与缩进变体），便于单测。
/// 与 parse_mixed_port 同款极简行解析，不引入 YAML 解析器。
fn parse_controller_secret(config: &str) -> Option<String> {
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("secret:") {
            let value = value.trim();
            // secret 为空串（secret: '' 或 secret:）视为未配置
            if !value.is_empty() && value != "''" && value != "\"\"" {
                return Some(value.to_string());
            }
            return None;
        }
    }
    None
}

/// 向配置文本注入 `secret: <v>` 行：优先插在 external-controller 行之后，
/// 找不到（模板结构被手改）则追加到文件末尾。仅在模板升级路径调用。
fn inject_controller_secret(config: &str, secret: &str) -> String {
    let mut lines: Vec<String> = config.lines().map(str::to_string).collect();
    let pos = lines
        .iter()
        .position(|l| l.trim_start().starts_with("external-controller:"));
    let line = format!("secret: {secret}");
    match pos {
        Some(i) => lines.insert(i + 1, line),
        None => lines.push(line),
    }
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// 已有配置的 secret 决策（纯函数便于单测），返回（生效 secret，需写回的补写后内容）：
/// - 配置里已有 secret → 沿用，不写回；
/// - 缺 secret 且带本工具模板标记（我们生成的旧版模板）→ 生成新密钥并补写；
/// - 缺 secret 且为用户手写配置（无标记）→ 不动配置，返回 None（client 不带鉴权头）。
fn resolve_existing_secret(content: &str) -> (Option<String>, Option<String>) {
    if let Some(secret) = parse_controller_secret(content) {
        return (Some(secret), None);
    }
    if !content.contains(TEMPLATE_MARKER) {
        return (None, None);
    }
    let secret = generate_controller_secret();
    let patched = inject_controller_secret(content, &secret);
    (Some(secret), Some(patched))
}

/// 解析系统代理开关指向的本地 mixed 端口，优先级：
/// 1. proxy_setting 持久化值（sidecar 上次拉起时写入的实际生效值）；
/// 2. sidecar 生成的 config.yaml 的 `mixed-port` 字段（用户可手改）；
/// 3. 常量 [`MIXED_PORT`] 兜底。
///
/// 注意阶段一约定：系统代理走 mihomo 的 mixed 入口，而不是 external-controller 端口。
pub async fn resolve_mixed_port(app_data_dir: &Path) -> u16 {
    let persisted = match ProxySettingStore::connect(app_data_dir.join("toogl.db")).await {
        Ok(store) => store
            .get(KEY_MIXED_PORT)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u16>().ok()),
        Err(_) => None,
    };
    if let Some(port) = persisted {
        return port;
    }
    let path = app_data_dir.join("mihomo").join("config.yaml");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| parse_mixed_port(&content))
        .unwrap_or(MIXED_PORT)
}

/// 纯解析：取顶层 `mixed-port: <n>` 行的端口号（忽略注释与缩进变体），便于单测。
/// 极简行解析而非引入 YAML 解析器：初始模板固定顶层一行，足够覆盖手改场景。
fn parse_mixed_port(config: &str) -> Option<u16> {
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("mixed-port:") {
            return value.trim().parse().ok();
        }
    }
    None
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
        let yaml = render_initial_config(9093, 7895, "test-secret-abc");
        // mixed-port 首跑顺延后的真实值写入模板
        assert!(yaml.contains("mixed-port: 7895"));
        assert!(yaml.contains("external-controller: 127.0.0.1:9093"));
        // external-controller 鉴权密钥写入模板
        assert!(yaml.contains("secret: test-secret-abc"));
        assert!(yaml.contains("allow-lan: false"));
        assert!(yaml.contains("proxies: []"));
        assert!(yaml.contains("type: select"));
        assert!(yaml.contains("- DIRECT"));
        assert!(yaml.contains("- MATCH,节点选择"));
        // 模板标记存在（升级路径据此区分本工具模板与用户手写配置）
        assert!(yaml.contains(TEMPLATE_MARKER));
    }

    #[test]
    fn mixed_port_pick_prefers_default_and_defers_when_occupied() {
        // 7890 空闲 → 用默认；7890~7892 被占 → 顺延到 7893
        assert_eq!(
            pick_port_in(MIXED_PORT_START, MIXED_PORT_END, |_| true),
            Some(7890)
        );
        let occupied: HashSet<u16> = [7890, 7891, 7892].into_iter().collect();
        assert_eq!(
            pick_port_in(MIXED_PORT_START, MIXED_PORT_END, |p| !occupied.contains(&p)),
            Some(7893)
        );
        // 全段被占 → None（调用方回退常量并告警）
        assert_eq!(
            pick_port_in(MIXED_PORT_START, MIXED_PORT_END, |_| false),
            None
        );
    }

    // ---- 配置生成（不覆盖已有） ----

    #[test]
    fn ensure_config_creates_template_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let (path, created) = ensure_config(tmp.path(), 9091, 7890, "first-run-secret").unwrap();
        assert!(created);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("external-controller: 127.0.0.1:9091"));
        assert!(content.contains("mixed-port: 7890"));
        assert!(content.contains("secret: first-run-secret"));
    }

    // ---- mixed-port 解析（系统代理开关指向的本地入口） ----

    #[test]
    fn parse_mixed_port_from_template_and_edited() {
        // 初始模板按传入端口生成
        assert_eq!(
            parse_mixed_port(&render_initial_config(9090, 7890, "s")),
            Some(7890)
        );
        assert_eq!(
            parse_mixed_port(&render_initial_config(9090, 7897, "s")),
            Some(7897)
        );
        // 用户手改为其他端口
        assert_eq!(
            parse_mixed_port("mixed-port: 7899\nexternal-controller: 127.0.0.1:9090\n"),
            Some(7899)
        );
        // 注释行与缺失字段
        assert_eq!(parse_mixed_port("# mixed-port: 1234\n"), None);
        assert_eq!(parse_mixed_port("mode: rule\n"), None);
        // 非法端口号
        assert_eq!(parse_mixed_port("mixed-port: abc\n"), None);
    }

    #[tokio::test]
    async fn resolve_mixed_port_priority_persisted_then_file_then_default() {
        // 无配置无持久化 → 常量兜底
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(resolve_mixed_port(tmp.path()).await, MIXED_PORT);
        // 有配置文件且手改过端口 → 以文件为准
        let mihomo_dir = tmp.path().join("mihomo");
        std::fs::create_dir_all(&mihomo_dir).unwrap();
        std::fs::write(mihomo_dir.join("config.yaml"), "mixed-port: 7901\n").unwrap();
        assert_eq!(resolve_mixed_port(tmp.path()).await, 7901);
        // 持久化值优先于文件（sidecar 拉起时写入的实际生效值）
        let store = ProxySettingStore::connect(tmp.path().join("toogl.db"))
            .await
            .unwrap();
        store.set(KEY_MIXED_PORT, "7895").await.unwrap();
        assert_eq!(resolve_mixed_port(tmp.path()).await, 7895);
        // 持久化值损坏 → 回退到文件解析
        store.set(KEY_MIXED_PORT, "not-a-port").await.unwrap();
        assert_eq!(resolve_mixed_port(tmp.path()).await, 7901);
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
        let (returned, created) = ensure_config(tmp.path(), 9095, 7890, "unused-secret").unwrap();
        assert_eq!(returned, path);
        assert!(!created);
        // 内容原样保留（哪怕端口与本次探测不一致）
        let content = std::fs::read_to_string(tmp.path().join("config.yaml")).unwrap();
        assert!(content.contains("用户手工配置"));
        assert!(content.contains("127.0.0.1:9000"));
    }

    // ---- external-controller secret 解析 / 注入 / 升级决策 ----

    #[test]
    fn parse_controller_secret_from_template_and_edited() {
        // 初始模板含生成值
        assert_eq!(
            parse_controller_secret(&render_initial_config(9090, 7890, "abc123")),
            Some("abc123".to_string())
        );
        // 用户手改 secret
        assert_eq!(
            parse_controller_secret("external-controller: 127.0.0.1:9090\nsecret: my-own\n"),
            Some("my-own".to_string())
        );
        // 缺失 / 注释 / 空值均视为未配置
        assert_eq!(parse_controller_secret("mode: rule\n"), None);
        assert_eq!(parse_controller_secret("# secret: abc\n"), None);
        assert_eq!(parse_controller_secret("secret: \n"), None);
        assert_eq!(parse_controller_secret("secret: ''\n"), None);
    }

    #[test]
    fn inject_controller_secret_after_external_controller_line() {
        let old = "# 头注释\nexternal-controller: 127.0.0.1:9093\nmode: rule\n";
        let patched = inject_controller_secret(old, "new-secret");
        assert_eq!(
            patched,
            "# 头注释\nexternal-controller: 127.0.0.1:9093\nsecret: new-secret\nmode: rule\n"
        );
        // 注入后可被解析回来
        assert_eq!(
            parse_controller_secret(&patched),
            Some("new-secret".to_string())
        );
    }

    #[test]
    fn inject_controller_secret_appends_when_no_controller_line() {
        let patched = inject_controller_secret("mode: rule\n", "s1");
        assert!(patched.contains("mode: rule"));
        assert!(patched.contains("secret: s1"));
        assert!(patched.ends_with('\n'));
        assert_eq!(parse_controller_secret(&patched), Some("s1".to_string()));
    }

    #[test]
    fn resolve_existing_secret_keeps_configured_secret() {
        let content = render_initial_config(9090, 7890, "existing-secret");
        let (secret, patched) = resolve_existing_secret(&content);
        assert_eq!(secret, Some("existing-secret".to_string()));
        assert!(patched.is_none());
    }

    #[test]
    fn resolve_existing_secret_patches_our_legacy_template() {
        // 模拟旧版本生成的模板：带模板标记、无 secret 行
        let legacy = render_initial_config(9090, 7890, "x")
            .lines()
            .filter(|l| !l.trim_start().starts_with("secret:"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(legacy.contains(TEMPLATE_MARKER));
        assert_eq!(parse_controller_secret(&legacy), None);

        let (secret, patched) = resolve_existing_secret(&legacy);
        let secret = secret.expect("旧模板应自动补写 secret");
        let patched = patched.expect("旧模板应产出补写后的内容");
        assert_eq!(secret.len(), 32);
        // 补写后内容可解析出同一 secret，且原有行保留
        assert_eq!(parse_controller_secret(&patched), Some(secret));
        assert!(patched.contains("external-controller: 127.0.0.1:9090"));
        assert!(patched.contains(TEMPLATE_MARKER));
    }

    #[test]
    fn resolve_existing_secret_leaves_user_handwritten_config_alone() {
        // 用户手写配置（无模板标记、无 secret）：不动配置、不启用鉴权
        let user = "# 用户手工配置\nexternal-controller: 127.0.0.1:9000\nmode: rule\n";
        let (secret, patched) = resolve_existing_secret(user);
        assert_eq!(secret, None);
        assert_eq!(patched, None);
    }

    // ---- 就绪等待（wait_ready）超时边界 ----

    #[tokio::test]
    async fn wait_ready_times_out_within_total_budget() {
        // 端口 1 必拒连：每次探测快速失败，总耗时受 total_timeout 约束
        let client = MihomoClient::new("http://127.0.0.1:1", None);
        let (_tx, mut rx) = tauri::async_runtime::channel(16);
        let start = Instant::now();
        let err = wait_ready_within(
            &client,
            &mut rx,
            Duration::from_millis(300),
            Duration::from_millis(50),
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(err.contains("就绪等待超时"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn wait_ready_caps_single_hanging_poll() {
        // 接受连接但永不响应的服务端，模拟 mihomo 挂死：
        // 单次探测被 per_poll_timeout 封顶，总耗时仍在 total_timeout 量级，
        // 而非 reqwest 默认的 10s 单次超时
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while let Ok((conn, _)) = listener.accept() {
                held.push(conn); // 持有连接，永不写响应
            }
        });
        let client = MihomoClient::new(format!("http://{addr}"), None);
        let (_tx, mut rx) = tauri::async_runtime::channel(16);
        let start = Instant::now();
        let err = wait_ready_within(
            &client,
            &mut rx,
            Duration::from_millis(300),
            Duration::from_millis(50),
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(err.contains("就绪等待超时"));
        // 关键断言：单次挂死被封顶，总耗时远小于单次 reqwest 默认超时（10s）
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn wait_ready_fails_fast_on_process_exit() {
        use tauri_plugin_shell::process::TerminatedPayload;
        let client = MihomoClient::new("http://127.0.0.1:1", None);
        let (tx, mut rx) = tauri::async_runtime::channel(16);
        tx.send(CommandEvent::Terminated(TerminatedPayload {
            code: Some(2),
            signal: None,
        }))
        .await
        .unwrap();
        let start = Instant::now();
        // 用默认 wait_ready（总上限 10s）：进程秒退应立即失败，不等满超时
        let err = wait_ready(&client, &mut rx).await.unwrap_err();
        assert!(err.contains("立即退出"));
        assert!(err.contains("2"));
        assert!(start.elapsed() < Duration::from_secs(2));
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
