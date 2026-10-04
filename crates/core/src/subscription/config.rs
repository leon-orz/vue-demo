//! 托管节点文件（toogl-nodes.yaml）写入 + config.yaml 的一次性接线。
//!
//! 接线形态：mihomo proxy-providers 的 file provider 指向托管节点文件，
//! 「节点选择」组通过 `use: [toogl]` 引用该 provider——节点增删只改托管文件，
//! config.yaml 接线一次后不再动。热重载由桌面端走 mihomo `PUT /configs?force`。
//!
//! 自动接线仅限本工具首跑生成的模板配置（按模板标记识别）；用户手写配置
//! 一律不动，返回 Wiring 错误提示手工补接线。

use std::path::{Path, PathBuf};

use serde_yaml::{Mapping, Value};

use super::SubscriptionError;

/// 托管节点文件名（与 config.yaml 同目录，proxy-providers file provider 的相对路径）
pub const NODES_FILE_NAME: &str = "toogl-nodes.yaml";

/// 托管节点文件头注释（每次重写都带上，提示用户不要手改）
const NODES_FILE_HEADER: &str = "# 本文件由 Toogl 托管，请勿手改\n";

/// 本工具首跑配置模板标记（复制自 apps/desktop/src-tauri/src/sidecar.rs 的
/// TEMPLATE_MARKER，两边必须保持一致）：据此区分「本工具模板（可自动接线）」
/// 与「用户手写配置（不动）」
const TEMPLATE_MARKER: &str = "本文件由 Toogl 首次运行自动生成";

/// proxy-providers 下本工具占用的 provider 名
const PROVIDER_NAME: &str = "toogl";

/// 接线目标代理组名（首跑模板里的 Selector 骨架组）
const TARGET_GROUP: &str = "节点选择";

/// 写托管节点文件：内容为注释头 + `proxies: [...]`（serde_yaml 序列化），
/// 先写 tmp 再 rename 原子替换（避免 mihomo 读到半截文件）。空列表也写（`proxies: []`）。
/// 返回写入的文件路径。
pub fn write_nodes_file(work_dir: &Path, nodes: &[Mapping]) -> Result<PathBuf, SubscriptionError> {
    let mut root = Mapping::new();
    root.insert(
        Value::String("proxies".to_string()),
        Value::Sequence(nodes.iter().map(|n| Value::Mapping(n.clone())).collect()),
    );
    let body = serde_yaml::to_string(&Value::Mapping(root))
        .map_err(|e| SubscriptionError::Io(format!("节点文件序列化失败: {e}")))?;
    let path = work_dir.join(NODES_FILE_NAME);
    let tmp = work_dir.join(format!("{NODES_FILE_NAME}.tmp"));
    std::fs::write(&tmp, format!("{NODES_FILE_HEADER}{body}"))
        .map_err(|e| SubscriptionError::Io(format!("节点文件写入失败: {e}")))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| SubscriptionError::Io(format!("节点文件替换失败: {e}")))?;
    Ok(path)
}

/// 确保 config.yaml 已接线到托管节点文件：
/// - 已接线（结构化判断：proxy-providers 有 toogl 且「节点选择」组 use 含 toogl）→ Ok(false)；
/// - 未接线且为本工具首跑模板（含模板标记）→ 自动接线：加 proxy-providers、
///   「节点选择」组加 `use: [toogl]`，原文件备份为 config.yaml.bak 后写回 → Ok(true)；
/// - 未接线且非本工具模板（用户手写配置）→ Wiring 错误，文件不动。
pub fn ensure_wired(config_path: &Path) -> Result<bool, SubscriptionError> {
    let original = std::fs::read_to_string(config_path)
        .map_err(|e| SubscriptionError::Io(format!("config.yaml 读取失败: {e}")))?;
    let mut doc: Mapping = serde_yaml::from_str(&original).map_err(|e| {
        SubscriptionError::Wiring(format!("config.yaml 不是合法 YAML，无法接线: {e}"))
    })?;

    if is_wired(&doc) {
        return Ok(false);
    }
    if !original.contains(TEMPLATE_MARKER) {
        return Err(SubscriptionError::Wiring(
            "config.yaml 缺少 Toogl 接线且非本工具生成的模板（用户手写配置），\
             请手工补充 proxy-providers 接线：\n\
             proxy-providers:\n  toogl:\n    type: file\n    path: toogl-nodes.yaml\n\
             并在「节点选择」组加 use: [toogl]"
                .to_string(),
        ));
    }

    wire(&mut doc)?;

    // YAML 序列化会丢注释，把原模板标记行 prepend 回文件顶部（保持升级路径识别能力）
    let marker_line = original
        .lines()
        .find(|l| l.contains(TEMPLATE_MARKER))
        .expect("模板标记已确认存在");
    let body = serde_yaml::to_string(&Value::Mapping(doc))
        .map_err(|e| SubscriptionError::Io(format!("config.yaml 序列化失败: {e}")))?;

    // 先备份再写回（接线只发生一次，备份保留用户可回滚的接线前版本）
    let backup = config_path.with_extension("yaml.bak");
    std::fs::copy(config_path, &backup)
        .map_err(|e| SubscriptionError::Io(format!("config.yaml 备份失败: {e}")))?;
    std::fs::write(config_path, format!("{marker_line}\n{body}"))
        .map_err(|e| SubscriptionError::Io(format!("config.yaml 写回失败: {e}")))?;
    Ok(true)
}

/// 结构化判断：proxy-providers 下有 toogl 键，且「节点选择」组 use 含 toogl
fn is_wired(doc: &Mapping) -> bool {
    let has_provider = doc
        .get(Value::String("proxy-providers".to_string()))
        .and_then(Value::as_mapping)
        .is_some_and(|p| p.contains_key(Value::String(PROVIDER_NAME.to_string())));
    let group_uses = find_target_group(doc).is_some_and(|g| {
        g.get(Value::String("use".to_string()))
            .and_then(Value::as_sequence)
            .is_some_and(|u| u.iter().any(|v| v.as_str() == Some(PROVIDER_NAME)))
    });
    has_provider && group_uses
}

/// 在 proxy-groups 序列中找名为「节点选择」的组
fn find_target_group(doc: &Mapping) -> Option<&Mapping> {
    doc.get(Value::String("proxy-groups".to_string()))?
        .as_sequence()?
        .iter()
        .filter_map(Value::as_mapping)
        .find(|g| {
            g.get(Value::String("name".to_string()))
                .and_then(|v| v.as_str())
                == Some(TARGET_GROUP)
        })
}

/// 执行接线（调用前已确认未接线且为本工具模板）：加 provider + 目标组 use
fn wire(doc: &mut Mapping) -> Result<(), SubscriptionError> {
    // proxy-providers: { toogl: { type: file, path: toogl-nodes.yaml } }
    let mut provider = Mapping::new();
    provider.insert(
        Value::String("type".to_string()),
        Value::String("file".to_string()),
    );
    provider.insert(
        Value::String("path".to_string()),
        Value::String(NODES_FILE_NAME.to_string()),
    );
    let providers_key = Value::String("proxy-providers".to_string());
    match doc.get_mut(&providers_key) {
        Some(Value::Mapping(providers)) => {
            providers.insert(
                Value::String(PROVIDER_NAME.to_string()),
                Value::Mapping(provider),
            );
        }
        Some(_) => {
            return Err(SubscriptionError::Wiring(
                "config.yaml 的 proxy-providers 不是键值对结构，请手工补接线".to_string(),
            ));
        }
        None => {
            let mut providers = Mapping::new();
            providers.insert(
                Value::String(PROVIDER_NAME.to_string()),
                Value::Mapping(provider),
            );
            doc.insert(providers_key, Value::Mapping(providers));
        }
    }

    // 「节点选择」组加 use: [toogl]
    let groups = doc
        .get_mut(Value::String("proxy-groups".to_string()))
        .and_then(Value::as_sequence_mut)
        .ok_or_else(|| {
            SubscriptionError::Wiring("config.yaml 缺 proxy-groups，请手工补接线".to_string())
        })?;
    let group = groups
        .iter_mut()
        .filter_map(Value::as_mapping_mut)
        .find(|g| {
            g.get(Value::String("name".to_string()))
                .and_then(Value::as_str)
                == Some(TARGET_GROUP)
        })
        .ok_or_else(|| {
            SubscriptionError::Wiring(format!(
                "config.yaml 找不到「{TARGET_GROUP}」代理组，请手工在目标组加 use: [toogl]"
            ))
        })?;
    match group.get_mut(Value::String("use".to_string())) {
        Some(Value::Sequence(uses)) => {
            if !uses.iter().any(|v| v.as_str() == Some(PROVIDER_NAME)) {
                uses.push(Value::String(PROVIDER_NAME.to_string()));
            }
        }
        Some(_) => {
            return Err(SubscriptionError::Wiring(format!(
                "「{TARGET_GROUP}」组的 use 字段不是列表，请手工补 use: [toogl]"
            )));
        }
        None => {
            group.insert(
                Value::String("use".to_string()),
                Value::Sequence(vec![Value::String(PROVIDER_NAME.to_string())]),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str) -> Mapping {
        let mut m = Mapping::new();
        m.insert(
            Value::String("name".to_string()),
            Value::String(name.to_string()),
        );
        m.insert(
            Value::String("type".to_string()),
            Value::String("vless".to_string()),
        );
        m.insert(
            Value::String("server".to_string()),
            Value::String("hk.example.com".to_string()),
        );
        m.insert(Value::String("port".to_string()), Value::Number(443.into()));
        m
    }

    /// 最小模板配置（带模板标记 + 「节点选择」骨架组，形状对齐 sidecar.rs 首跑模板）
    fn template_config() -> String {
        format!(
            "# Toogl（图格）mihomo 初始配置模板\n\
             # {TEMPLATE_MARKER}，可手工修改；已存在时不会被覆盖。\n\
             mixed-port: 7890\n\
             proxies: []\n\
             proxy-groups:\n  - name: 节点选择\n    type: select\n    proxies:\n      - DIRECT\n\
             rules:\n  - MATCH,节点选择\n"
        )
    }

    fn write_config(dir: &Path, content: &str) -> PathBuf {
        let path = dir.join("config.yaml");
        std::fs::write(&path, content).unwrap();
        path
    }

    // ---- write_nodes_file ----

    #[test]
    fn write_nodes_file_roundtrip_and_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_nodes_file(tmp.path(), &[node("香港 01"), node("日本 01")]).unwrap();
        assert_eq!(path, tmp.path().join(NODES_FILE_NAME));

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with(NODES_FILE_HEADER));
        // 重新解析验证结构正确
        let doc: Mapping = serde_yaml::from_str(&content).unwrap();
        let proxies = doc
            .get(Value::String("proxies".to_string()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0]["name"], Value::String("香港 01".to_string()));
        assert_eq!(proxies[1]["port"], Value::Number(443.into()));

        // 覆盖写（空列表）：proxies: []，tmp 文件不残留
        write_nodes_file(tmp.path(), &[]).unwrap();
        let doc: Mapping = serde_yaml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            doc.get(Value::String("proxies".to_string())),
            Some(&Value::Sequence(vec![]))
        );
        assert!(!tmp.path().join(format!("{NODES_FILE_NAME}.tmp")).exists());
    }

    // ---- ensure_wired ----

    #[test]
    fn ensure_wired_first_run_wires_and_backs_up() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), &template_config());

        assert!(ensure_wired(&config).unwrap());
        assert!(config.with_extension("yaml.bak").exists());

        let wired = std::fs::read_to_string(&config).unwrap();
        // 模板标记行保留在文件顶部（后续升级路径仍识别为本工具模板）
        assert!(wired.lines().next().unwrap().contains(TEMPLATE_MARKER));

        let doc: Mapping = serde_yaml::from_str(&wired).unwrap();
        // proxy-providers.toogl → file provider 指向托管节点文件
        let providers = doc
            .get(Value::String("proxy-providers".to_string()))
            .unwrap()
            .as_mapping()
            .unwrap();
        let toogl = providers
            .get(Value::String(PROVIDER_NAME.to_string()))
            .unwrap()
            .as_mapping()
            .unwrap();
        assert_eq!(
            toogl.get(Value::String("type".to_string())),
            Some(&Value::String("file".to_string()))
        );
        assert_eq!(
            toogl.get(Value::String("path".to_string())),
            Some(&Value::String(NODES_FILE_NAME.to_string()))
        );
        // 「节点选择」组 use 含 toogl
        let groups = doc
            .get(Value::String("proxy-groups".to_string()))
            .unwrap()
            .as_sequence()
            .unwrap();
        let group = groups[0].as_mapping().unwrap();
        let uses = group
            .get(Value::String("use".to_string()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(uses, &[Value::String(PROVIDER_NAME.to_string())]);
        // 原有内容（proxies / rules）不丢
        assert!(doc.contains_key(Value::String("rules".to_string())));
    }

    #[test]
    fn ensure_wired_is_idempotent_on_second_call() {
        let tmp = tempfile::tempdir().unwrap();
        let config = write_config(tmp.path(), &template_config());
        assert!(ensure_wired(&config).unwrap());
        let after_first = std::fs::read_to_string(&config).unwrap();

        // 二次调用：已接线 → Ok(false)，文件不再变化
        assert!(!ensure_wired(&config).unwrap());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), after_first);
    }

    #[test]
    fn ensure_wired_rejects_user_handwritten_config_without_touching_it() {
        let tmp = tempfile::tempdir().unwrap();
        let user = "# 用户手工配置\nproxy-groups:\n  - name: 节点选择\n    type: select\n    proxies: [DIRECT]\n";
        let config = write_config(tmp.path(), user);

        let err = ensure_wired(&config).unwrap_err();
        assert!(matches!(err, SubscriptionError::Wiring(_)));
        // 文案提示手工补接线
        assert!(err.to_string().contains("proxy-providers"));
        // 文件不动、不产生备份
        assert_eq!(std::fs::read_to_string(&config).unwrap(), user);
        assert!(!config.with_extension("yaml.bak").exists());
    }

    #[test]
    fn ensure_wired_errors_when_target_group_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let text = format!(
            "# {TEMPLATE_MARKER}\nproxy-groups:\n  - name: 其他组\n    type: select\n    proxies: [DIRECT]\n"
        );
        let config = write_config(tmp.path(), &text);

        let err = ensure_wired(&config).unwrap_err();
        assert!(matches!(err, SubscriptionError::Wiring(_)));
        assert!(err.to_string().contains(TARGET_GROUP));
        // 失败不写回（备份也没有）
        assert_eq!(std::fs::read_to_string(&config).unwrap(), text);
    }

    #[test]
    fn ensure_wired_recognizes_partial_wiring_as_not_wired() {
        // 有 provider 但组未 use（半接线状态）→ 视为未接线，模板下自动补齐
        let tmp = tempfile::tempdir().unwrap();
        let text = format!(
            "# {TEMPLATE_MARKER}\n\
             proxy-providers:\n  toogl:\n    type: file\n    path: toogl-nodes.yaml\n\
             proxy-groups:\n  - name: 节点选择\n    type: select\n    proxies: [DIRECT]\n"
        );
        let config = write_config(tmp.path(), &text);
        assert!(ensure_wired(&config).unwrap());
        assert!(is_wired(
            &serde_yaml::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap()
        ));
    }
}
