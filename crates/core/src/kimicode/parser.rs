//! wire.jsonl 逐行解析：把 usage.record 行归一化为四类 token 用量。
//! 容错原则（NF-13）：未知字段忽略、非用量行跳过、坏行返回错误由调用方计数，不中断整体扫描。

use serde::Deserialize;

/// 单行解析出的归一化用量（不含 session/project，由扫描器按文件归属补充）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUsage {
    /// Unix 毫秒时间戳
    pub ts: i64,
    pub model: String,
    pub input_other: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
}

/// 坏行标记（JSON 损坏或缺关键字段），仅作为跳过信号，不携带详情
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadLine;

/// wire.jsonl 一行的反序列化视图。字段名以本机真实日志实测为准（见模块级注释）。
#[derive(Debug, Deserialize)]
struct WireLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    model: Option<String>,
    /// Unix 毫秒时间戳
    time: Option<i64>,
    usage: Option<WireUsage>,
    #[serde(rename = "usageScope")]
    usage_scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireUsage {
    #[serde(rename = "inputOther")]
    input_other: Option<i64>,
    output: Option<i64>,
    #[serde(rename = "inputCacheRead")]
    cache_read: Option<i64>,
    #[serde(rename = "inputCacheCreation")]
    cache_creation: Option<i64>,
}

/// 解析结果：
/// - `Ok(Some)`：一条可入库的 turn 级用量事件
/// - `Ok(None)`：非用量行 / session 级快照行 / 空行，正常跳过
/// - `Err(())`：坏行（JSON 损坏或缺关键字段），调用方计数后跳过
pub fn parse_line(line: &str) -> Result<Option<ParsedUsage>, BadLine> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(None);
    }
    let wire: WireLine = serde_json::from_str(line).map_err(|_| BadLine)?;
    if wire.kind.as_deref() != Some("usage.record") {
        return Ok(None);
    }
    // session 级 usage 是会话末次上下文快照（非新增消耗），只统计 turn 级事件
    if wire.usage_scope.as_deref() == Some("session") {
        return Ok(None);
    }
    let Some(ts) = wire.time else {
        // 没有时间戳无法归日，视为坏行
        return Err(BadLine);
    };
    // usage 字段缺失按 0 处理（《02-开发计划》M1 实现要点）
    let usage = wire.usage.unwrap_or(WireUsage {
        input_other: None,
        output: None,
        cache_read: None,
        cache_creation: None,
    });
    Ok(Some(ParsedUsage {
        ts,
        model: wire.model.unwrap_or_else(|| "unknown".to_string()),
        input_other: usage.input_other.unwrap_or(0),
        output: usage.output.unwrap_or(0),
        cache_read: usage.cache_read.unwrap_or(0),
        cache_creation: usage.cache_creation.unwrap_or(0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_turn_usage_record() {
        let line = r#"{"type":"usage.record","model":"kimi-code/kimi-for-coding","usage":{"inputOther":349,"output":152,"inputCacheRead":19712,"inputCacheCreation":0},"usageScope":"turn","time":1789376090151}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert_eq!(parsed.ts, 1789376090151);
        assert_eq!(parsed.model, "kimi-code/kimi-for-coding");
        assert_eq!(parsed.input_other, 349);
        assert_eq!(parsed.output, 152);
        assert_eq!(parsed.cache_read, 19712);
        assert_eq!(parsed.cache_creation, 0);
    }

    #[test]
    fn skips_session_scope_snapshot() {
        // session 级是上下文快照，计入会虚增用量
        let line = r#"{"type":"usage.record","model":"m","usage":{"inputOther":1,"output":2,"inputCacheRead":210432,"inputCacheCreation":0},"usageScope":"session","time":1783417504960}"#;
        assert_eq!(parse_line(line), Ok(None));
    }

    #[test]
    fn missing_usage_treated_as_zero() {
        let line = r#"{"type":"usage.record","model":"m","time":1000,"usageScope":"turn"}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert_eq!(parsed.input_other, 0);
        assert_eq!(parsed.output, 0);
        assert_eq!(parsed.cache_read, 0);
        assert_eq!(parsed.cache_creation, 0);
    }

    #[test]
    fn missing_model_falls_back_to_unknown() {
        let line = r#"{"type":"usage.record","usage":{"inputOther":1,"output":1,"inputCacheRead":0,"inputCacheCreation":0},"time":1000}"#;
        let parsed = parse_line(line).unwrap().unwrap();
        assert_eq!(parsed.model, "unknown");
    }

    #[test]
    fn skips_non_usage_lines_and_empty_lines() {
        assert_eq!(parse_line(""), Ok(None));
        assert_eq!(parse_line("   "), Ok(None));
        let other = r#"{"type":"session.start","time":1000}"#;
        assert_eq!(parse_line(other), Ok(None));
    }

    #[test]
    fn corrupted_json_is_bad_line() {
        assert!(parse_line("{not json").is_err());
        assert!(parse_line(r#"{"type":"usage.record","time":"#).is_err());
    }

    #[test]
    fn usage_record_without_time_is_bad_line() {
        let line = r#"{"type":"usage.record","model":"m","usage":{"inputOther":1}}"#;
        assert!(parse_line(line).is_err());
    }
}
