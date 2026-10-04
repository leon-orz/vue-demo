//! 订阅/粘贴内容形态识别：把一段文本解析成 mihomo proxies 条目列表。
//!
//! 三种形态按序识别（命中即返回，不再往下试探）：
//! 1. Clash/mihomo YAML：整体是含 `proxies` 序列的 Mapping，逐条校验提取；
//! 2. base64 整段编码的分享链接列表（机场订阅的常见形态），解码后按行拆；
//! 3. 纯文本分享链接列表，直接按行拆。
//!
//! 逐条容错：坏条目/坏行记入 [`ParsedNodes::failures`]，不拖垮整体；
//! 仅当一条有效节点都没有时才返回 Parse 错误。

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use ts_rs::TS;

use super::{share, SubscriptionError};

/// 单条解析失败记录（坏行/坏条目的原文 + 原因），供前端展示「N 条导入失败」明细
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/core-types/bindings/")]
pub struct NodeFailure {
    /// 失败条目原文（YAML 条目为序列化后的片段，链接行为整行）
    pub line: String,
    /// 失败原因（share::parse_share_link 的 Link 错误文案或 YAML 校验说明）
    pub reason: String,
}

/// 内容解析结果：有效节点 + 失败明细（两者可同时非空）
#[derive(Debug, Clone, Default)]
pub struct ParsedNodes {
    /// 成功解析的 mihomo proxies 条目
    pub nodes: Vec<Mapping>,
    /// 失败明细
    pub failures: Vec<NodeFailure>,
}

/// 节点条目的必备字段（Clash proxies 条目的最小可用集）
const REQUIRED_NODE_KEYS: [&str; 4] = ["name", "type", "server", "port"];

/// 识别内容形态并解析为节点列表。
/// 输入全空（trim 后为空）→ Parse("内容为空")；一条有效节点都解析不出 → Parse 错误。
pub fn parse_nodes_text(text: &str) -> Result<ParsedNodes, SubscriptionError> {
    if text.trim().is_empty() {
        return Err(SubscriptionError::Parse("内容为空".to_string()));
    }
    let parsed = if let Some(p) = try_clash_yaml(text) {
        p
    } else if let Some(p) = try_base64_lines(text) {
        p
    } else {
        parse_link_lines(text)
    };
    if parsed.nodes.is_empty() {
        let reason = if parsed.failures.is_empty() {
            "内容中没有可识别的节点".to_string()
        } else {
            format!("未解析到任何有效节点（{} 条失败）", parsed.failures.len())
        };
        return Err(SubscriptionError::Parse(reason));
    }
    Ok(parsed)
}

/// 形态 1：整体 YAML 解析成功且顶层是含 `proxies` 序列的 Mapping → 逐条校验提取。
/// 不满足任一条件返回 None（交給后续形态；如纯文本链接列表会被 YAML 折叠成标量而非 Mapping）。
fn try_clash_yaml(text: &str) -> Option<ParsedNodes> {
    let value: Value = serde_yaml::from_str(text).ok()?;
    let proxies = value
        .as_mapping()?
        .get(Value::String("proxies".to_string()))?
        .as_sequence()?;
    let mut out = ParsedNodes::default();
    for item in proxies {
        match check_node_entry(item) {
            Ok(node) => out.nodes.push(node),
            Err(reason) => out.failures.push(NodeFailure {
                // 坏条目序列化回单行片段供展示（多行 YAML 压成一行，截断防超长）
                line: serde_yaml::to_string(item)
                    .unwrap_or_default()
                    .replace('\n', " ")
                    .chars()
                    .take(120)
                    .collect(),
                reason,
            }),
        }
    }
    Some(out)
}

/// 校验单条 proxies 条目：必须是 Mapping 且含 name/type/server/port 四个必备字段
fn check_node_entry(item: &Value) -> Result<Mapping, String> {
    let mapping = item
        .as_mapping()
        .ok_or_else(|| "条目不是键值对结构".to_string())?;
    for key in REQUIRED_NODE_KEYS {
        if !mapping.contains_key(Value::String(key.to_string())) {
            return Err(format!("条目缺少必备字段: {key}"));
        }
    }
    Ok(mapping.clone())
}

/// 形态 2：整体 base64 解码（四种编码形态依次试），解码结果需为 UTF-8 且含换行或 `://`
/// （排除误命中：单条链接/随机文本不会被解码成合法内容）。
fn try_base64_lines(text: &str) -> Option<ParsedNodes> {
    let trimmed = text.trim();
    for engine in [&URL_SAFE_NO_PAD, &URL_SAFE, &STANDARD_NO_PAD, &STANDARD] {
        let Ok(bytes) = engine.decode(trimmed) else {
            continue;
        };
        let Ok(decoded) = String::from_utf8(bytes) else {
            continue;
        };
        if decoded.contains('\n') || decoded.contains("://") {
            return Some(parse_link_lines(&decoded));
        }
    }
    None
}

/// 形态 3（兜底）：按行拆分享链接。trim、跳过空行与行首 `#` 注释行
/// （分享链接 fragment 里的 `#` 不在行首，不受影响）；每条走 share::parse_share_link。
fn parse_link_lines(text: &str) -> ParsedNodes {
    let mut out = ParsedNodes::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match share::parse_share_link(line) {
            Ok(node) => out.nodes.push(node),
            Err(e) => out.failures.push(NodeFailure {
                line: line.to_string(),
                reason: e.to_string(),
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINK_VLESS: &str = "vless://uuid-1@hk.example.com:443?security=tls#%E9%A6%99%E6%B8%AF";
    const LINK_SS: &str = "ss://YWVzLTEyOC1nY206cGFzc3cwcmQ@ss.example.com:8388#SS";

    fn names(parsed: &ParsedNodes) -> Vec<&str> {
        parsed
            .nodes
            .iter()
            .map(|n| {
                n.get(Value::String("name".to_string()))
                    .unwrap()
                    .as_str()
                    .unwrap()
            })
            .collect()
    }

    // ---- 形态 1：Clash YAML ----

    #[test]
    fn clash_yaml_proxies_section() {
        let text = r#"
proxies:
  - name: 香港 01
    type: vless
    server: hk.example.com
    port: 443
    uuid: uuid-1
  - name: 日本 01
    type: hysteria2
    server: jp.example.com
    port: 1443
    password: pw
"#;
        let parsed = parse_nodes_text(text).unwrap();
        assert_eq!(names(&parsed), ["香港 01", "日本 01"]);
        assert!(parsed.failures.is_empty());
    }

    #[test]
    fn clash_yaml_bad_entries_go_to_failures() {
        let text = r#"
proxies:
  - name: 好节点
    type: ss
    server: a.example.com
    port: 8388
    cipher: aes-128-gcm
    password: pw
  - name: 缺端口
    type: ss
    server: b.example.com
  - 只是一个字符串
"#;
        let parsed = parse_nodes_text(text).unwrap();
        assert_eq!(names(&parsed), ["好节点"]);
        assert_eq!(parsed.failures.len(), 2);
        assert!(parsed.failures[0].reason.contains("port"));
        assert!(parsed.failures[1].reason.contains("键值对"));
    }

    // ---- 形态 2：base64 整段编码 ----

    #[test]
    fn base64_blob_of_share_links() {
        let blob = format!("{LINK_VLESS}\n{LINK_SS}\n");
        for engine in [&URL_SAFE_NO_PAD, &URL_SAFE, &STANDARD_NO_PAD, &STANDARD] {
            let encoded = engine.encode(&blob);
            let parsed = parse_nodes_text(&encoded).unwrap();
            assert_eq!(
                names(&parsed),
                ["香港", "SS"],
                "编码形态 {engine:?} 应可识别"
            );
        }
    }

    #[test]
    fn base64_blob_with_bad_lines_keeps_partial_failures() {
        let blob = format!("{LINK_VLESS}\nvmess://bad\ngarbage-line\n");
        let encoded = STANDARD.encode(blob);
        let parsed = parse_nodes_text(&encoded).unwrap();
        assert_eq!(names(&parsed), ["香港"]);
        assert_eq!(parsed.failures.len(), 2);
    }

    // ---- 形态 3：纯文本链接列表 ----

    #[test]
    fn plain_text_link_list_with_comments_and_blanks() {
        let text = format!("# 这是注释\n\n{LINK_VLESS}\n   \n{LINK_SS}\n");
        let parsed = parse_nodes_text(&text).unwrap();
        assert_eq!(names(&parsed), ["香港", "SS"]);
        assert!(parsed.failures.is_empty());
    }

    #[test]
    fn plain_text_mixed_good_and_bad_lines() {
        let text = format!("{LINK_VLESS}\nnot-a-link\nvmess://eyJ2IjoiMiJ9\n");
        let parsed = parse_nodes_text(&text).unwrap();
        assert_eq!(names(&parsed), ["香港"]);
        assert_eq!(parsed.failures.len(), 2);
        assert_eq!(parsed.failures[0].line, "not-a-link");
        // 失败原因带协议名，便于用户定位
        assert!(parsed.failures[1].reason.contains("vmess"));
    }

    // ---- 整体失败 ----

    #[test]
    fn all_bad_lines_is_parse_error() {
        let err = parse_nodes_text("garbage-1\ngarbage-2\n").unwrap_err();
        assert!(matches!(err, SubscriptionError::Parse(_)));
        assert!(err.to_string().contains('2'));
    }

    #[test]
    fn empty_and_whitespace_input_is_parse_error() {
        for text in ["", "   ", "\n\n  \n"] {
            let err = parse_nodes_text(text).unwrap_err();
            assert!(matches!(err, SubscriptionError::Parse(_)));
            assert!(err.to_string().contains("内容为空"));
        }
    }
}
