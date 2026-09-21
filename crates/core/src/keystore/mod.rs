//! 密钥存储抽象（《02-开发计划》1.3）：
//! - 桌面端：KeyringStore（Windows 凭据管理器，keyring-rs）
//! - hub（Linux 无桌面）：后续退回"主密钥加密的文件存储"，trait 不变
//! - 测试：MemoryStore

use std::collections::HashMap;
use std::sync::Mutex;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeystoreError {
    #[error("keyring 错误: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("内部锁错误")]
    LockPoisoned,
}

/// 桌面端 keyring 命名空间（与应用 identifier 一致，统一在此定义）
pub const KEYRING_SERVICE_NAME: &str = "app.toogl.desktop";

impl From<KeystoreError> for crate::models::CommandError {
    fn from(e: KeystoreError) -> Self {
        crate::models::CommandError::internal(e.to_string())
    }
}

pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, value: &str) -> Result<(), KeystoreError>;
    /// key 不存在时返回 Ok(None)
    fn get(&self, key: &str) -> Result<Option<String>, KeystoreError>;
    fn delete(&self, key: &str) -> Result<(), KeystoreError>;
}

/// 系统钥匙串实现。service 为命名空间（桌面端用应用 identifier）。
pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

impl SecretStore for KeyringStore {
    fn set(&self, key: &str, value: &str) -> Result<(), KeystoreError> {
        keyring::Entry::new(&self.service, key)?.set_password(value)?;
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, KeystoreError> {
        match keyring::Entry::new(&self.service, key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn delete(&self, key: &str) -> Result<(), KeystoreError> {
        match keyring::Entry::new(&self.service, key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// 进程内存实现：单元测试与 hub 降级用。
#[derive(Default)]
pub struct MemoryStore {
    map: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn set(&self, key: &str, value: &str) -> Result<(), KeystoreError> {
        self.map
            .lock()
            .map_err(|_| KeystoreError::LockPoisoned)?
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<String>, KeystoreError> {
        Ok(self
            .map
            .lock()
            .map_err(|_| KeystoreError::LockPoisoned)?
            .get(key)
            .cloned())
    }

    fn delete(&self, key: &str) -> Result<(), KeystoreError> {
        self.map
            .lock()
            .map_err(|_| KeystoreError::LockPoisoned)?
            .remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_roundtrip() {
        let store = MemoryStore::default();
        assert_eq!(store.get("k").unwrap(), None);

        store.set("k", "v1").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v1"));

        store.set("k", "v2").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v2"));

        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);

        // 删除不存在的 key 不报错
        store.delete("missing").unwrap();
    }

    /// 真实系统钥匙串冒烟测试（写->读->删）。
    /// 需在带桌面环境/凭据服务的本机手动跑：
    /// cargo test -p toogl-core -- --ignored keyring_real_roundtrip
    #[test]
    #[ignore = "依赖真实系统钥匙串，手动执行"]
    fn keyring_real_roundtrip() {
        let store = KeyringStore::new("app.toogl.desktop.test");
        let key = "s1-1-smoke-test";

        let _ = store.delete(key);
        store.set(key, "hello-keyring").unwrap();
        assert_eq!(store.get(key).unwrap().as_deref(), Some("hello-keyring"));
        store.delete(key).unwrap();
        assert_eq!(store.get(key).unwrap(), None);
    }
}
