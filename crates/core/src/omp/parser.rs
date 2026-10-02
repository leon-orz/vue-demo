//! omp 会话 jsonl 逐行解析：把 assistant 消息行归一化为四类 token 用量。
//! 容错原则（NF-13）：未知字段忽略、非用量行跳过、坏行返回错误由调用方计数，不中断整体扫描。
//!
//! 异常行约定：assistant 行的 stopReason 非 "stop"/"toolUse"（如 "aborted"）或带 errorMessage
//! 时视为异常行——解析结果置 `error_stop = true`，由扫描器计入 bad_lines 诊断指标
//! （ScanReport 无独立异常行字段，与 JSON 损坏行合并计数）；事件本身带 usage 仍正常入库
//! （token 已实际消耗，如限流中断前的部分输出）。

use serde::Deserialize;

/// 单行解析出的归一化用量（不含 session/project，由扫描器按文件归属补充）
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedUsage {
    /// Unix 毫秒时间戳
    pub ts: i64,
    /// 模型名：provider/model（如 "deepseek/deepseek-v4-flash"），缺 provider 时仅 model，缺省 "unknown"
    pub model: String,
    pub input_other: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    /// 日志自带成本（usage.cost.total，美元）；omp 经 OpenRouter/官方直连时由 omp 计算
    pub cost: Option<f64>,
    /// 异常行标记：stopReason 非 stop/toolUse 或带 errorMessage
    pub error_stop: bool,
}

/// 坏行标记（JSON 损坏或缺关键字段），仅作为跳过信号，不携带详情
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadLine;

/// 会话元信息（type=="session" 行的子集）：id 提供会话标识，cwd 提供项目名
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionMeta {
    pub id: Option<String>,
    pub cwd: Option<String>,
}

/// 会话 jsonl 一行的反序列化视图。字段名以本机真实日志实测为准（见模块级注释）。
#[derive(Debug, Deserialize)]
struct OmpLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    /// 顶层 RFC3339 时间戳（message.timestamp 缺失时的兜底）
    timestamp: Option<String>,
    /// session 元信息行字段
    id: Option<String>,
    cwd: Option<String>,
    message: Option<OmpMessage>,
}

#[derive(Debug, Deserialize)]
struct OmpMessage {
    role: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    #[serde(rename = "stopReason")]
    stop_reason: Option<String>,
    /// 只判存在性，不解析内容
    #[serde(rename = "errorMessage")]
    error_message: Option<serde::de::IgnoredAny>,
    /// Unix 毫秒时间戳（assistant 行实测均有）
    timestamp: Option<i64>,
    usage: Option<OmpUsage>,
}

#[derive(Debug, Deserialize)]
struct OmpUsage {
    input: Option<i64>,
    output: Option<i64>,
    #[serde(rename = "cacheRead")]
    cache_read: Option<i64>,
    #[serde(rename = "cacheWrite")]
    cache_creation: Option<i64>,
    cost: Option<OmpCost>,
}

#[derive(Debug, Deserialize)]
struct OmpCost {
    total: Option<f64>,
}

/// 正常结束标记：非此集合的 stopReason 视为异常行
const NORMAL_STOP_REASONS: [&str; 2] = ["stop", "toolUse"];

/// 解析会话元信息行（type=="session"）。非 session 行或解析失败返回 None。
pub fn parse_session_meta(line: &str) -> Option<SessionMeta> {
    let parsed: OmpLine = serde_json::from_str(line.trim()).ok()?;
    if parsed.kind.as_deref() != Some("session") {
        return None;
    }
    Some(SessionMeta {
        id: parsed.id,
        cwd: parsed.cwd,
    })
}

/// 解析用量事件行：
/// - `Ok(Some)`：一条可入库的 assistant 用量事件
/// - `Ok(None)`：非 assistant 消息行 / 空行，正常跳过
/// - `Err(BadLine)`：坏行（JSON 损坏或无可用时间戳），调用方计数后跳过
pub fn parse_line(line: &str) -> Result<Option<ParsedUsage>, BadLine> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let parsed: OmpLine = serde_json::from_str(line).map_err(|_| BadLine)?;
    if parsed.kind.as_deref() != Some("message") {
        return Ok(None);
    }
    let Some(msg) = parsed.message else {
        return Ok(None);
    };
    if msg.role.as_deref() != Some("assistant") {
        return Ok(None);
    }
    // 时间戳优先取 message.timestamp（Unix 毫秒），缺失时回退解析顶层 RFC3339 时间戳
    let ts = match msg.timestamp {
        Some(ts) => ts,
        None => parsed
            .timestamp
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.timestamp_millis())
            // 没有时间戳无法归日，视为坏行
            .ok_or(BadLine)?,
    };
    // usage 字段缺失按 0 处理（《02-开发计划》M1 实现要点）
    let usage = msg.usage.unwrap_or(OmpUsage {
        input: None,
        output: None,
        cache_read: None,
        cache_creation: None,
        cost: None,
    });
    let model = match (msg.provider.as_deref(), msg.model.as_deref()) {
        (Some(provider), Some(model)) => format!("{provider}/{model}"),
        (_, Some(model)) => model.to_string(),
        _ => "unknown".to_string(),
    };
    let error_stop = msg.error_message.is_some()
        || msg
            .stop_reason
            .as_deref()
            .is_some_and(|s| !NORMAL_STOP_REASONS.contains(&s));
    Ok(Some(ParsedUsage {
        ts,
        model,
        input_other: usage.input.unwrap_or(0),
        output: usage.output.unwrap_or(0),
        cache_read: usage.cache_read.unwrap_or(0),
        cache_creation: usage.cache_creation.unwrap_or(0),
        cost: usage.cost.and_then(|c| c.total),
        error_stop,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_assistant_usage_line() {
        // 脱敏后的真实行形态
        let line = r#"{"type":"message","id":"28deb559","timestamp":"2026-09-21T07:32:01.237Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}],"api":"openai-completions","provider":"deepseek","model":"deepseek-flash","usage":{"input":277,"output":20,"cacheRead":18176,"cacheWrite":0,"totalTokens":18473,"cost":{"input":0.0000831,"output":0.000024,"cacheRead":0.000109056,"cacheWrite":0,"total":0.000216156}},"stopReason":"stop","timestamp":1789975920370}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert_eq!(parsed.ts, 1789975920370);
        assert_eq!(parsed.model, "deepseek/deepseek-flash");
        assert_eq!(parsed.input_other, 277);
        assert_eq!(parsed.output, 20);
        assert_eq!(parsed.cache_read, 18176);
        assert_eq!(parsed.cache_creation, 0);
        assert_eq!(parsed.cost, Some(0.000216156));
        assert!(!parsed.error_stop);
    }

    #[test]
    fn tool_use_stop_reason_is_normal() {
        let line = r#"{"type":"message","timestamp":"2026-09-21T07:18:03.566Z","message":{"role":"assistant","content":[],"provider":"deepseek","model":"m","usage":{"input":100,"output":10,"cacheRead":0,"cacheWrite":5},"stopReason":"toolUse","timestamp":1789975081139}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert!(!parsed.error_stop);
        assert_eq!(parsed.cache_creation, 5);
        assert_eq!(parsed.cost, None);
    }

    #[test]
    fn missing_usage_treated_as_zero() {
        let line = r#"{"type":"message","timestamp":"2026-09-21T07:18:03.566Z","message":{"role":"assistant","model":"m","stopReason":"stop","timestamp":1000}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert_eq!(parsed.input_other, 0);
        assert_eq!(parsed.output, 0);
        assert_eq!(parsed.cache_read, 0);
        assert_eq!(parsed.cache_creation, 0);
        assert_eq!(parsed.cost, None);
    }

    #[test]
    fn error_stop_reason_flagged() {
        let line = r#"{"type":"message","timestamp":"2026-09-21T07:45:24.126Z","message":{"role":"assistant","content":[],"provider":"deepseek","model":"deepseek-v4-flash","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"cost":{"total":0}},"stopReason":"aborted","errorMessage":"Interrupted by user","timestamp":1789976724123}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert!(parsed.error_stop);
    }

    #[test]
    fn error_message_without_stop_reason_flagged() {
        let line = r#"{"type":"message","message":{"role":"assistant","model":"m","usage":{"input":1,"output":1},"errorMessage":"rate limited","timestamp":1000}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert!(parsed.error_stop);
    }

    #[test]
    fn falls_back_to_top_level_rfc3339_timestamp() {
        // message.timestamp 缺失时用顶层 ISO 时间戳兜底
        let line = r#"{"type":"message","timestamp":"2026-09-21T07:32:01.237Z","message":{"role":"assistant","model":"m","usage":{"input":1,"output":1},"stopReason":"stop"}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        let expected = chrono::DateTime::parse_from_rfc3339("2026-09-21T07:32:01.237Z")
            .unwrap()
            .timestamp_millis();
        assert_eq!(parsed.ts, expected);
    }

    #[test]
    fn model_without_provider_used_as_is() {
        let line = r#"{"type":"message","message":{"role":"assistant","model":"solo-model","usage":{"input":1,"output":1},"timestamp":1000}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert_eq!(parsed.model, "solo-model");
    }

    #[test]
    fn missing_model_falls_back_to_unknown() {
        let line = r#"{"type":"message","message":{"role":"assistant","usage":{"input":1,"output":1},"timestamp":1000}}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert_eq!(parsed.model, "unknown");
    }

    #[test]
    fn skips_non_assistant_lines_and_empty_lines() {
        assert_eq!(parse_line(""), Ok(None));
        assert_eq!(parse_line("   "), Ok(None));
        // 用户 / 工具结果消息行
        let user = r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"hi"}]},"timestamp":"t"}"#;
        assert_eq!(parse_line(user), Ok(None));
        let tool = r#"{"type":"message","message":{"role":"toolResult","toolName":"bash","content":[]},"timestamp":"t"}"#;
        assert_eq!(parse_line(tool), Ok(None));
        // 其他类型行
        let session = r#"{"type":"session","id":"s1","cwd":"C:/work/app"}"#;
        assert_eq!(parse_line(session), Ok(None));
        let title = r#"{"type":"title","title":"t"}"#;
        assert_eq!(parse_line(title), Ok(None));
    }

    #[test]
    fn corrupted_json_is_bad_line() {
        assert!(parse_line("{not json").is_err());
        assert!(parse_line(r#"{"type":"message","message":"#).is_err());
    }

    #[test]
    fn assistant_line_without_any_timestamp_is_bad_line() {
        let line =
            r#"{"type":"message","message":{"role":"assistant","model":"m","usage":{"input":1}}}"#;
        assert!(parse_line(line).is_err());
    }

    #[test]
    fn parses_session_meta() {
        let line = r#"{"type":"session","version":3,"id":"01a0c2c0-2122-76df-8355-758ac3337b52","timestamp":"2026-09-21T06:56:06.434Z","cwd":"C:\\zone\\2demo\\game-demo","title":"t"}"#;
        let meta = parse_session_meta(line).unwrap();
        assert_eq!(
            meta.id.as_deref(),
            Some("01a0c2c0-2122-76df-8355-758ac3337b52")
        );
        assert_eq!(meta.cwd.as_deref(), Some("C:\\zone\\2demo\\game-demo"));
    }

    #[test]
    fn session_meta_ignores_other_lines() {
        assert_eq!(parse_session_meta(r#"{"type":"title","title":"t"}"#), None);
        assert_eq!(parse_session_meta("{broken"), None);
    }
}
