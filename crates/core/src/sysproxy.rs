//! Windows 系统代理读写（M5 系统代理开关，对应《02-开发计划》M5 节）。
//!
//! 职责：
//! - 读写 HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings 的
//!   ProxyEnable / ProxyServer（真实实现 WinRegIo 走 winreg，仅 Windows 编译）；
//! - 写后通过 wininet InternetSetOption（SETTINGS_CHANGED + REFRESH）刷新系统通知；
//! - 开/关成对可靠：开启前保存旧值（用户可能本就有手动代理），关闭时还原而非清空；
//! - 启动自检：比对注册表现状与本软件上次记录的状态，识别崩溃残留供上层提示恢复。
//!
//! 纪律：注册表访问抽象为 [`RegistryIo`]、状态持久化抽象为 [`ProxyStateStore`]，
//! 测试一律用内存 mock，绝不在测试中触碰真实注册表。

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::models::{SystemProxyCheck, SystemProxyStatus};

/// Internet Settings 键路径（HKCU 下，无需管理员权限）
pub const INTERNET_SETTINGS_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

#[derive(Debug, Error)]
pub enum SysProxyError {
    #[error("注册表读写失败: {0}")]
    Registry(String),
    #[error("代理状态存取失败: {0}")]
    Store(String),
    #[error("系统代理变更通知失败: {0}")]
    Notify(String),
}

impl From<SysProxyError> for crate::models::CommandError {
    fn from(e: SysProxyError) -> Self {
        crate::models::CommandError::internal(e.to_string())
    }
}

/// 注册表访问抽象（内部接缝）：真实实现 [`WinRegIo`]（仅 Windows），测试用内存实现。
pub trait RegistryIo: Send + Sync {
    /// 读取当前系统代理状态（ProxyEnable 缺失视为关闭，ProxyServer 缺失为 None）
    fn read(&self) -> Result<SystemProxyStatus, SysProxyError>;
    /// 写入 ProxyEnable；server 为 Some 时同时写 ProxyServer，None 时不动 ProxyServer
    fn write(&self, enabled: bool, server: Option<&str>) -> Result<(), SysProxyError>;
    /// 精确还原完整状态（无损恢复用）：server 为 None 时删除 ProxyServer 值，
    /// 而非保留现状——用户开启前若没有 ProxyServer，恢复后也不该有
    fn restore(&self, status: &SystemProxyStatus) -> Result<(), SysProxyError>;
    /// 通知系统代理设置已变更（真实实现走 InternetSetOption；测试实现可空操作）
    fn notify_change(&self) -> Result<(), SysProxyError>;
}

/// 本软件开启系统代理前保存的现场（用于无损恢复与崩溃残留自检）。
/// 持久化由命令层落到 SQLite proxy_setting 表；仅存于本软件管理会话期间。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProxyBackupState {
    /// 本软件开启前的系统代理状态（关闭时还原此值而非清空）
    pub previous: SystemProxyStatus,
}

/// 代理状态持久化抽象（内部接缝）：命令层用 SQLite 实现，测试用内存实现。
pub trait ProxyStateStore: Send + Sync {
    /// 无记录返回 Ok(None)
    fn load(&self) -> Result<Option<ProxyBackupState>, SysProxyError>;
    fn save(&self, state: &ProxyBackupState) -> Result<(), SysProxyError>;
    fn clear(&self) -> Result<(), SysProxyError>;
}

/// 系统代理管理器：组合注册表 IO 与状态持久化，向上提供开/关/自检/恢复。
/// enable/disable/startup_check 经内部互斥锁串行化：命令层在 spawn_blocking
/// 线程池里真并行，两个并发 enable 的 load→read→save 若交错会把「开启态」
/// 存成 previous 假现场，之后 disable 会把系统代理锁死在开启。
pub struct SystemProxyManager<R: RegistryIo, S: ProxyStateStore> {
    reg: R,
    store: S,
    /// 状态变更操作串行化锁（方法都是同步的，锁内只做注册表/存储 IO，代价极小）
    op_lock: std::sync::Mutex<()>,
}

impl<R: RegistryIo, S: ProxyStateStore> SystemProxyManager<R, S> {
    pub fn new(reg: R, store: S) -> Self {
        Self {
            reg,
            store,
            op_lock: std::sync::Mutex::new(()),
        }
    }

    /// 当前系统代理真实状态（直接读注册表，不读本软件记录）
    pub fn status(&self) -> Result<SystemProxyStatus, SysProxyError> {
        self.reg.read()
    }

    /// 开启系统代理：首次开启前保存旧值（已有备份则不覆盖，保留最初现场），
    /// 然后写 ProxyEnable=1 + ProxyServer 并广播系统通知。
    pub fn enable(&self, server: &str) -> Result<(), SysProxyError> {
        let _guard = self.lock_op();
        if self.store.load()?.is_none() {
            let previous = self.reg.read()?;
            self.store.save(&ProxyBackupState { previous })?;
        }
        self.reg.write(true, Some(server))?;
        self.reg.notify_change()
    }

    /// 关闭系统代理：若现场由本软件开启（有备份）则还原旧值并清除备份；
    /// 否则仅置 ProxyEnable=0（不动 ProxyServer，保留用户既有配置）。
    pub fn disable(&self) -> Result<(), SysProxyError> {
        let _guard = self.lock_op();
        match self.store.load()? {
            Some(backup) => {
                self.reg.restore(&backup.previous)?;
                self.store.clear()?;
            }
            None => self.reg.write(false, None)?,
        }
        self.reg.notify_change()
    }

    /// 启动自检：检测"软件上次异常退出导致系统代理残留开启"。
    /// 有备份且注册表仍开着 → leftover=true（供上层提示并调 [`Self::recover`] 恢复）；
    /// 有备份但注册表已关 → 备份失效（用户/系统已自行关闭），静默清除备份。
    pub fn startup_check(&self) -> Result<SystemProxyCheck, SysProxyError> {
        let _guard = self.lock_op();
        let current = self.reg.read()?;
        match self.store.load()? {
            Some(backup) if current.enabled => Ok(SystemProxyCheck {
                current,
                leftover: true,
                previous: Some(backup.previous),
            }),
            Some(_) => {
                self.store.clear()?;
                Ok(SystemProxyCheck {
                    current,
                    leftover: false,
                    previous: None,
                })
            }
            None => Ok(SystemProxyCheck {
                current,
                leftover: false,
                previous: None,
            }),
        }
    }

    /// 残留恢复：按备份还原开启前的旧值并清除备份。
    /// 与 disable 同逻辑（有备份即还原），由上层在 startup_check 报 leftover 后调用。
    pub fn recover(&self) -> Result<(), SysProxyError> {
        self.disable()
    }

    /// 取操作锁；中毒（持锁线程 panic）时取回守卫继续——锁只为串行化，不为完整性断言
    fn lock_op(&self) -> std::sync::MutexGuard<'_, ()> {
        self.op_lock.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// 真实注册表实现（winreg + wininet），仅 Windows 编译。
#[cfg(windows)]
#[derive(Debug, Default, Clone, Copy)]
pub struct WinRegIo;

#[cfg(windows)]
impl WinRegIo {
    fn open_key(write: bool) -> Result<winreg::RegKey, SysProxyError> {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        let hkcu = winreg::RegKey::predef(HKEY_CURRENT_USER);
        let flags = if write {
            KEY_READ | KEY_WRITE
        } else {
            KEY_READ
        };
        hkcu.open_subkey_with_flags(INTERNET_SETTINGS_KEY, flags)
            .map_err(|e| SysProxyError::Registry(e.to_string()))
    }
}

#[cfg(windows)]
impl RegistryIo for WinRegIo {
    fn read(&self) -> Result<SystemProxyStatus, SysProxyError> {
        let key = Self::open_key(false)?;
        let enabled = key
            .get_value::<u32, _>("ProxyEnable")
            .map(|v| v != 0)
            .unwrap_or(false);
        let server = key.get_value::<String, _>("ProxyServer").ok();
        Ok(SystemProxyStatus { enabled, server })
    }

    fn write(&self, enabled: bool, server: Option<&str>) -> Result<(), SysProxyError> {
        let key = Self::open_key(true)?;
        key.set_value("ProxyEnable", &u32::from(enabled))
            .map_err(|e| SysProxyError::Registry(e.to_string()))?;
        if let Some(server) = server {
            key.set_value("ProxyServer", &server)
                .map_err(|e| SysProxyError::Registry(e.to_string()))?;
        }
        Ok(())
    }

    fn restore(&self, status: &SystemProxyStatus) -> Result<(), SysProxyError> {
        let key = Self::open_key(true)?;
        key.set_value("ProxyEnable", &u32::from(status.enabled))
            .map_err(|e| SysProxyError::Registry(e.to_string()))?;
        match &status.server {
            Some(server) => key
                .set_value("ProxyServer", server)
                .map_err(|e| SysProxyError::Registry(e.to_string()))?,
            // 值本就不存在时删除会报 NotFound，属正常路径
            None => match key.delete_value("ProxyServer") {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(SysProxyError::Registry(e.to_string())),
            },
        }
        Ok(())
    }

    fn notify_change(&self) -> Result<(), SysProxyError> {
        notify_system_proxy_change()
    }
}

/// 桌面端默认装配：真实注册表 + 调用方提供的状态存储。
#[cfg(windows)]
impl<S: ProxyStateStore> SystemProxyManager<WinRegIo, S> {
    pub fn with_system_registry(store: S) -> Self {
        Self::new(WinRegIo, store)
    }
}

/// 广播系统代理变更：InternetSetOption(SETTINGS_CHANGED + REFRESH)。
/// 选 wininet 而非 SendMessageTimeout(WM_SETTINGCHANGE)：WinINet 体系（系统设置页
/// 与多数桌面程序）的官方刷新路径，调用简单、无窗口句柄依赖，失败可检测。
#[cfg(windows)]
fn notify_system_proxy_change() -> Result<(), SysProxyError> {
    use windows_sys::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };
    unsafe {
        let changed = InternetSetOptionW(
            std::ptr::null(),
            INTERNET_OPTION_SETTINGS_CHANGED,
            std::ptr::null(),
            0,
        );
        let refresh = InternetSetOptionW(
            std::ptr::null(),
            INTERNET_OPTION_REFRESH,
            std::ptr::null(),
            0,
        );
        if changed == 0 || refresh == 0 {
            return Err(SysProxyError::Notify(format!(
                "InternetSetOptionW 返回失败（settings_changed={changed}, refresh={refresh}）"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// 内存注册表 mock：绝不触碰真实注册表。可 Clone 以便测试断言内部状态。
    #[derive(Clone)]
    struct MemReg {
        status: Arc<Mutex<SystemProxyStatus>>,
        notify_count: Arc<Mutex<u32>>,
        fail_write: Arc<Mutex<bool>>,
    }

    impl MemReg {
        fn new() -> Self {
            Self::with(false, None)
        }

        fn with(enabled: bool, server: Option<&str>) -> Self {
            Self {
                status: Arc::new(Mutex::new(SystemProxyStatus {
                    enabled,
                    server: server.map(str::to_string),
                })),
                notify_count: Arc::new(Mutex::new(0)),
                fail_write: Arc::new(Mutex::new(false)),
            }
        }

        fn snapshot(&self) -> SystemProxyStatus {
            self.status.lock().unwrap().clone()
        }

        fn notify_count(&self) -> u32 {
            *self.notify_count.lock().unwrap()
        }

        fn set_fail_write(&self, fail: bool) {
            *self.fail_write.lock().unwrap() = fail;
        }
    }

    impl RegistryIo for MemReg {
        fn read(&self) -> Result<SystemProxyStatus, SysProxyError> {
            Ok(self.snapshot())
        }

        fn write(&self, enabled: bool, server: Option<&str>) -> Result<(), SysProxyError> {
            if *self.fail_write.lock().unwrap() {
                return Err(SysProxyError::Registry("模拟写入失败".into()));
            }
            let mut status = self.status.lock().unwrap();
            status.enabled = enabled;
            if let Some(server) = server {
                status.server = Some(server.to_string());
            }
            Ok(())
        }

        fn restore(&self, status: &SystemProxyStatus) -> Result<(), SysProxyError> {
            if *self.fail_write.lock().unwrap() {
                return Err(SysProxyError::Registry("模拟写入失败".into()));
            }
            *self.status.lock().unwrap() = status.clone();
            Ok(())
        }

        fn notify_change(&self) -> Result<(), SysProxyError> {
            *self.notify_count.lock().unwrap() += 1;
            Ok(())
        }
    }

    /// 内存状态存储 mock
    #[derive(Clone, Default)]
    struct MemStore {
        state: Arc<Mutex<Option<ProxyBackupState>>>,
    }

    impl MemStore {
        fn has_state(&self) -> bool {
            self.state.lock().unwrap().is_some()
        }
    }

    impl ProxyStateStore for MemStore {
        fn load(&self) -> Result<Option<ProxyBackupState>, SysProxyError> {
            Ok(self.state.lock().unwrap().clone())
        }

        fn save(&self, state: &ProxyBackupState) -> Result<(), SysProxyError> {
            *self.state.lock().unwrap() = Some(state.clone());
            Ok(())
        }

        fn clear(&self) -> Result<(), SysProxyError> {
            *self.state.lock().unwrap() = None;
            Ok(())
        }
    }

    fn manager(reg: MemReg, store: MemStore) -> SystemProxyManager<MemReg, MemStore> {
        SystemProxyManager::new(reg, store)
    }

    #[test]
    fn enable_writes_proxy_enable_and_server() {
        let reg = MemReg::new();
        let m = manager(reg.clone(), MemStore::default());
        m.enable("127.0.0.1:7890").unwrap();
        assert_eq!(
            reg.snapshot(),
            SystemProxyStatus {
                enabled: true,
                server: Some("127.0.0.1:7890".into())
            }
        );
        assert!(reg.notify_count() > 0);
    }

    #[test]
    fn disable_without_backup_only_clears_enable() {
        // 非本软件开启（无备份）：仅置 ProxyEnable=0，不动用户既有 ProxyServer
        let reg = MemReg::with(true, Some("corp-proxy:3128"));
        let m = manager(reg.clone(), MemStore::default());
        m.disable().unwrap();
        assert_eq!(
            reg.snapshot(),
            SystemProxyStatus {
                enabled: false,
                server: Some("corp-proxy:3128".into())
            }
        );
    }

    #[test]
    fn enable_saves_previous_and_disable_restores_losslessly() {
        // 用户本就有手动代理：开启前保存旧值，关闭时还原而非清空
        let reg = MemReg::with(true, Some("corp-proxy:3128"));
        let store = MemStore::default();
        let m = manager(reg.clone(), store.clone());
        m.enable("127.0.0.1:7890").unwrap();
        assert!(store.has_state());
        m.disable().unwrap();
        assert_eq!(
            reg.snapshot(),
            SystemProxyStatus {
                enabled: true,
                server: Some("corp-proxy:3128".into())
            }
        );
        assert!(!store.has_state());
    }

    #[test]
    fn enable_twice_keeps_original_backup() {
        // 重复开启不得覆盖最初现场
        let reg = MemReg::new();
        let m = manager(reg.clone(), MemStore::default());
        m.enable("127.0.0.1:7890").unwrap();
        m.enable("127.0.0.1:7891").unwrap();
        m.disable().unwrap();
        assert_eq!(
            reg.snapshot(),
            SystemProxyStatus {
                enabled: false,
                server: None
            }
        );
    }

    #[test]
    fn startup_check_clean_without_backup() {
        let reg = MemReg::new();
        let m = manager(reg, MemStore::default());
        let check = m.startup_check().unwrap();
        assert!(!check.leftover);
        assert_eq!(check.previous, None);
        assert!(!check.current.enabled);
    }

    #[test]
    fn startup_check_detects_crash_leftover() {
        // 模拟崩溃：enable 后进程未走 disable，重启后自检应识别残留
        let reg = MemReg::with(true, Some("corp-proxy:3128"));
        let store = MemStore::default();
        manager(reg.clone(), store.clone())
            .enable("127.0.0.1:7890")
            .unwrap();
        // 崩溃 → 重启（新 manager，同一注册表与状态存储）
        let m = manager(reg.clone(), store);
        let check = m.startup_check().unwrap();
        assert!(check.leftover);
        assert_eq!(
            check.previous,
            Some(SystemProxyStatus {
                enabled: true,
                server: Some("corp-proxy:3128".into())
            })
        );
        assert_eq!(check.current.server.as_deref(), Some("127.0.0.1:7890"));
        // 无损恢复：还原用户原有代理
        m.recover().unwrap();
        assert_eq!(
            reg.snapshot(),
            SystemProxyStatus {
                enabled: true,
                server: Some("corp-proxy:3128".into())
            }
        );
        assert!(!m.startup_check().unwrap().leftover);
    }

    #[test]
    fn startup_check_clears_stale_backup_when_proxy_already_off() {
        // 有备份但注册表已被外部关闭：备份失效，静默清除，不报残留
        let reg = MemReg::new();
        let store = MemStore::default();
        manager(reg.clone(), store.clone())
            .enable("127.0.0.1:7890")
            .unwrap();
        // 外部（用户/系统）关闭了代理
        reg.write(false, None).unwrap();
        let m = manager(reg, store.clone());
        let check = m.startup_check().unwrap();
        assert!(!check.leftover);
        assert!(!store.has_state());
    }

    #[test]
    fn write_failure_propagates_and_keeps_backup() {
        let reg = MemReg::new();
        let store = MemStore::default();
        let m = manager(reg.clone(), store.clone());
        reg.set_fail_write(true);
        let err = m.enable("127.0.0.1:7890").unwrap_err();
        assert!(matches!(err, SysProxyError::Registry(_)));
        // 备份已保存（现场仍在），下次启动自检可感知
        assert!(store.has_state());
    }

    /// 读延迟注入的注册表 mock：拉大 enable 的 load→read→save 竞态窗口
    #[derive(Clone)]
    struct SlowReadReg {
        inner: MemReg,
        delay: std::time::Duration,
    }

    impl RegistryIo for SlowReadReg {
        fn read(&self) -> Result<SystemProxyStatus, SysProxyError> {
            std::thread::sleep(self.delay);
            self.inner.read()
        }

        fn write(&self, enabled: bool, server: Option<&str>) -> Result<(), SysProxyError> {
            self.inner.write(enabled, server)
        }

        fn restore(&self, status: &SystemProxyStatus) -> Result<(), SysProxyError> {
            self.inner.restore(status)
        }

        fn notify_change(&self) -> Result<(), SysProxyError> {
            self.inner.notify_change()
        }
    }

    /// 并发 enable 串行化语义：8 个线程同时 enable（mock 读带延迟，无 op_lock 时
    /// load→read→save 必交错出「把开启态存成 previous」的假现场），最终备份里的
    /// previous 必须是开启前的原始状态，disable 后无损还原
    #[test]
    fn concurrent_enables_are_serialized_and_keep_original_backup() {
        let reg = SlowReadReg {
            inner: MemReg::new(),
            delay: std::time::Duration::from_millis(20),
        };
        let store = MemStore::default();
        let m = Arc::new(SystemProxyManager::new(reg.clone(), store.clone()));
        let mut handles = Vec::new();
        for i in 0..8u16 {
            let m = Arc::clone(&m);
            handles.push(std::thread::spawn(move || {
                m.enable(&format!("127.0.0.1:{}", 7890 + i)).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // 只有首个 enable 存现场，且一定是开启前的原始状态（关闭、无 ProxyServer）
        let backup = store.state.lock().unwrap().clone().unwrap();
        assert_eq!(
            backup.previous,
            SystemProxyStatus {
                enabled: false,
                server: None
            }
        );
        assert!(reg.inner.snapshot().enabled);
        // 关闭后无损还原到原始状态（而非锁死在某个开启态）
        m.disable().unwrap();
        assert_eq!(
            reg.inner.snapshot(),
            SystemProxyStatus {
                enabled: false,
                server: None
            }
        );
    }
}
