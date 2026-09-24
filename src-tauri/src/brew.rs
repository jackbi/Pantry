//! Homebrew 侧的搜索与详情。
//!
//! 接口选择基于本机实测（都带 `HOMEBREW_NO_AUTO_UPDATE=1`）：
//! - `brew search --formula/--cask <kw>`：0.2–0.6s，纯文本，一行一个名字；没有结果时
//!   往 stderr 写 `Error: No formulae or casks found for "x".` 且退出码非 0
//! - `brew info --json=v2 --formula a b c`：5 个包 0.32s，一次能问多个，用它给搜索结果补描述
//! - `brew info --json=v2 --cask <token>`：带 artifacts，能直接拿到 `/Applications/x.app`
//! - 安装量：`formulae.brew.sh/api/formula/<name>.json` 里的 `analytics.install["30d"]`
//!   （npm 的周下载量对应物；只有 formula 有，cask 没有逐包接口）。这台机器直连要 7.5s、
//!   走代理 0.87s，所以它**不随详情一起返回**，由前端另发一次请求，慢也不挡详情。

use std::collections::HashSet;
use std::process::Command;
use std::time::Instant;

use serde::Serialize;

use crate::packages::run_with_env;
use crate::shell_env;

pub const NO_AUTO_UPDATE: &[(&str, &str)] = &[("HOMEBREW_NO_AUTO_UPDATE", "1")];
/// 搜索一次最多回传多少个候选项，避免 `brew search` 的宽泛匹配把界面灌满
const MAX_RESULTS_PER_KIND: usize = 30;
/// 一次 `brew info` 最多问多少个，命令行太长反而不稳
const MAX_INFO_BATCH: usize = 30;
const ANALYTICS_TIMEOUT_SECS: u64 = 10;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrewSearchItem {
    pub name: String,
    pub kind: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub installed: bool,
    pub outdated: bool,
    pub latest: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrewSearchResponse {
    pub query: String,
    pub items: Vec<BrewSearchItem>,
    /// 单个 kind 失败不影响另一个，失败原因如实带回
    pub errors: Vec<String>,
    pub took_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrewDetail {
    pub name: String,
    pub kind: String,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub tap: Option<String>,
    /// 已安装版本；未安装为 None
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub outdated: bool,
    pub pinned: bool,
    pub dependencies: Vec<String>,
    pub conflicts: Vec<String>,
    pub caveats: Option<String>,
    /// 已废弃 / 已停用时的原因，界面按警告呈现
    pub deprecation: Option<String>,
    /// cask 装出来的 .app 路径，用于「打开」与「在访达中显示」
    pub app_path: Option<String>,
    /// cask 自带更新器（如 Docker Desktop），这类升级通常该走应用内更新
    pub auto_updates: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrewStats {
    pub name: String,
    /// 近 30 天安装次数，取不到为 None
    pub installs_30d: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrewAvailability {
    /// `macos` / `linux` / …：Homebrew 入口只在 macOS 上出现
    pub platform: String,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

/// 侧栏是否显示 Homebrew 入口由它决定：当前系统是 macOS，且能找到 brew。
pub fn availability() -> BrewAvailability {
    let path = shell_env::resolve_program("brew").map(|found| found.display().to_string());
    let version = path
        .as_ref()
        .and_then(|_| run_brew(&["--version"], None).ok())
        .and_then(|text| text.lines().next().map(str::to_string));
    BrewAvailability {
        platform: std::env::consts::OS.to_string(),
        available: path.is_some(),
        path,
        version,
    }
}

// ------------------------------------------------------------------ 执行

/// brew 命令的额外环境变量：固定带 `HOMEBREW_NO_AUTO_UPDATE=1`，
/// 用户配置了代理再带上——访达启动的 GUI 自己读不到终端变量，
/// 搜索 / 详情 / 安装要用同一个出口，否则会出现"装得上、搜不到"。
fn brew_env(proxy: Option<&str>) -> Vec<(&'static str, String)> {
    let mut env: Vec<(&'static str, String)> = NO_AUTO_UPDATE
        .iter()
        .map(|(key, value)| (*key, (*value).to_string()))
        .collect();
    if let Some(proxy) = proxy.map(str::trim).filter(|value| !value.is_empty()) {
        env.push(("HTTPS_PROXY", proxy.to_string()));
        env.push(("HTTP_PROXY", proxy.to_string()));
    }
    env
}

fn run_brew(args: &[&str], proxy: Option<&str>) -> Result<String, String> {
    let env = brew_env(proxy);
    let refs: Vec<(&str, &str)> = env.iter().map(|(key, value)| (*key, value.as_str())).collect();
    run_with_env("brew", args, &refs)
}

/// `brew search` 把"没找到"写成 stderr 且退出码非 0，
/// 这里单独执行以便区分「没有结果」和「brew 坏了」。
fn search_names(kind: &str, query: &str, proxy: Option<&str>) -> Result<Vec<String>, String> {
    let executable =
        shell_env::resolve_program("brew").ok_or_else(|| "未找到 brew".to_string())?;
    let mut command = Command::new(executable);
    command
        .args(["search", &format!("--{kind}"), query])
        .env("PATH", shell_env::path_env());
    for (key, value) in &brew_env(proxy) {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|error| format!("执行 brew search 失败：{error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let names = parse_search_names(&stdout);
    if names.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // 没有匹配不是错误，别把 stderr 原文糊到界面上
        if stderr.contains("No formulae or casks found") {
            return Ok(Vec::new());
        }
        if !output.status.success() {
            let detail = stderr.trim();
            return Err(if detail.is_empty() {
                "brew search 失败".to_string()
            } else {
                detail.to_string()
            });
        }
    }
    Ok(names)
}

/// 纯文本结果 → 包名列表。过滤空行、`==> Casks` 这类分组标题与提示语。
pub fn parse_search_names(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("==>"))
        .filter(|line| !line.contains(' '))
        .filter(|line| !line.starts_with("Error:"))
        .filter(|line| seen.insert((*line).to_string()))
        .map(str::to_string)
        .collect()
}

// ------------------------------------------------------------------ 详情

fn as_string_list(node: Option<&serde_json::Value>) -> Vec<String> {
    node.and_then(|value| value.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn text(node: Option<&serde_json::Value>) -> Option<String> {
    node.and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn deprecation_of(item: &serde_json::Value) -> Option<String> {
    let deprecated = item.get("deprecated").and_then(|node| node.as_bool()).unwrap_or(false);
    let disabled = item.get("disabled").and_then(|node| node.as_bool()).unwrap_or(false);
    let reason = text(item.get("deprecation_reason"))
        .or_else(|| text(item.get("disable_reason")));
    if !deprecated && !disabled {
        return None;
    }
    let label = if disabled { "已停用" } else { "已废弃" };
    Some(match reason {
        Some(reason) => format!("{label}：{reason}"),
        None => label.to_string(),
    })
}

/// 从 cask 的 artifacts 里挑出 .app 包路径，brew 详情与已安装扫描共用一份。
///
/// 三种真实形态：`{"app": [...], "target": "/Applications/X.app"}`、只有 `{font: [...]}`、
/// 只有 `{pkg: [...]}`。少数 cask 不写 `target`，按约定装在 /Applications 下。
pub(crate) fn cask_app_path(item: &serde_json::Value) -> Option<String> {
    let artifacts = item.get("artifacts").and_then(|node| node.as_array())?;
    for artifact in artifacts {
        let Some(entry) = artifact.as_object() else {
            continue;
        };
        let Some(app) = entry.get("app") else {
            continue;
        };
        if let Some(target) = text(entry.get("target")) {
            if target.ends_with(".app") {
                return Some(target);
            }
        }
        let Some(name) = app
            .as_array()
            .and_then(|list| list.first())
            .and_then(|node| node.as_str())
        else {
            continue;
        };
        return Some(format!("/Applications/{name}"));
    }
    None
}

/// 解析 `brew info --json=v2 --formula <name>`。
pub fn parse_formula(json: &str) -> Result<BrewDetail, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("brew 输出不是合法 JSON：{error}"))?;
    let item = value
        .get("formulae")
        .and_then(|node| node.as_array())
        .and_then(|list| list.first())
        .ok_or_else(|| "brew 没有返回这个 formula".to_string())?;

    let name = text(item.get("name")).ok_or_else(|| "formula 缺少 name".to_string())?;
    let installed = item
        .get("installed")
        .and_then(|node| node.as_array())
        .and_then(|list| list.first())
        .and_then(|entry| entry.get("version"))
        .and_then(|node| node.as_str())
        .map(str::to_string);
    let stable = text(item.get("versions").and_then(|node| node.get("stable")));
    let outdated = item.get("outdated").and_then(|node| node.as_bool()).unwrap_or(false);
    let version_changed = stable.is_some() && stable != installed;
    let latest = if outdated && version_changed { stable.clone() } else { None };

    Ok(BrewDetail {
        name,
        kind: "formula".to_string(),
        display_name: None,
        version: stable,
        description: text(item.get("desc")),
        homepage: text(item.get("homepage")),
        license: text(item.get("license")),
        tap: text(item.get("tap")),
        installed,
        latest,
        outdated,
        pinned: item.get("pinned").and_then(|node| node.as_bool()).unwrap_or(false),
        dependencies: as_string_list(item.get("dependencies")),
        conflicts: as_string_list(item.get("conflicts_with")),
        caveats: text(item.get("caveats")),
        deprecation: deprecation_of(item),
        app_path: None,
        auto_updates: false,
    })
}

/// 解析 `brew info --json=v2 --cask <token>`。
pub fn parse_cask(json: &str) -> Result<BrewDetail, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("brew 输出不是合法 JSON：{error}"))?;
    let item = value
        .get("casks")
        .and_then(|node| node.as_array())
        .and_then(|list| list.first())
        .ok_or_else(|| "brew 没有返回这个 cask".to_string())?;

    let name = text(item.get("token")).ok_or_else(|| "cask 缺少 token".to_string())?;
    let available = text(item.get("version"));
    // 已安装的 cask 用 installed（字符串）；没装的为 null
    let installed = text(item.get("installed"));
    let outdated = item.get("outdated").and_then(|node| node.as_bool()).unwrap_or(false);
    let version_changed = available.is_some() && available != installed;
    let latest = if outdated && version_changed { available.clone() } else { None };

    Ok(BrewDetail {
        name,
        kind: "cask".to_string(),
        display_name: item
            .get("name")
            .and_then(|node| node.as_array())
            .and_then(|list| list.first())
            .and_then(|node| node.as_str())
            .map(str::to_string),
        version: available,
        description: text(item.get("desc")),
        homepage: text(item.get("homepage")),
        license: None,
        tap: text(item.get("tap")),
        installed,
        latest,
        outdated,
        pinned: item.get("pinned").and_then(|node| node.as_bool()).unwrap_or(false),
        dependencies: as_string_list(item.get("depends_on").and_then(|node| node.get("formula"))),
        conflicts: as_string_list(item.get("conflicts_with")),
        caveats: text(item.get("caveats")),
        deprecation: deprecation_of(item),
        app_path: cask_app_path(item),
        auto_updates: item.get("auto_updates").and_then(|node| node.as_bool()).unwrap_or(false),
    })
}

/// 从 `brew info --json=v2` 的多包结果里取每个包的摘要，用于搜索结果列表。
fn summarize(kind: &str, json: &str) -> Vec<BrewSearchItem> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let key = if kind == "cask" { "casks" } else { "formulae" };
    value
        .get(key)
        .and_then(|node| node.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|item| {
            let name = if kind == "cask" {
                text(item.get("token"))?
            } else {
                text(item.get("name"))?
            };
            let available = text(item.get("version")).or_else(|| {
                text(item.get("versions").and_then(|node| node.get("stable")))
            });
            let installed = if kind == "cask" {
                text(item.get("installed"))
            } else {
                item.get("installed")
                    .and_then(|node| node.as_array())
                    .and_then(|list| list.first())
                    .and_then(|entry| entry.get("version"))
                    .and_then(|node| node.as_str())
                    .map(str::to_string)
            };
            let outdated = item.get("outdated").and_then(|node| node.as_bool()).unwrap_or(false);
            let version_changed = available.is_some() && available != installed;
            let latest = if outdated && version_changed { available.clone() } else { None };
            Some(BrewSearchItem {
                name,
                kind: kind.to_string(),
                version: available,
                description: text(item.get("desc")),
                homepage: text(item.get("homepage")),
                installed: installed.is_some(),
                outdated,
                latest,
            })
        })
        .collect()
}

// ------------------------------------------------------------------ 命令实现

/// 搜索同时覆盖 formula 与 cask：从名字看不出类型，少问一次就得再点一次。
/// 类型筛选在前端做（两边的结果都已经拿到，本地过滤是即时生效的）。
pub async fn search(query: String, proxy: Option<String>) -> Result<BrewSearchResponse, String> {
    tauri::async_runtime::spawn_blocking(move || search_blocking(query, proxy))
        .await
        .map_err(|error| format!("搜索任务失败：{error}"))?
}

fn search_blocking(query: String, proxy: Option<String>) -> Result<BrewSearchResponse, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("搜索关键词不能为空".to_string());
    }
    let want = ["formula".to_string(), "cask".to_string()];

    let started = Instant::now();
    // 两种 kind 各自发一次 brew search，互不阻塞
    let keyword = query.as_str();
    let proxy = proxy.as_deref();
    let found = std::thread::scope(|scope| {
        let handles: Vec<_> = want
            .iter()
            .map(|kind| {
                let kind = kind.as_str();
                (
                    kind.to_string(),
                    scope.spawn(move || search_names(kind, keyword, proxy)),
                )
            })
            .collect();
        handles
            .into_iter()
            .map(|(kind, handle)| {
                (
                    kind,
                    handle.join().unwrap_or_else(|_| Err("搜索线程 panic".to_string())),
                )
            })
            .collect::<Vec<_>>()
    });

    // 两个 kind 各自一次 brew info 补齐描述与安装状态，同样并行
    // （brew 是 Ruby 启动，单次约 0.35s，串起来就要一秒多）
    let enriched = std::thread::scope(|scope| {
        let handles: Vec<_> = found
            .iter()
            .filter_map(|(kind, result)| match result {
                Ok(names) if !names.is_empty() => {
                    let names: Vec<String> =
                        names.iter().take(MAX_RESULTS_PER_KIND).cloned().collect();
                    let kind = kind.as_str();
                    Some((
                        kind.to_string(),
                        names.clone(),
                        scope.spawn(move || info_batch(kind, &names, proxy)),
                    ))
                }
                _ => None,
            })
            .collect();
        handles
            .into_iter()
            .map(|(kind, names, handle)| {
                (
                    kind,
                    names,
                    handle.join().unwrap_or_else(|_| Err("详情线程 panic".to_string())),
                )
            })
            .collect::<Vec<_>>()
    });

    let mut items = Vec::new();
    let mut errors = Vec::new();
    for (kind, result) in &found {
        if let Err(error) = result {
            errors.push(format!("{kind} 搜索失败：{error}"));
        }
    }
    for (kind, names, result) in enriched {
        match result {
            Ok(list) => items.extend(list),
            Err(error) => {
                // 拿不到详情就退回只有名字的清单，至少还能点进去
                errors.push(format!("{kind} 详情获取失败：{error}"));
                items.extend(names.into_iter().map(|name| BrewSearchItem {
                    name,
                    kind: kind.clone(),
                    version: None,
                    description: None,
                    homepage: None,
                    installed: false,
                    outdated: false,
                    latest: None,
                }));
            }
        }
    }

    // 超过一批上限的名字不补详情，但也不能丢：仍以 name-only 返回，点进去能看到完整详情
    for (kind, result) in &found {
        let Ok(names) = result else { continue };
        for name in names.iter().skip(MAX_RESULTS_PER_KIND) {
            items.push(BrewSearchItem {
                name: name.clone(),
                kind: kind.clone(),
                version: None,
                description: None,
                homepage: None,
                installed: false,
                outdated: false,
                latest: None,
            });
        }
    }

    items.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(BrewSearchResponse {
        query,
        items,
        errors,
        took_ms: started.elapsed().as_millis() as u64,
    })
}

fn info_batch(
    kind: &str,
    names: &[String],
    proxy: Option<&str>,
) -> Result<Vec<BrewSearchItem>, String> {
    let names: Vec<&str> = names.iter().take(MAX_INFO_BATCH).map(String::as_str).collect();
    let mut args = vec!["info", "--json=v2", if kind == "cask" { "--cask" } else { "--formula" }];
    args.extend(names.iter().copied());
    let json = run_brew(&args, proxy)?;
    Ok(summarize(kind, &json))
}

pub async fn detail(
    name: String,
    kind: String,
    proxy: Option<String>,
) -> Result<BrewDetail, String> {
    tauri::async_runtime::spawn_blocking(move || detail_blocking(name, kind, proxy))
        .await
        .map_err(|error| format!("详情任务失败：{error}"))?
}

/// 近 30 天安装次数。与详情分开调用：它是第三方统计站，慢的时候要 7.5s，
/// 不能让详情面板干等。
pub async fn stats(name: String, proxy: Option<String>, insecure: bool) -> BrewStats {
    match formula_installs_30d(&name, proxy, insecure).await {
        Ok(count) => BrewStats {
            name,
            installs_30d: Some(count),
            error: None,
        },
        Err(error) => BrewStats {
            name,
            installs_30d: None,
            error: Some(error),
        },
    }
}

fn detail_blocking(name: String, kind: String, proxy: Option<String>) -> Result<BrewDetail, String> {
    let kind = if kind == "cask" { "cask" } else { "formula" };
    let flag = if kind == "cask" { "--cask" } else { "--formula" };
    let json = run_brew(&["info", "--json=v2", flag, &name], proxy.as_deref())?;
    if kind == "cask" {
        parse_cask(&json)
    } else {
        parse_formula(&json)
    }
}

/// `formulae.brew.sh` 是 Homebrew 的公开统计站，与 brew 命令无关，
/// 因此单独建客户端（短超时），拿不到就让界面显示"—"。
///
/// 代理与证书开关跟随设置页：这台机器走代理 0.87s、直连要 7.5s，
/// 用户填了代理就按他填的走，没填才回落到自动探测。
async fn formula_installs_30d(
    name: &str,
    proxy: Option<String>,
    insecure: bool,
) -> Result<u64, String> {
    let client = crate::market::build_client(proxy, insecure, ANALYTICS_TIMEOUT_SECS)?;
    let url = format!("https://formulae.brew.sh/api/formula/{name}.json");
    let body = client
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("统计接口请求失败：{error}"))?
        .text()
        .await
        .map_err(|error| format!("统计接口读取失败：{error}"))?;
    let value: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| format!("统计接口不是合法 JSON：{error}"))?;
    value
        .get("analytics")
        .and_then(|node| node.get("install_30d"))
        .and_then(|node| node.get(name))
        .and_then(|node| node.as_u64())
        .or_else(|| {
            // 新版结构是 analytics.install["30d"][name]
            value
                .get("analytics")
                .and_then(|node| node.get("install"))
                .and_then(|node| node.get("30d"))
                .and_then(|node| node.get(name))
                .and_then(|node| node.as_u64())
        })
        .ok_or_else(|| "统计里没有这个 formula".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// brew 命令的环境变量：自动更新开关永远在，代理只有配置了才带。
    #[test]
    fn brew_env_always_disables_auto_update() {
        let plain = brew_env(None);
        assert_eq!(plain, vec![("HOMEBREW_NO_AUTO_UPDATE", "1".to_string())]);

        let with_proxy = brew_env(Some("http://127.0.0.1:7890"));
        let keys: Vec<&str> = with_proxy.iter().map(|(key, _)| *key).collect();
        assert!(keys.contains(&"HOMEBREW_NO_AUTO_UPDATE"), "{keys:?}");
        assert!(keys.contains(&"HTTPS_PROXY"), "{keys:?}");
        assert!(keys.contains(&"HTTP_PROXY"), "代理要同时写 HTTP 与 HTTPS：{keys:?}");

        // 空白串按未设置处理，别把空代理塞给 brew
        assert_eq!(brew_env(Some("   ")), plain);
    }

    #[test]
    fn filters_search_output_noise() {
        let text = "==> Formulae\nripgrep\nripgrep-all\n\nripgrep\nripgrep: some hint text\nError: nope\n";
        // 分组标题、提示语、错误行都要丢掉，重复项只留一次
        assert_eq!(parse_search_names(text), vec!["ripgrep", "ripgrep-all"]);
        assert!(parse_search_names("").is_empty());
    }

    #[test]
    fn parses_formula_detail() {
        let json = r#"{"formulae":[{
          "name": "llvm",
          "desc": "Next-gen compiler infrastructure",
          "homepage": "https://llvm.org/",
          "license": "Apache-2.0 WITH LLVM-exception",
          "tap": "homebrew/core",
          "versions": {"stable": "23.1.1"},
          "installed": [{"version": "23.1.1"}],
          "outdated": true,
          "pinned": false,
          "dependencies": ["z3", "zstd"],
          "conflicts_with": [],
          "caveats": null,
          "deprecated": false,
          "disabled": false
        }]}"#;
        let detail = parse_formula(json).expect("应能解析");
        assert_eq!(detail.kind, "formula");
        assert_eq!(detail.version.as_deref(), Some("23.1.1"));
        assert_eq!(detail.installed.as_deref(), Some("23.1.1"));
        assert_eq!(detail.dependencies, vec!["z3", "zstd"]);
        // 版本号没变只是修订号更新：不给「可升级到 X」，与已装页口径一致
        assert!(detail.outdated);
        assert!(detail.latest.is_none());
        assert!(detail.deprecation.is_none());
        assert!(detail.app_path.is_none());
    }

    #[test]
    fn parses_formula_with_new_version_and_deprecation() {
        let json = r#"{"formulae":[{
          "name": "oldthing",
          "versions": {"stable": "2.0.0"},
          "installed": [{"version": "1.0.0"}],
          "outdated": true,
          "deprecated": true,
          "deprecation_reason": "不再维护"
        }]}"#;
        let detail = parse_formula(json).expect("应能解析");
        assert_eq!(detail.latest.as_deref(), Some("2.0.0"));
        assert_eq!(detail.deprecation.as_deref(), Some("已废弃：不再维护"));
    }

    #[test]
    fn parses_cask_detail_and_app_path() {
        let json = r#"{"casks":[{
          "token": "cc-switch",
          "name": ["CC Switch"],
          "desc": "供应商切换工具",
          "homepage": "https://example.com",
          "version": "3.20.3",
          "installed": "3.20.3",
          "outdated": false,
          "auto_updates": true,
          "pinned": false,
          "deprecated": false,
          "disabled": false,
          "artifacts": [
            {"app": ["CC Switch.app"], "target": "/Applications/CC Switch.app"},
            {"zap": [{"trash": ["~/.cc-switch"]}]}
          ]
        }]}"#;
        let detail = parse_cask(json).expect("应能解析");
        assert_eq!(detail.kind, "cask");
        assert_eq!(detail.app_path.as_deref(), Some("/Applications/CC Switch.app"));
        assert_eq!(detail.display_name.as_deref(), Some("CC Switch"));
        assert_eq!(detail.installed.as_deref(), Some("3.20.3"));
        assert!(detail.auto_updates);
        assert!(detail.latest.is_none());
    }

    #[test]
    fn cask_without_app_artifact_has_no_path() {
        // 只装字体或安装器的 cask 没有可打开的 App，界面据此隐藏按钮
        let json = r#"{"casks":[{
          "token": "font-fira-code",
          "version": "6.2",
          "installed": "6.2",
          "artifacts": [{"font": ["FiraCode-Regular.ttf"], "target": "~/Library/Fonts"}]
        }]}"#;
        let detail = parse_cask(json).expect("应能解析");
        assert!(detail.app_path.is_none());
    }

    #[test]
    fn rejects_payloads_without_the_expected_key() {
        assert!(parse_formula(r#"{"casks":[]}"#).is_err());
        assert!(parse_cask(r#"{"formulae":[]}"#).is_err());
        assert!(parse_formula("<html>").is_err());
    }

    /// 字段名必须与 src/types.ts 一致
    #[test]
    fn brew_json_matches_frontend_contract() {
        let json = r#"{"formulae":[{"name":"ripgrep","versions":{"stable":"15.2.0"},"installed":[]}]}"#;
        let detail = serde_json::to_value(parse_formula(json).unwrap()).unwrap();
        for key in [
            "name",
            "kind",
            "displayName",
            "version",
            "description",
            "homepage",
            "license",
            "tap",
            "installed",
            "latest",
            "outdated",
            "pinned",
            "dependencies",
            "conflicts",
            "caveats",
            "deprecation",
            "appPath",
            "autoUpdates",
        ] {
            assert!(detail.get(key).is_some(), "BrewDetail 缺少 {key}");
        }

        let stats = serde_json::to_value(BrewStats {
            name: "ripgrep".to_string(),
            installs_30d: Some(1),
            error: None,
        })
        .unwrap();
        for key in ["name", "installs30d", "error"] {
            assert!(stats.get(key).is_some(), "BrewStats 缺少 {key}");
        }
    }

    /// 真机验证：`cargo test -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_search_and_detail() {
        let response = tauri::async_runtime::block_on(search("ripgrep".to_string(), None))
            .expect("搜索应成功");
        println!(
            "搜索 ripgrep：{} 条，{}ms，错误 {:?}",
            response.items.len(),
            response.took_ms,
            response.errors
        );
        assert!(
            response
                .items
                .iter()
                .any(|item| item.name == "ripgrep" && item.description.is_some()),
            "结果应带描述"
        );

        let formula = tauri::async_runtime::block_on(detail(
            "ripgrep".to_string(),
            "formula".to_string(),
            None,
        ))
        .expect("formula 详情应成功");
        println!(
            "ripgrep：{} {}，依赖 {} 个",
            formula.version.clone().unwrap_or_default(),
            formula.license.clone().unwrap_or_default(),
            formula.dependencies.len(),
        );
        assert!(formula.homepage.is_some());

        let stats = tauri::async_runtime::block_on(stats("ripgrep".to_string(), None, false));
        println!("ripgrep 30 天安装：{:?}（{:?}）", stats.installs_30d, stats.error);

        // 本机装过 cask：应能解析出 App 路径，没有 App 的 cask 则为 None
        let cask = tauri::async_runtime::block_on(detail(
            "cc-switch".to_string(),
            "cask".to_string(),
            None,
        ))
        .expect("cask 详情应成功");
        println!(
            "cc-switch：{} 已装 {:?}，App 路径 {:?}",
            cask.version.clone().unwrap_or_default(),
            cask.installed,
            cask.app_path
        );
        assert_eq!(cask.kind, "cask");
        assert!(cask.installed.is_some(), "本机装过 cc-switch");
    }

    #[test]
    #[ignore]
    fn real_search_without_results_is_not_an_error() {
        let response =
            tauri::async_runtime::block_on(search("zzz-no-such-package-123".to_string(), None))
                .expect("没有结果也算成功");
        assert!(response.items.is_empty());
        assert!(
            response.errors.is_empty(),
            "不该把「没找到」当错误：{:?}",
            response.errors
        );
    }
}
