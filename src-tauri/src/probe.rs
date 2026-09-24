//! 注册表连通性探测与代理解析。
//!
//! 本机直连 `registry.npmjs.org` 会失败，而命令行工具之所以能工作，是因为环境里
//! 有代理与关闭 TLS 校验的变量。应用自己发请求时没有这些，所以要显式解析代理并
//! 允许单独开关"跳过证书校验"，把结果如实暴露给设置页。

use serde::Serialize;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyReport {
    pub proxy: Option<String>,
    /// env / system / none
    pub source: String,
    pub from_env: Option<String>,
    pub from_system: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub url: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub elapsed_ms: u64,
    pub proxy: Option<String>,
    pub insecure: bool,
    pub sample: Option<String>,
    pub error: Option<String>,
}

/// 读取环境变量里的代理设置。
pub fn env_proxy() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|value| !value.is_empty()))
}

/// 读取 macOS 系统代理设置（`scutil --proxy`）。
pub fn system_proxy() -> Option<String> {
    let output = Command::new("/usr/sbin/scutil").arg("--proxy").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_scutil(&String::from_utf8_lossy(&output.stdout))
}

fn parse_scutil(text: &str) -> Option<String> {
    let mut https_enabled = false;
    let mut https_host = None;
    let mut https_port = None;
    let mut http_enabled = false;
    let mut http_host = None;
    let mut http_port = None;

    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "HTTPSEnable" => https_enabled = value == "1",
            "HTTPSProxy" => https_host = Some(value.to_string()),
            "HTTPSPort" => https_port = Some(value.to_string()),
            "HTTPEnable" => http_enabled = value == "1",
            "HTTPProxy" => http_host = Some(value.to_string()),
            "HTTPPort" => http_port = Some(value.to_string()),
            _ => {}
        }
    }

    let (enabled, host, port) = if https_enabled && https_host.is_some() {
        (true, https_host, https_port)
    } else {
        (http_enabled, http_host, http_port)
    };
    if !enabled {
        return None;
    }
    let host = host?;
    match port {
        Some(port) => Some(format!("http://{host}:{port}")),
        None => Some(format!("http://{host}")),
    }
}

/// 代理优先级：显式传入 > 环境变量 > 系统设置。
pub fn resolve_proxy(explicit: Option<String>) -> ProxyReport {
    let from_env = env_proxy();
    let from_system = system_proxy();
    let (proxy, source) = if let Some(value) = explicit.filter(|value| !value.is_empty()) {
        (Some(value), "explicit")
    } else if let Some(value) = from_env.clone() {
        (Some(value), "env")
    } else if let Some(value) = from_system.clone() {
        (Some(value), "system")
    } else {
        (None, "none")
    };
    ProxyReport {
        proxy,
        source: source.to_string(),
        from_env,
        from_system,
    }
}

pub struct ProbeRequest {
    pub url: String,
    pub proxy: Option<String>,
    pub insecure: bool,
    pub timeout_secs: u64,
}

pub async fn probe(request: ProbeRequest) -> ProbeResult {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(request.timeout_secs))
        .user_agent("pantry/0.1 (+tauri)");

    if let Some(proxy) = request.proxy.as_ref() {
        match reqwest::Proxy::all(proxy) {
            Ok(configured) => builder = builder.proxy(configured),
            Err(error) => {
                return ProbeResult {
                    url: request.url,
                    ok: false,
                    status: None,
                    elapsed_ms: 0,
                    proxy: request.proxy,
                    insecure: request.insecure,
                    sample: None,
                    error: Some(format!("代理地址无法解析：{error}")),
                }
            }
        }
    }
    if request.insecure {
        builder = builder.danger_accept_invalid_certs(true);
    }

    let started = Instant::now();
    let client = match builder.build() {
        Ok(client) => client,
        Err(error) => {
            return ProbeResult {
                url: request.url,
                ok: false,
                status: None,
                elapsed_ms: 0,
                proxy: request.proxy,
                insecure: request.insecure,
                sample: None,
                error: Some(format!("HTTP 客户端创建失败：{error}")),
            }
        }
    };

    match client.get(&request.url).send().await {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            ProbeResult {
                elapsed_ms: started.elapsed().as_millis() as u64,
                url: request.url,
                ok: status.is_success(),
                status: Some(status.as_u16()),
                proxy: request.proxy,
                insecure: request.insecure,
                sample: Some(body.chars().take(160).collect()),
                error: if status.is_success() {
                    None
                } else {
                    Some(format!("HTTP {status}"))
                },
            }
        }
        Err(error) => ProbeResult {
            elapsed_ms: started.elapsed().as_millis() as u64,
            url: request.url,
            ok: false,
            status: None,
            proxy: request.proxy,
            insecure: request.insecure,
            sample: None,
            error: Some(error.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scutil_output() {
        let text = "    HTTPEnable : 1\n    HTTPPort : 7890\n    HTTPProxy : 127.0.0.1\n    HTTPSEnable : 1\n    HTTPSPort : 7890\n    HTTPSProxy : 127.0.0.1\n";
        assert_eq!(parse_scutil(text).as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(parse_scutil("  HTTPEnable : 0\n"), None);
    }

    #[test]
    fn proxy_report_reports_source() {
        let report = resolve_proxy(None);
        println!(
            "代理来源 = {}，环境变量 = {:?}，系统 = {:?}",
            report.source, report.from_env, report.from_system
        );
        assert!(!report.source.is_empty());
    }

    #[test]
    fn payload_json_matches_frontend_contract() {
        let proxy = serde_json::to_string(&resolve_proxy(None)).unwrap();
        for key in ["fromEnv", "fromSystem", "source"] {
            assert!(proxy.contains(key), "缺少字段 {key}，实际：{proxy}");
        }

        let result = ProbeResult {
            url: "https://example.com".into(),
            ok: true,
            status: Some(200),
            elapsed_ms: 12,
            proxy: None,
            insecure: false,
            sample: Some("{}".into()),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        for key in ["elapsedMs", "insecure", "status", "sample"] {
            assert!(json.contains(key), "缺少字段 {key}，实际：{json}");
        }
    }

    /// 联网测试，默认跳过：`cargo test -- --ignored`
    #[test]
    #[ignore]
    fn npmmirror_is_reachable() {
        let result = tauri::async_runtime::block_on(probe(ProbeRequest {
            url: "https://registry.npmmirror.com/vue/latest".to_string(),
            proxy: None,
            insecure: false,
            timeout_secs: 20,
        }));
        println!(
            "npmmirror: ok={} status={:?} 耗时={}ms 错误={:?}",
            result.ok, result.status, result.elapsed_ms, result.error
        );
        assert!(result.ok, "npmmirror 应可访问：{:?}", result.error);
    }

    /// 对照测试：npmjs 直连在本机预期失败，用来验证探测能如实报错。
    #[test]
    #[ignore]
    fn npmjs_direct_reports_failure() {
        let result = tauri::async_runtime::block_on(probe(ProbeRequest {
            url: "https://registry.npmjs.org/vue/latest".to_string(),
            proxy: None,
            insecure: false,
            timeout_secs: 20,
        }));
        println!(
            "npmjs 直连: ok={} status={:?} 耗时={}ms 错误={:?}",
            result.ok, result.status, result.elapsed_ms, result.error
        );
    }

    /// 本机实际可用的组合：环境变量里的代理 + 跳过证书校验。
    /// 结论写进设置页默认值，不要靠猜。
    #[test]
    #[ignore]
    fn npmjs_via_env_proxy_with_insecure() {
        let resolved = resolve_proxy(None);
        println!("解析出的代理 = {:?}（来源 {}）", resolved.proxy, resolved.source);
        let result = tauri::async_runtime::block_on(probe(ProbeRequest {
            url: "https://registry.npmjs.org/vue/latest".to_string(),
            proxy: resolved.proxy,
            insecure: true,
            timeout_secs: 20,
        }));
        println!(
            "npmjs 代理+跳过证书: ok={} status={:?} 耗时={}ms 错误={:?}",
            result.ok, result.status, result.elapsed_ms, result.error
        );
    }
}
