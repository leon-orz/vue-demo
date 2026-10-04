//! 分享链接解析（纯函数）：vless / hysteria2(hy2) / trojan / ss → mihomo proxies 条目。
//!
//! 输出为 serde_yaml::Mapping（serde_yaml::Mapping 保插入序，键序即下面的写入序），
//! 字段与 mihomo 配置一一对应。未知 query 参数忽略；缺关键字段（server / port / uuid 等）
//! 返回 Link 错误，由上层逐条容错。
//!
//! 节点名取 URL fragment（# 后，percent-decoded），缺省回退 `server:port`。

use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine as _;
use serde_yaml::{Mapping, Value};

use super::SubscriptionError;

/// 解析单条分享链接为 mihomo proxies 条目（vless / hysteria2 / hy2 / trojan / ss）
pub fn parse_share_link(link: &str) -> Result<Mapping, SubscriptionError> {
    let link = link.trim();
    if let Some(rest) = link.strip_prefix("ss://") {
        return parse_ss(rest);
    }
    let url =
        url::Url::parse(link).map_err(|e| SubscriptionError::Link(format!("URL 解析失败: {e}")))?;
    match url.scheme() {
        "vless" => parse_vless(&url),
        "hysteria2" | "hy2" => parse_hysteria2(&url),
        "trojan" => parse_trojan(&url),
        scheme => Err(SubscriptionError::Link(format!("不支持的协议: {scheme}"))),
    }
}

// ---- 通用小工具 ----

fn put(m: &mut Mapping, key: &str, value: Value) {
    m.insert(Value::String(key.to_string()), value);
}

fn put_str(m: &mut Mapping, key: &str, value: &str) {
    put(m, key, Value::String(value.to_string()));
}

fn put_bool(m: &mut Mapping, key: &str, value: bool) {
    put(m, key, Value::Bool(value));
}

fn put_num(m: &mut Mapping, key: &str, value: u64) {
    put(m, key, Value::Number(value.into()));
}

/// query 参数取值（首个同名参数）
fn q<'a>(params: &'a [(String, String)], key: &str) -> Option<&'a str> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

/// query 参数收集（percent + form 解码）
fn params_of(url: &url::Url) -> Vec<(String, String)> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// 节点名：fragment percent-decoded；缺省回退 server:port
fn node_name(url: &url::Url, server: &str, port: u16) -> String {
    match url.fragment() {
        Some(f) if !f.is_empty() => percent_decode(f),
        _ => format!("{server}:{port}"),
    }
}

/// percent 解码（fragment / ss 用户信息用；不处理 form 的 '+' 语义）
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// allowInsecure / insecure 参数 → skip-cert-verify
fn truthy(v: Option<&str>) -> bool {
    matches!(v, Some("1") | Some("true"))
}

/// alpn 参数（逗号分隔）→ YAML 字符串序列
fn put_alpn(m: &mut Mapping, params: &[(String, String)]) {
    if let Some(alpn) = q(params, "alpn") {
        let list: Vec<Value> = alpn
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| Value::String(s.to_string()))
            .collect();
        if !list.is_empty() {
            put(m, "alpn", Value::Sequence(list));
        }
    }
}

/// 公共尾部：server / port 已从 URL 取出后的 host 提取（IPv6 去方括号）
fn server_of(url: &url::Url) -> Result<String, SubscriptionError> {
    url.host_str()
        .map(|h| h.to_string())
        .ok_or_else(|| SubscriptionError::Link("缺服务器地址".to_string()))
}

fn port_of(url: &url::Url) -> Result<u16, SubscriptionError> {
    url.port()
        .ok_or_else(|| SubscriptionError::Link("缺端口".to_string()))
}

/// network（type 参数）与对应传输层 opts 写入（ws / grpc / h2；tcp 缺省不写）
fn put_network(m: &mut Mapping, params: &[(String, String)]) {
    let Some(network) = q(params, "type") else {
        return;
    };
    if network == "tcp" {
        return;
    }
    put_str(m, "network", network);
    match network {
        "ws" => {
            let mut ws = Mapping::new();
            if let Some(path) = q(params, "path") {
                put_str(&mut ws, "path", path);
            }
            if let Some(host) = q(params, "host") {
                let mut headers = Mapping::new();
                put_str(&mut headers, "Host", host);
                put(&mut ws, "headers", Value::Mapping(headers));
            }
            put(m, "ws-opts", Value::Mapping(ws));
        }
        "grpc" => {
            let mut grpc = Mapping::new();
            if let Some(name) = q(params, "serviceName") {
                put_str(&mut grpc, "grpc-service-name", name);
            }
            put(m, "grpc-opts", Value::Mapping(grpc));
        }
        "h2" | "http" => {
            let mut h2 = Mapping::new();
            if let Some(host) = q(params, "host") {
                put(
                    &mut h2,
                    "host",
                    Value::Sequence(vec![Value::String(host.to_string())]),
                );
            }
            if let Some(path) = q(params, "path") {
                put_str(&mut h2, "path", path);
            }
            put(m, "h2-opts", Value::Mapping(h2));
        }
        _ => {}
    }
}

// ---- vless ----
// vless://uuid@server:port?security=tls|reality&flow=...&sni=...&fp=...&pbk=...&sid=...&type=ws&path=...#name

fn parse_vless(url: &url::Url) -> Result<Mapping, SubscriptionError> {
    let uuid = percent_decode(url.username());
    if uuid.is_empty() {
        return Err(SubscriptionError::Link("vless 缺 uuid".to_string()));
    }
    let server = server_of(url)?;
    let port = port_of(url)?;
    let params = params_of(url);

    let mut m = Mapping::new();
    put_str(&mut m, "name", &node_name(url, &server, port));
    put_str(&mut m, "type", "vless");
    put_str(&mut m, "server", &server);
    put_num(&mut m, "port", u64::from(port));
    put_str(&mut m, "uuid", &uuid);
    put_bool(&mut m, "udp", true);

    let security = q(&params, "security").unwrap_or("");
    if security == "tls" || security == "reality" {
        put_bool(&mut m, "tls", true);
    }
    if let Some(flow) = q(&params, "flow") {
        put_str(&mut m, "flow", flow);
    }
    if let Some(sni) = q(&params, "sni") {
        put_str(&mut m, "servername", sni);
    }
    if let Some(fp) = q(&params, "fp") {
        put_str(&mut m, "client-fingerprint", fp);
    }
    if security == "reality" {
        let mut reality = Mapping::new();
        if let Some(pbk) = q(&params, "pbk") {
            put_str(&mut reality, "public-key", pbk);
        }
        if let Some(sid) = q(&params, "sid") {
            put_str(&mut reality, "short-id", sid);
        }
        put(&mut m, "reality-opts", Value::Mapping(reality));
    }
    if truthy(q(&params, "allowInsecure")) {
        put_bool(&mut m, "skip-cert-verify", true);
    }
    put_alpn(&mut m, &params);
    put_network(&mut m, &params);
    Ok(m)
}

// ---- hysteria2 / hy2 ----
// hysteria2://password@server:port?sni=...&insecure=1&obfs=salamander&obfs-password=...#name

fn parse_hysteria2(url: &url::Url) -> Result<Mapping, SubscriptionError> {
    // 分享链接形态 hysteria2://password@server:port：密码在 URL 的 username 位置
    let password = percent_decode(url.username());
    if password.is_empty() {
        return Err(SubscriptionError::Link("hysteria2 缺密码".to_string()));
    }
    let server = server_of(url)?;
    let port = port_of(url)?;
    let params = params_of(url);

    let mut m = Mapping::new();
    put_str(&mut m, "name", &node_name(url, &server, port));
    put_str(&mut m, "type", "hysteria2");
    put_str(&mut m, "server", &server);
    put_num(&mut m, "port", u64::from(port));
    put_str(&mut m, "password", &password);
    if let Some(sni) = q(&params, "sni") {
        put_str(&mut m, "sni", sni);
    }
    if truthy(q(&params, "insecure")) {
        put_bool(&mut m, "skip-cert-verify", true);
    }
    if let Some(obfs) = q(&params, "obfs") {
        put_str(&mut m, "obfs", obfs);
        if let Some(obfs_password) = q(&params, "obfs-password") {
            put_str(&mut m, "obfs-password", obfs_password);
        }
    }
    put_bool(&mut m, "udp", true);
    Ok(m)
}

// ---- trojan ----
// trojan://password@server:port?sni=...&allowInsecure=1&type=ws&path=...&host=...#name

fn parse_trojan(url: &url::Url) -> Result<Mapping, SubscriptionError> {
    // 分享链接形态 trojan://password@server:port：密码在 URL 的 username 位置
    let password = percent_decode(url.username());
    if password.is_empty() {
        return Err(SubscriptionError::Link("trojan 缺密码".to_string()));
    }
    let server = server_of(url)?;
    let port = port_of(url)?;
    let params = params_of(url);

    let mut m = Mapping::new();
    put_str(&mut m, "name", &node_name(url, &server, port));
    put_str(&mut m, "type", "trojan");
    put_str(&mut m, "server", &server);
    put_num(&mut m, "port", u64::from(port));
    put_str(&mut m, "password", &password);
    if let Some(sni) = q(&params, "sni") {
        put_str(&mut m, "sni", sni);
    }
    if truthy(q(&params, "allowInsecure")) {
        put_bool(&mut m, "skip-cert-verify", true);
    }
    put_alpn(&mut m, &params);
    put_bool(&mut m, "udp", true);
    put_network(&mut m, &params);
    Ok(m)
}

// ---- ss ----
// 两种形态（SIP002 与旧版整段 base64）：
//   ss://base64(method:password)@server:port/?plugin=...#name
//   ss://base64(method:password@server:port)#name

/// base64 解码：依次尝试 url-safe / 标准、有/无填充四种形态
fn b64_decode(s: &str) -> Option<String> {
    let trimmed = s.trim();
    for engine in [&URL_SAFE_NO_PAD, &URL_SAFE, &STANDARD_NO_PAD, &STANDARD] {
        if let Ok(bytes) = engine.decode(trimmed) {
            if let Ok(text) = String::from_utf8(bytes) {
                return Some(text);
            }
        }
    }
    None
}

fn parse_ss(rest: &str) -> Result<Mapping, SubscriptionError> {
    // 摘出 fragment（节点名）与 query（plugin 等）
    let (rest, fragment) = match rest.split_once('#') {
        Some((head, frag)) => (head, Some(percent_decode(frag))),
        None => (rest, None),
    };
    let (rest, query) = match rest.split_once('?') {
        Some((head, qs)) => (head, Some(qs)),
        None => (rest, None),
    };

    // 拆出用户信息（method:password）与 server:port
    let (userinfo, server_port) = if let Some((user, sp)) = rest.rsplit_once('@') {
        // SIP002：@ 前可能是 base64 编码的用户信息，也可能是明文
        (
            b64_decode(user).unwrap_or_else(|| percent_decode(user)),
            sp.to_string(),
        )
    } else {
        // 旧版：整段 base64(method:password@server:port)
        let decoded = b64_decode(rest)
            .ok_or_else(|| SubscriptionError::Link("ss 链接 base64 解码失败".to_string()))?;
        let (user, sp) = decoded
            .rsplit_once('@')
            .ok_or_else(|| SubscriptionError::Link("ss 链接缺 @ 分隔".to_string()))?;
        (user.to_string(), sp.to_string())
    };

    let (method, password) = userinfo
        .split_once(':')
        .ok_or_else(|| SubscriptionError::Link("ss 用户信息缺 method:password".to_string()))?;
    let (server, port_str) = server_port
        .rsplit_once(':')
        .ok_or_else(|| SubscriptionError::Link("ss 缺 server:port".to_string()))?;
    let port: u16 = port_str
        .trim_end_matches('/')
        .parse()
        .map_err(|_| SubscriptionError::Link(format!("ss 端口非法: {port_str}")))?;
    let server = server.trim_matches(|c| c == '[' || c == ']');

    let name = fragment
        .filter(|f| !f.is_empty())
        .unwrap_or_else(|| format!("{server}:{port}"));

    let mut m = Mapping::new();
    put_str(&mut m, "name", &name);
    put_str(&mut m, "type", "ss");
    put_str(&mut m, "server", server);
    put_num(&mut m, "port", u64::from(port));
    put_str(&mut m, "cipher", method);
    put_str(&mut m, "password", password);
    put_bool(&mut m, "udp", true);

    // plugin 参数（form 解码后的形如 obfs-local;obfs=http;obfs-host=xxx）
    if let Some(qs) = query {
        let params: Vec<(String, String)> = url::form_urlencoded::parse(qs.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        if let Some(plugin) = q(&params, "plugin") {
            put_ss_plugin(&mut m, plugin)?;
        }
    }
    Ok(m)
}

/// ss plugin 映射：simple-obfs/obfs-local → obfs；v2ray-plugin → v2ray-plugin；
/// 未知插件报错（静默丢弃插件会得到一个连不上的节点，不如明确失败）
fn put_ss_plugin(m: &mut Mapping, plugin: &str) -> Result<(), SubscriptionError> {
    let mut parts = plugin.split(';');
    let kind = parts.next().unwrap_or_default().trim();
    let kv: Vec<(&str, &str)> = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim(), v.trim()))
        .collect();
    let get = |key: &str| kv.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);

    let mut opts = Mapping::new();
    match kind {
        "simple-obfs" | "obfs-local" => {
            put_str(m, "plugin", "obfs");
            put_str(&mut opts, "mode", get("obfs").unwrap_or("http"));
            if let Some(host) = get("obfs-host") {
                put_str(&mut opts, "host", host);
            }
        }
        "v2ray-plugin" => {
            put_str(m, "plugin", "v2ray-plugin");
            put_str(&mut opts, "mode", get("mode").unwrap_or("websocket"));
            if let Some(host) = get("host") {
                put_str(&mut opts, "host", host);
            }
            if get("tls").is_some() {
                put_bool(&mut opts, "tls", true);
            }
        }
        other => {
            return Err(SubscriptionError::Link(format!("ss 插件不支持: {other}")));
        }
    }
    put(m, "plugin-opts", Value::Mapping(opts));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(m: &'a Mapping, key: &str) -> &'a Value {
        m.get(Value::String(key.to_string())).unwrap()
    }

    fn get_str<'a>(m: &'a Mapping, key: &str) -> &'a str {
        get(m, key).as_str().unwrap()
    }

    // ---- vless ----

    #[test]
    fn vless_tls_ws_full() {
        let m = parse_share_link(
            "vless://uuid-123@hk.example.com:443?security=tls&sni=cdn.example.com&fp=chrome&flow=xtls-rprx-vision&type=ws&path=%2Fray&host=cdn.example.com#%E9%A6%99%E6%B8%AF%2001",
        )
        .unwrap();
        assert_eq!(get_str(&m, "name"), "香港 01");
        assert_eq!(get_str(&m, "type"), "vless");
        assert_eq!(get_str(&m, "server"), "hk.example.com");
        assert_eq!(get(&m, "port").as_u64(), Some(443));
        assert_eq!(get_str(&m, "uuid"), "uuid-123");
        assert_eq!(get(&m, "tls").as_bool(), Some(true));
        assert_eq!(get_str(&m, "flow"), "xtls-rprx-vision");
        assert_eq!(get_str(&m, "servername"), "cdn.example.com");
        assert_eq!(get_str(&m, "client-fingerprint"), "chrome");
        assert_eq!(get_str(&m, "network"), "ws");
        let ws = get(&m, "ws-opts").as_mapping().unwrap();
        assert_eq!(get_str(ws, "path"), "/ray");
        let headers = get(ws, "headers").as_mapping().unwrap();
        assert_eq!(get_str(headers, "Host"), "cdn.example.com");
    }

    #[test]
    fn vless_reality() {
        let m = parse_share_link(
            "vless://uuid-r@jp.example.com:443?security=reality&pbk=PUBLIC_KEY&sid=ab12&fp=firefox#JP",
        )
        .unwrap();
        assert_eq!(get(&m, "tls").as_bool(), Some(true));
        let reality = get(&m, "reality-opts").as_mapping().unwrap();
        assert_eq!(get_str(reality, "public-key"), "PUBLIC_KEY");
        assert_eq!(get_str(reality, "short-id"), "ab12");
        // 未指定 type 时不写 network（默认 tcp）
        assert!(m.get(Value::String("network".into())).is_none());
    }

    #[test]
    fn vless_missing_uuid_and_port_are_errors() {
        assert!(parse_share_link("vless://@host:443").is_err());
        assert!(parse_share_link("vless://uuid@host").is_err());
    }

    #[test]
    fn vless_name_fallback_to_server_port() {
        let m = parse_share_link("vless://u@1.2.3.4:8443?security=none").unwrap();
        assert_eq!(get_str(&m, "name"), "1.2.3.4:8443");
        // security 非 tls/reality → 不写 tls
        assert!(m.get(Value::String("tls".into())).is_none());
    }

    // ---- hysteria2 ----

    #[test]
    fn hysteria2_with_obfs_and_insecure() {
        let m = parse_share_link(
            "hysteria2://p%40ss@hy.example.com:1443?sni=real.example.com&insecure=1&obfs=salamander&obfs-password=secret#Hy2",
        )
        .unwrap();
        assert_eq!(get_str(&m, "name"), "Hy2");
        assert_eq!(get_str(&m, "type"), "hysteria2");
        assert_eq!(get_str(&m, "password"), "p@ss");
        assert_eq!(get_str(&m, "sni"), "real.example.com");
        assert_eq!(get(&m, "skip-cert-verify").as_bool(), Some(true));
        assert_eq!(get_str(&m, "obfs"), "salamander");
        assert_eq!(get_str(&m, "obfs-password"), "secret");
        assert_eq!(get(&m, "udp").as_bool(), Some(true));
    }

    #[test]
    fn hy2_alias_works() {
        let m = parse_share_link("hy2://pw@host:443").unwrap();
        assert_eq!(get_str(&m, "type"), "hysteria2");
        assert_eq!(get_str(&m, "name"), "host:443");
    }

    // ---- trojan ----

    #[test]
    fn trojan_basic_with_sni() {
        let m =
            parse_share_link("trojan://pw@tj.example.com:443?sni=tj.example.com#Trojan").unwrap();
        assert_eq!(get_str(&m, "type"), "trojan");
        assert_eq!(get_str(&m, "password"), "pw");
        assert_eq!(get_str(&m, "sni"), "tj.example.com");
        assert_eq!(get(&m, "udp").as_bool(), Some(true));
    }

    // ---- ss ----

    #[test]
    fn ss_sip002_base64_userinfo() {
        // base64url("aes-128-gcm:passw0rd")，无填充
        let m = parse_share_link(
            "ss://YWVzLTEyOC1nY206cGFzc3cwcmQ@ss.example.com:8388#SS%E8%8A%82%E7%82%B9",
        )
        .unwrap();
        assert_eq!(get_str(&m, "name"), "SS节点");
        assert_eq!(get_str(&m, "type"), "ss");
        assert_eq!(get_str(&m, "server"), "ss.example.com");
        assert_eq!(get(&m, "port").as_u64(), Some(8388));
        assert_eq!(get_str(&m, "cipher"), "aes-128-gcm");
        assert_eq!(get_str(&m, "password"), "passw0rd");
    }

    #[test]
    fn ss_legacy_whole_base64() {
        // base64("aes-256-gcm:pw@1.2.3.4:1080")（标准编码，带填充）
        let encoded = STANDARD.encode("aes-256-gcm:pw@1.2.3.4:1080");
        let m = parse_share_link(&format!("ss://{encoded}#legacy")).unwrap();
        assert_eq!(get_str(&m, "cipher"), "aes-256-gcm");
        assert_eq!(get_str(&m, "password"), "pw");
        assert_eq!(get_str(&m, "server"), "1.2.3.4");
        assert_eq!(get(&m, "port").as_u64(), Some(1080));
        assert_eq!(get_str(&m, "name"), "legacy");
    }

    #[test]
    fn ss_plain_userinfo_and_plugin() {
        let m = parse_share_link(
            "ss://chacha20-ietf-poly1305:pw@host:443/?plugin=obfs-local%3Bobfs%3Dhttp%3Bobfs-host%3Dcdn.example.com#p",
        )
        .unwrap();
        assert_eq!(get_str(&m, "plugin"), "obfs");
        let opts = get(&m, "plugin-opts").as_mapping().unwrap();
        assert_eq!(get_str(opts, "mode"), "http");
        assert_eq!(get_str(opts, "host"), "cdn.example.com");
    }

    #[test]
    fn ss_unknown_plugin_is_error() {
        assert!(parse_share_link("ss://m:p@host:443/?plugin=mystery").is_err());
    }

    #[test]
    fn ss_garbage_is_error() {
        assert!(parse_share_link("ss://@@@").is_err());
        assert!(parse_share_link("ss://not-base64!!!").is_err());
    }

    // ---- 通用 ----

    #[test]
    fn unsupported_scheme_is_error() {
        let err = parse_share_link("vmess://eyJ2IjoiMiJ9").unwrap_err();
        assert!(err.to_string().contains("vmess"));
    }

    #[test]
    fn percent_decode_handles_utf8_and_keeps_plus() {
        assert_eq!(percent_decode("%E9%A6%99%E6%B8%AF"), "香港");
        // fragment 里 '+' 不是空格（区别于 form 语义）
        assert_eq!(percent_decode("a+b%20c"), "a+b c");
        // 非法 % 序列原样保留
        assert_eq!(percent_decode("100%zz"), "100%zz");
    }
}
