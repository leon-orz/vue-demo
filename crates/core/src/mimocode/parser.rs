//! mimocode.db 的 message.data JSON 解析：归一化为四类 token 用量 + 成本。
//!
//! 本机尚未安装 MiMo Code（真实库文件不可得），结构按《02-开发计划》M1 节描述锁定：
//! message 表 data 字段为 JSON，含 tokens / cost / modelID / providerID。
//! 容错原则（NF-13）：未知字段忽略、无 tokens 的行正常跳过、坏行返回错误由调用方计数，
//! 不中断整体扫描；CLI 升级格式漂移时跑 fixture 测试即可发现。

use serde::Deserialize;

/// 单行解析出的归一化用量（不含 platform/cli，由 store 层入库时补充）
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedUsage {
    /// Unix 毫秒时间戳
    pub ts: i64,
    pub session_id: String,
    pub model: String,
    pub input_other: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_creation: i64,
    /// 日志自带成本（MiMo 套餐进度推算依赖）；无此字段为 None
    pub cost: Option<f64>,
}

/// 坏行标记（JSON 损坏或有 tokens 但缺时间戳），仅作为跳过信号，不携带详情
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadRow;

/// message.data 的反序列化视图。字段名以文档描述为准（见模块级注释）。
#[derive(Debug, Deserialize)]
struct MessageData {
    #[serde(rename = "sessionID")]
    session_id: Option<String>,
    #[serde(rename = "modelID")]
    model_id: Option<String>,
    /// 文档列出的字段，当前聚合维度不含供应商，反序列化以验证结构但暂不使用
    #[serde(rename = "providerID")]
    #[allow(dead_code)]
    provider_id: Option<String>,
    cost: Option<f64>,
    tokens: Option<Tokens>,
    time: Option<MessageTime>,
}

#[derive(Debug, Deserialize)]
struct Tokens {
    input: Option<i64>,
    output: Option<i64>,
    /// 推理 token 计入输出（按生成 token 计费），无此字段为 0
    reasoning: Option<i64>,
    cache: Option<CacheTokens>,
}

#[derive(Debug, Deserialize)]
struct CacheTokens {
    read: Option<i64>,
    write: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MessageTime {
    created: Option<i64>,
    completed: Option<i64>,
}

/// 解析 message 表一行的 data JSON。
/// - `Ok(Some)`：一条可入库的用量事件（ts 为 Unix 毫秒）
/// - `Ok(None)`：无 tokens 的行（用户消息 / 系统消息等），正常跳过
/// - `Err(BadRow)`：坏行（JSON 损坏，或有 tokens 但没有任何可用时间戳），调用方计数后跳过
pub fn parse_data(data: &str) -> Result<Option<ParsedUsage>, BadRow> {
    let msg: MessageData = serde_json::from_str(data).map_err(|_| BadRow)?;
    let Some(tokens) = msg.tokens else {
        // 无 tokens 的行不是用量事件（用户提问等），正常跳过
        return Ok(None);
    };
    // 时间戳优先 completed（消耗实际发生时点），回退 created；两者都缺则无法归日，视为坏行
    let ts = msg
        .time
        .and_then(|t| t.completed.or(t.created))
        .ok_or(BadRow)?;
    let cache = tokens.cache.unwrap_or(CacheTokens {
        read: None,
        write: None,
    });
    Ok(Some(ParsedUsage {
        ts,
        session_id: msg.session_id.unwrap_or_else(|| "unknown".to_string()),
        model: msg.model_id.unwrap_or_else(|| "unknown".to_string()),
        input_other: tokens.input.unwrap_or(0),
        output: tokens.output.unwrap_or(0) + tokens.reasoning.unwrap_or(0),
        cache_read: cache.read.unwrap_or(0),
        cache_creation: cache.write.unwrap_or(0),
        cost: msg.cost,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_assistant_message() {
        let data = r#"{
            "sessionID": "ses_1",
            "role": "assistant",
            "modelID": "mimo-v2-flash",
            "providerID": "xiaomi",
            "cost": 0.0012,
            "tokens": {"input": 1000, "output": 200, "reasoning": 50,
                       "cache": {"read": 800, "write": 100}},
            "time": {"created": 1700000000000, "completed": 1700000005000}
        }"#;
        let parsed = parse_data(data).unwrap().unwrap();
        assert_eq!(parsed.ts, 1700000005000); // completed 优先
        assert_eq!(parsed.session_id, "ses_1");
        assert_eq!(parsed.model, "mimo-v2-flash");
        assert_eq!(parsed.input_other, 1000);
        assert_eq!(parsed.output, 250); // reasoning 计入输出
        assert_eq!(parsed.cache_read, 800);
        assert_eq!(parsed.cache_creation, 100);
        assert_eq!(parsed.cost, Some(0.0012));
    }

    #[test]
    fn falls_back_to_time_created() {
        let data = r#"{"tokens": {"input": 1, "output": 2}, "time": {"created": 1700000000000}}"#;
        let parsed = parse_data(data).unwrap().unwrap();
        assert_eq!(parsed.ts, 1700000000000);
    }

    #[test]
    fn skips_row_without_tokens() {
        // 用户消息无 tokens，正常跳过而非坏行
        let data = r#"{"role": "user", "time": {"created": 1700000000000}}"#;
        assert_eq!(parse_data(data), Ok(None));
    }

    #[test]
    fn missing_fields_default() {
        // 缺 cache / cost / modelID / sessionID：按 0 / None / "unknown" 处理
        let data = r#"{"tokens": {"input": 10, "output": 5}, "time": {"created": 1000}}"#;
        let parsed = parse_data(data).unwrap().unwrap();
        assert_eq!(parsed.cache_read, 0);
        assert_eq!(parsed.cache_creation, 0);
        assert_eq!(parsed.cost, None);
        assert_eq!(parsed.model, "unknown");
        assert_eq!(parsed.session_id, "unknown");
    }

    #[test]
    fn corrupted_json_is_bad_row() {
        assert!(parse_data("{not json").is_err());
        assert!(parse_data("").is_err());
    }

    #[test]
    fn tokens_without_any_time_is_bad_row() {
        // 有 tokens 但无任何时间戳：无法归日，计为坏行
        let data = r#"{"tokens": {"input": 1, "output": 1}, "modelID": "m"}"#;
        assert!(parse_data(data).is_err());
    }
}
