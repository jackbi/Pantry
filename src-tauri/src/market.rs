//! npm 市场：搜索、详情、版本历史、README。
//!
//! 接口选择基于本机实测，不是照搬 npm 官网的做法：
//! - 搜索：`{registry}/-/v1/search`，npmmirror 直连约 2s（官方源直连不通）
//!   响应里带完整 versions 数组，版本数不用额外请求
//! - 详情：`{registry}/{name}/latest`，3.5KB / 0.14s
//! - 当前标签：`{registry}/-/package/{name}/dist-tags`，237B / 0.11s
//! - 下载量：走官方 `api.npmjs.org`（本机可直连，0.8s）。**镜像站的数字口径不同**：
//!   react 近一周官方 1.33 亿、镜像 291 万，所以默认用官方，失败才回退并标注来源
//! - 版本历史：`{registry}/{name}` 完整 packument（只需要 time/dist-tags，但无轻量接口），
//!   按需加载 + gzip；实测 react 压缩后 1.27MB、typescript 1.72MB
//! - README：仍从 jsDelivr 定向拉单个 README.md，不随详情加载

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::probe;

pub const DEFAULT_REGISTRY: &str = "https://registry.npmmirror.com";
/// 下载量统计的官方接口，与 registry 分开：镜像站不提供同口径数据
const DOWNLOADS_API: &str = "https://api.npmjs.org";
const README_CDN: &str = "https://cdn.jsdelivr.net/npm";
const CACHE_TTL: Duration = Duration::from_secs(600);
const MAX_README_BYTES: usize = 200 * 1024;
/// 版本历史一次最多回传多少条，避免上千个版本拖垮前端
const MAX_VERSION_ROWS: usize = 300;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageLinks {
    pub npm: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub issues: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub published_at: Option<String>,
    pub publisher: Option<String>,
    pub keywords: Vec<String>,
    pub links: PackageLinks,
    pub score: Option<f64>,
    /// 已发布版本总数，取自搜索响应里的 versions 数组
    pub version_count: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub query: String,
    pub total: u64,
    pub hits: Vec<SearchHit>,
    pub took_ms: u64,
    pub cached: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependency {
    pub name: String,
    pub range: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageDetail {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub unpacked_size: Option<u64>,
    pub file_count: Option<u64>,
    pub dependencies: Vec<Dependency>,
    pub peer_dependencies: Vec<Dependency>,
    pub keywords: Vec<String>,
    pub deprecated: Option<String>,
    pub weekly_downloads: Option<u64>,
    /// 近 7 天每日下载量，用于趋势图；取不到时为空
    pub weekly_series: Vec<u64>,
    /// 下载量来源：`npm` 为官方口径，`npmmirror` 为镜像口径
    pub downloads_source: Option<String>,
    pub downloads_error: Option<String>,
    /// 当前 dist-tags（latest / next / alpha …）
    pub dist_tags: Vec<DistTag>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DistTag {
    pub tag: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageVersion {
    pub version: String,
    /// 命中该版本的 dist-tag，如 latest / next
    pub tags: Vec<String>,
    pub published_at: Option<String>,
    pub downloads_last_week: Option<u64>,
    pub deprecated: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionHistory {
    pub name: String,
    pub total: usize,
    pub last_published_at: Option<String>,
    pub versions: Vec<PackageVersion>,
    pub truncated: bool,
    pub downloads_source: Option<String>,
    pub downloads_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadmeResponse {
    pub name: String,
    pub version: String,
    pub markdown: String,
    pub source_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestVersion {
    pub name: String,
    pub latest: Option<String>,
    pub error: Option<String>,
}

// ------------------------------------------------------------------- 缓存

struct CacheEntry {
    stored_at: Instant,
    body: String,
}

fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_get(key: &str) -> Option<String> {
    let guard = cache().lock().ok()?;
    let entry = guard.get(key)?;
    if entry.stored_at.elapsed() > CACHE_TTL {
        return None;
    }
    Some(entry.body.clone())
}

fn cache_put(key: String, body: String) {
    if let Ok(mut guard) = cache().lock() {
        guard.insert(
            key,
            CacheEntry {
                stored_at: Instant::now(),
                body,
            },
        );
    }
}

// ------------------------------------------------------------------ HTTP

pub(crate) fn build_client(proxy: Option<String>, insecure: bool, timeout_secs: u64) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("pantry/0.1 (+tauri)");

    let resolved = probe::resolve_proxy(proxy);
    if let Some(address) = resolved.proxy {
        let configured = reqwest::Proxy::all(&address)
            .map_err(|error| format!("代理地址无法解析（{address}）：{error}"))?;
        builder = builder.proxy(configured);
    }
    if insecure {
        builder = builder.danger_accept_invalid_certs(true);
    }
    builder.build().map_err(|error| format!("HTTP 客户端创建失败：{error}"))
}

/// 直连优先的客户端：**只有用户显式配置代理时才走代理**。
///
/// `api.npmjs.org` 与 registry 不是同一台机器，本机实测直连稳定 0.8s，
/// 而自动探测到的本地代理会让它挂起（同样的 URL 走代理 17.5s 后失败）。
/// 因此这里忽略 env / 系统代理，拿不到就走镜像兜底。
pub(crate) fn build_direct_client(explicit_proxy: Option<String>, timeout_secs: u64) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("pantry/0.1 (+tauri)");

    match explicit_proxy.filter(|value| !value.trim().is_empty()) {
        Some(address) => {
            let configured = reqwest::Proxy::all(&address)
                .map_err(|error| format!("代理地址无法解析（{address}）：{error}"))?;
            builder = builder.proxy(configured);
        }
        None => builder = builder.no_proxy(),
    }

    builder.build().map_err(|error| format!("HTTP 客户端创建失败：{error}"))
}

async fn get_text(
    client: &reqwest::Client,
    url: &str,
    cache_key: Option<&str>,
) -> Result<(String, bool), String> {
    if let Some(key) = cache_key {
        if let Some(body) = cache_get(key) {
            return Ok((body, true));
        }
    }

    let response = client
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("请求失败：{error}"))?;

    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    if let Some(key) = cache_key {
        cache_put(key.to_string(), body.clone());
    }
    Ok((body, false))
}

// ------------------------------------------------------------------ 解析

pub fn parse_search(query: &str, json: &str, took_ms: u64, cached: bool) -> Result<SearchResponse, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("搜索响应不是合法 JSON：{error}"))?;

    let hits = value
        .get("objects")
        .and_then(|node| node.as_array())
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let package = entry.get("package")?;
            let name = package.get("name")?.as_str()?.to_string();
            let links = package.get("links").cloned().unwrap_or(serde_json::Value::Null);
            let string = |key: &str| {
                links
                    .get(key)
                    .and_then(|node| node.as_str())
                    .map(str::to_string)
            };
            Some(SearchHit {
                name,
                version: package
                    .get("version")
                    .and_then(|node| node.as_str())
                    .unwrap_or_default()
                    .to_string(),
                description: package
                    .get("description")
                    .and_then(|node| node.as_str())
                    .map(str::to_string),
                published_at: package
                    .get("date")
                    .and_then(|node| node.as_str())
                    .map(str::to_string),
                publisher: package
                    .get("publisher")
                    .and_then(|node| node.get("username"))
                    .and_then(|node| node.as_str())
                    .map(str::to_string),
                keywords: package
                    .get("keywords")
                    .and_then(|node| node.as_array())
                    .map(|list| {
                        list.iter()
                            .filter_map(|item| item.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
                links: PackageLinks {
                    npm: string("npm"),
                    homepage: string("homepage"),
                    repository: string("repository"),
                    issues: string("bugs"),
                },
                score: entry
                    .get("score")
                    .and_then(|node| node.get("final"))
                    .and_then(|node| node.as_f64()),
                version_count: package
                    .get("versions")
                    .and_then(|node| node.as_array())
                    .map(|list| list.len() as u64),
            })
        })
        .collect();

    Ok(SearchResponse {
        query: query.to_string(),
        total: value.get("total").and_then(|node| node.as_u64()).unwrap_or(0),
        hits,
        took_ms,
        cached,
    })
}

fn dependency_list(node: Option<&serde_json::Value>) -> Vec<Dependency> {
    let mut items: Vec<Dependency> = node
        .and_then(|value| value.as_object())
        .map(|map| {
            map.iter()
                .map(|(name, range)| Dependency {
                    name: name.clone(),
                    range: range.as_str().unwrap_or_default().to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    items.sort_by(|left, right| left.name.cmp(&right.name));
    items
}

pub fn parse_detail(json: &str) -> Result<PackageDetail, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("详情响应不是合法 JSON：{error}"))?;

    let name = value
        .get("name")
        .and_then(|node| node.as_str())
        .ok_or_else(|| "详情响应缺少 name".to_string())?
        .to_string();

    let repository = value
        .get("repository")
        .and_then(|node| {
            node.as_str().map(str::to_string).or_else(|| {
                node.get("url")
                    .and_then(|inner| inner.as_str())
                    .map(str::to_string)
            })
        })
        .map(|url| {
            url.trim_start_matches("git+")
                .trim_end_matches(".git")
                .to_string()
        });

    let license = value.get("license").and_then(|node| {
        node.as_str().map(str::to_string).or_else(|| {
            node.get("type")
                .and_then(|inner| inner.as_str())
                .map(str::to_string)
        })
    });

    let dist = value.get("dist");
    Ok(PackageDetail {
        name,
        version: value
            .get("version")
            .and_then(|node| node.as_str())
            .unwrap_or_default()
            .to_string(),
        description: value
            .get("description")
            .and_then(|node| node.as_str())
            .map(str::to_string),
        license,
        homepage: value
            .get("homepage")
            .and_then(|node| node.as_str())
            .map(str::to_string),
        repository,
        unpacked_size: dist
            .and_then(|node| node.get("unpackedSize"))
            .and_then(|node| node.as_u64()),
        file_count: dist
            .and_then(|node| node.get("fileCount"))
            .and_then(|node| node.as_u64()),
        dependencies: dependency_list(value.get("dependencies")),
        peer_dependencies: dependency_list(value.get("peerDependencies")),
        keywords: value
            .get("keywords")
            .and_then(|node| node.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        deprecated: value
            .get("deprecated")
            .and_then(|node| node.as_str())
            .map(str::to_string),
        weekly_downloads: None,
        weekly_series: Vec::new(),
        downloads_source: None,
        downloads_error: None,
        dist_tags: Vec::new(),
    })
}

pub fn parse_downloads(json: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .get("downloads")?
        .as_u64()
}

/// 近 7 天每日下载量。官方与镜像的 range 接口都返回 `downloads: [{day, downloads}]`。
pub fn parse_download_series(json: &str) -> Vec<u64> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|value| value.get("downloads").cloned())
        .and_then(|node| node.as_array().cloned())
        .map(|days| {
            days.iter()
                .filter_map(|day| day.get("downloads").and_then(|node| node.as_u64()))
                .collect()
        })
        .unwrap_or_default()
}

/// dist-tags：`{"latest":"1.2.3"}`，排序后返回，latest 排最前。
pub fn parse_dist_tags(json: &str) -> Vec<DistTag> {
    serde_json::from_str::<serde_json::Value>(json)
        .map(|value| dist_tags_from(&value))
        .unwrap_or_default()
}

fn dist_tags_from(value: &serde_json::Value) -> Vec<DistTag> {
    let mut tags: Vec<DistTag> = value
        .as_object()
        .map(|map| {
            map.iter()
                .filter_map(|(tag, version)| {
                    version.as_str().map(|version| DistTag {
                        tag: tag.clone(),
                        version: version.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    sort_tags(&mut tags);
    tags
}

fn sort_tags(tags: &mut [DistTag]) {
    tags.sort_by(|left, right| {
        let rank = |tag: &str| match tag {
            "latest" => 0,
            "next" => 1,
            "beta" => 2,
            "alpha" => 3,
            "canary" => 4,
            _ => 5,
        };
        rank(&left.tag)
            .cmp(&rank(&right.tag))
            .then_with(|| left.tag.cmp(&right.tag))
    });
}

/// 各版本近一周下载量。官方 `versions/{name}/last-week` 返回 `{downloads:{版本:次数}}`，
/// 镜像的 `downloads/range` 返回 `{versions:{版本:[{day,downloads}]}}`，两种都解析。
pub fn parse_version_downloads(json: &str) -> HashMap<String, u64> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return HashMap::new();
    };

    if let Some(map) = value.get("downloads").and_then(|node| node.as_object()) {
        return map
            .iter()
            .filter_map(|(version, count)| count.as_u64().map(|count| (version.clone(), count)))
            .collect();
    }

    value
        .get("versions")
        .and_then(|node| node.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(version, days)| {
                    let days = days.as_array()?;
                    let total: u64 = days
                        .iter()
                        .filter_map(|day| day.get("downloads").and_then(|node| node.as_u64()))
                        .sum();
                    Some((version.clone(), total))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 从完整 packument 抽取版本历史：只保留版本号、发布时间、标签、近一周下载量。
pub fn parse_versions(
    name: &str,
    packument: &str,
    downloads: &HashMap<String, u64>,
    downloads_source: Option<String>,
    downloads_error: Option<String>,
) -> Result<VersionHistory, String> {
    let value: serde_json::Value = serde_json::from_str(packument)
        .map_err(|error| format!("版本列表响应不是合法 JSON：{error}"))?;

    let published = value
        .get("versions")
        .and_then(|node| node.as_object())
        .ok_or_else(|| "版本列表响应缺少 versions".to_string())?;
    let times = value.get("time").and_then(|node| node.as_object());
    let tag_list = dist_tags_from(value.get("dist-tags").unwrap_or(&serde_json::Value::Null));

    let mut tags_by_version: HashMap<&str, Vec<String>> = HashMap::new();
    for item in &tag_list {
        tags_by_version
            .entry(item.version.as_str())
            .or_default()
            .push(item.tag.clone());
    }

    let mut versions: Vec<PackageVersion> = published
        .iter()
        .map(|(version, meta)| PackageVersion {
            version: version.clone(),
            tags: tags_by_version.get(version.as_str()).cloned().unwrap_or_default(),
            published_at: times
                .and_then(|map| map.get(version))
                .and_then(|node| node.as_str())
                .map(str::to_string),
            downloads_last_week: downloads.get(version).copied(),
            deprecated: meta
                .get("deprecated")
                .and_then(|node| node.as_str())
                .map(str::to_string),
        })
        .collect();

    // 与 npm 官网一致：按发布时间倒序。没有时间的版本（极少）排在最后。
    versions.sort_by(|left, right| match (&left.published_at, &right.published_at) {
        (Some(a), Some(b)) => b.cmp(a),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => right.version.cmp(&left.version),
    });

    let total = versions.len();
    let truncated = total > MAX_VERSION_ROWS;
    versions.truncate(MAX_VERSION_ROWS);

    let last_published_at = times
        .and_then(|map| map.get("modified"))
        .and_then(|node| node.as_str())
        .map(str::to_string)
        .or_else(|| versions.first().and_then(|item| item.published_at.clone()));

    Ok(VersionHistory {
        name: name.to_string(),
        total,
        last_published_at,
        versions,
        truncated,
        downloads_source,
        downloads_error,
    })
}

// ------------------------------------------------------------------ 命令实现

pub struct RequestOptions {
    pub registry: Option<String>,
    pub proxy: Option<String>,
    pub insecure: bool,
}

/// 内置默认值，给设置页做占位与"恢复默认"用。
///
/// 刻意由后端提供而不是前端写死：默认源改一次只改这里，两边不会漂移。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDefaults {
    pub registry: String,
    pub downloads_api: String,
    pub readme_cdn: String,
}

pub fn defaults() -> MarketDefaults {
    MarketDefaults {
        registry: DEFAULT_REGISTRY.to_string(),
        downloads_api: DOWNLOADS_API.to_string(),
        readme_cdn: README_CDN.to_string(),
    }
}

impl RequestOptions {
    fn registry(&self) -> String {
        self.registry
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_REGISTRY.to_string())
            .trim_end_matches('/')
            .to_string()
    }
}

/// 下载量探测结果。官方接口优先，失败才回退镜像，并如实标注来源——
/// 两种口径差异很大（react 近一周官方 1.33 亿、镜像 291 万），不标注会误读。
struct DownloadsProbe {
    total: Option<u64>,
    series: Vec<u64>,
    per_version: HashMap<String, u64>,
    source: Option<String>,
    error: Option<String>,
}

/// 下载量是补充信息，单独给它更短的超时：官方接口偶发抽风（实测 typescript
/// 的 point 要 10s、range 直接超时），不能让它把版本列表一起拖住。
const DOWNLOADS_TIMEOUT_SECS: u64 = 8;

/// 并发取若干 URL，各自失败只丢那一路数据。
async fn fetch_all(client: &reqwest::Client, urls: &[String]) -> Vec<Option<String>> {
    let handles: Vec<_> = urls
        .iter()
        .map(|url| {
            let client = client.clone();
            let url = url.clone();
            tauri::async_runtime::spawn(async move {
                let key = format!("downloads:{url}");
                get_text(&client, &url, Some(&key)).await.ok().map(|(body, _)| body)
            })
        })
        .collect();

    let mut bodies = Vec::with_capacity(handles.len());
    for handle in handles {
        bodies.push(handle.await.unwrap_or(None));
    }
    bodies
}

async fn fetch_downloads(
    proxy: Option<String>,
    registry: &str,
    name: &str,
    with_versions: bool,
) -> DownloadsProbe {
    let empty = |error: Option<String>| DownloadsProbe {
        total: None,
        series: Vec::new(),
        per_version: HashMap::new(),
        source: None,
        error,
    };
    let client = match build_direct_client(proxy, DOWNLOADS_TIMEOUT_SECS) {
        Ok(client) => client,
        Err(error) => return empty(Some(error)),
    };
    let encoded = urlencode(name);

    for (base, source) in [
        (DOWNLOADS_API.to_string(), "npm"),
        // 镜像只统计镜像站自己的流量，作为兜底并标注来源
        (registry.to_string(), "npmmirror"),
    ] {
        let mut urls = vec![
            format!("{base}/downloads/point/last-week/{encoded}"),
            format!("{base}/downloads/range/last-week/{encoded}"),
        ];
        // 官方把各版本下载量放在 /versions 下，range 响应里没有
        if with_versions && source == "npm" {
            // 这个接口只认单段路径：作用域包名里的 `/` 必须转义，否则直接 404
            // （/downloads/* 在两个域名下都接受原文 `/`，所以那里沿用 urlencode）
            urls.push(format!(
                "{DOWNLOADS_API}/versions/{}/last-week",
                urlencode_segment(name)
            ));
        }

        let bodies = fetch_all(&client, &urls).await;
        let point = bodies.first().cloned().flatten();
        let range = bodies.get(1).cloned().flatten();
        if point.is_none() && range.is_none() {
            continue;
        }

        let total = point.as_deref().and_then(parse_downloads);
        let series = range.as_deref().map(parse_download_series).unwrap_or_default();
        let per_version = if !with_versions {
            HashMap::new()
        } else if source == "npm" {
            bodies.get(2).cloned().flatten().map(|body| parse_version_downloads(&body)).unwrap_or_default()
        } else {
            // 镜像的 range 响应自带 versions 字段，逐日数据求和即可
            range.as_deref().map(parse_version_downloads).unwrap_or_default()
        };
        // 各版本下载量整块缺失时必须说清：接口不列出「零下载版本」，
        // 空表若当成 0 展示会让人误以为这些版本真的没人下
        let error = (with_versions && per_version.is_empty())
            .then(|| "各版本近一周下载量取不到".to_string());

        return DownloadsProbe {
            total,
            series,
            per_version,
            source: Some(source.to_string()),
            error,
        };
    }

    empty(Some("下载量接口不可用".to_string()))
}

pub async fn search(
    query: String,
    size: u32,
    from: u32,
    options: RequestOptions,
) -> Result<SearchResponse, String> {
    let keyword = query.trim().to_string();
    if keyword.is_empty() {
        return Err("请输入搜索关键词".to_string());
    }
    let registry = options.registry();
    let size = size.clamp(1, 100);
    let url = format!(
        "{registry}/-/v1/search?text={}&size={size}&from={from}",
        urlencode(&keyword)
    );
    let cache_key = format!("search:{url}");

    let client = build_client(options.proxy, options.insecure, 20)?;
    let started = Instant::now();
    let (body, cached) = get_text(&client, &url, Some(&cache_key)).await?;
    let took_ms = started.elapsed().as_millis() as u64;
    parse_search(&keyword, &body, took_ms, cached)
}

pub async fn detail(name: String, options: RequestOptions) -> Result<PackageDetail, String> {
    let registry = options.registry();
    let url = format!("{registry}/{}/latest", urlencode(&name));
    let cache_key = format!("detail:{url}");
    let client = build_client(options.proxy.clone(), options.insecure, 20)?;

    let (body, _) = get_text(&client, &url, Some(&cache_key)).await?;
    let mut detail = parse_detail(&body)?;

    // 当前标签很轻（237B），随详情一起取，省得再点一次
    let tags_url = format!("{registry}/-/package/{}/dist-tags", urlencode(&name));
    if let Ok((body, _)) = get_text(&client, &tags_url, Some(&format!("tags:{tags_url}"))).await {
        detail.dist_tags = parse_dist_tags(&body);
    }

    // 下载量单独取；失败不影响详情主体，如实记录原因与来源
    let downloads = fetch_downloads(options.proxy.clone(), &registry, &name, false).await;
    detail.weekly_downloads = downloads.total;
    detail.weekly_series = downloads.series;
    detail.downloads_source = downloads.source;
    detail.downloads_error = downloads.error;
    Ok(detail)
}

/// 版本历史：packument 里才有的发布时间 + 各版本近一周下载量。
///
/// 按需调用——完整 packument 即使 gzip 后，react 也有 1.27MB、typescript 1.72MB。
pub async fn versions(name: String, options: RequestOptions) -> Result<VersionHistory, String> {
    let registry = options.registry();
    let client = build_client(options.proxy.clone(), options.insecure, 30)?;
    let url = format!("{registry}/{}", urlencode(&name));
    let (body, _) = get_text(&client, &url, Some(&format!("packument:{url}"))).await?;

    let downloads = fetch_downloads(options.proxy.clone(), &registry, &name, true).await;
    parse_versions(
        &name,
        &body,
        &downloads.per_version,
        downloads.source,
        downloads.error,
    )
}

pub async fn readme(
    name: String,
    version: String,
    options: RequestOptions,
) -> Result<ReadmeResponse, String> {
    let package = if version.is_empty() {
        name.clone()
    } else {
        format!("{name}@{version}")
    };
    let url = format!("{README_CDN}/{package}/README.md");
    let client = build_client(options.proxy, options.insecure, 20)?;
    let (body, _) = get_text(&client, &url, None).await?;

    let truncated = if body.len() > MAX_README_BYTES {
        let mut end = MAX_README_BYTES;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…\n\n（README 过长，已截断）", &body[..end])
    } else {
        body
    };

    Ok(ReadmeResponse {
        name,
        version,
        markdown: truncated,
        source_url: url,
    })
}

/// 批量取「registry 上的最新版本」，用于给 npm 系已安装包标记可升级。
///
/// 逐个请求 `/latest`（约 3.5KB），按 chunk 限制并发，避免几十个包同时打过去。
pub async fn latest_versions(
    names: Vec<String>,
    options: RequestOptions,
    chunk_size: usize,
) -> Vec<LatestVersion> {
    let registry = options.registry();
    let proxy = options.proxy.clone();
    let insecure = options.insecure;
    let chunk_size = chunk_size.max(1);
    let mut results = Vec::with_capacity(names.len());

    for chunk in names.chunks(chunk_size) {
        let mut handles = Vec::with_capacity(chunk.len());
        for name in chunk {
            let registry = registry.clone();
            let proxy = proxy.clone();
            let name = name.clone();
            handles.push(tauri::async_runtime::spawn(async move {
                let client = match build_client(proxy, insecure, 15) {
                    Ok(client) => client,
                    Err(error) => {
                        return LatestVersion {
                            name,
                            latest: None,
                            error: Some(error),
                        }
                    }
                };
                let url = format!("{registry}/{}/latest", urlencode(&name));
                let cache_key = format!("latest:{url}");
                match get_text(&client, &url, Some(&cache_key)).await {
                    Ok((body, _)) => match parse_detail(&body) {
                        Ok(detail) => LatestVersion {
                            name,
                            latest: Some(detail.version),
                            error: None,
                        },
                        Err(error) => LatestVersion {
                            name,
                            latest: None,
                            error: Some(error),
                        },
                    },
                    Err(error) => LatestVersion {
                        name,
                        latest: None,
                        error: Some(error),
                    },
                }
            }));
        }
        for handle in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(error) => results.push(LatestVersion {
                    name: String::new(),
                    latest: None,
                    error: Some(format!("任务失败：{error}")),
                }),
            }
        }
    }
    results
}

fn urlencode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            // 保留 @ 与 /：registry 与 jsDelivr 都按原样接受作用域包名
            b'@' | b'/' => encoded.push(byte as char),
            other => encoded.push_str(&format!("%{other:02X}")),
        }
    }
    encoded
}

/// 把包名当作**单个路径段**编码（`/` 转义为 `%2F`），
/// 用于 `/versions/{pkg}/last-week` 这类只接受一段路径的接口。
fn urlencode_segment(value: &str) -> String {
    urlencode(value).replace('/', "%2F")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH_FIXTURE: &str = r#"{
      "objects": [{
        "package": {
          "name": "vite",
          "version": "8.0.16",
          "description": "Native-ESM powered web dev build tool",
          "date": "2026-09-01T00:00:00.000Z",
          "publisher": { "username": "yyx990803" },
          "keywords": ["build", "vite"],
          "links": { "npm": "https://www.npmjs.com/package/vite", "homepage": "https://vite.dev" }
        },
        "score": { "final": 0.9 }
      }],
      "total": 1
    }"#;

    const DETAIL_FIXTURE: &str = r#"{
      "name": "vue",
      "version": "3.5.43",
      "description": "The progressive JavaScript framework",
      "license": "MIT",
      "homepage": "https://vuejs.org",
      "repository": { "type": "git", "url": "git+https://github.com/vuejs/core.git" },
      "dependencies": { "@vue/shared": "3.5.43", "@vue/runtime-dom": "3.5.43" },
      "peerDependencies": { "typescript": "*" },
      "keywords": ["framework"],
      "dist": { "unpackedSize": 1234567, "fileCount": 37 }
    }"#;

    #[test]
    fn parses_search_response() {
        let result = parse_search("vite", SEARCH_FIXTURE, 120, false).expect("应能解析");
        assert_eq!(result.total, 1);
        assert_eq!(result.hits.len(), 1);
        let hit = &result.hits[0];
        assert_eq!(hit.name, "vite");
        assert_eq!(hit.version, "8.0.16");
        assert_eq!(hit.publisher.as_deref(), Some("yyx990803"));
        assert_eq!(hit.links.homepage.as_deref(), Some("https://vite.dev"));
        assert_eq!(hit.keywords.len(), 2);
    }

    #[test]
    fn parses_detail_and_cleans_repository_url() {
        let detail = parse_detail(DETAIL_FIXTURE).expect("应能解析");
        assert_eq!(detail.name, "vue");
        assert_eq!(detail.version, "3.5.43");
        assert_eq!(detail.license.as_deref(), Some("MIT"));
        // git+ 前缀与 .git 后缀要去掉
        assert_eq!(
            detail.repository.as_deref(),
            Some("https://github.com/vuejs/core")
        );
        assert_eq!(detail.unpacked_size, Some(1234567));
        assert_eq!(detail.dependencies.len(), 2);
        // 依赖按名称排序
        assert_eq!(detail.dependencies[0].name, "@vue/runtime-dom");
        assert_eq!(detail.peer_dependencies.len(), 1);
    }

    #[test]
    fn parses_downloads_and_handles_garbage() {
        assert_eq!(parse_downloads(r#"{"downloads":1145992}"#), Some(1145992));
        assert_eq!(parse_downloads("not json"), None);
        assert_eq!(parse_downloads(r#"{"other":1}"#), None);
    }

    #[test]
    fn parses_daily_download_series() {
        let json = r#"{"start":"2026-09-17","end":"2026-09-23","downloads":[
            {"day":"2026-09-17","downloads":432189},{"day":"2026-09-18","downloads":399943}
        ]}"#;
        assert_eq!(parse_download_series(json), vec![432189, 399943]);
        // point 接口没有逐日数组，不能当趋势用
        assert!(parse_download_series(r#"{"downloads":123}"#).is_empty());
    }

    #[test]
    fn orders_dist_tags_with_latest_first() {
        let tags = parse_dist_tags(r#"{"alpha":"0.1.7-alpha.2","next":"0.1.7-rc.1","latest":"0.1.5-rc.3"}"#);
        let order: Vec<&str> = tags.iter().map(|item| item.tag.as_str()).collect();
        assert_eq!(order, vec!["latest", "next", "alpha"]);
        assert_eq!(tags[0].version, "0.1.5-rc.3");
        assert!(parse_dist_tags("<html>").is_empty());
    }

    #[test]
    fn parses_both_per_version_download_shapes() {
        // 官方：直接给每个版本的次数
        let official = parse_version_downloads(r#"{"downloads":{"1.0.0":10,"1.0.1":7}}"#);
        assert_eq!(official.get("1.0.0"), Some(&10));
        assert_eq!(official.get("1.0.1"), Some(&7));

        // 镜像：给逐日明细，需要自己求和
        let mirror = parse_version_downloads(
            r#"{"versions":{"1.0.0":[{"day":"2026-09-17","downloads":3},{"day":"2026-09-18","downloads":4}]}}"#,
        );
        assert_eq!(mirror.get("1.0.0"), Some(&7));
    }

    #[test]
    fn builds_version_history_sorted_by_publish_time() {
        let packument = r#"{
          "name": "@deepseek-ai/dsh",
          "dist-tags": {"latest":"0.1.5-rc.3","next":"0.1.7-rc.1","alpha":"0.1.7-alpha.2"},
          "time": {
            "created": "2026-08-01T00:00:00.000Z",
            "modified": "2026-09-24T06:00:00.000Z",
            "0.1.5-rc.3": "2026-09-22T00:00:00.000Z",
            "0.1.7-alpha.2": "2026-09-23T00:00:00.000Z",
            "0.1.7-rc.1": "2026-09-24T05:00:00.000Z"
          },
          "versions": {
            "0.1.5-rc.3": {},
            "0.1.7-alpha.2": {},
            "0.1.7-rc.1": {},
            "0.1.4": {"deprecated": "改用 0.1.5-rc.3"}
          }
        }"#;
        let mut downloads = HashMap::new();
        downloads.insert("0.1.5-rc.3".to_string(), 23210);

        let history = parse_versions(
            "@deepseek-ai/dsh",
            packument,
            &downloads,
            Some("npm".to_string()),
            None,
        )
        .expect("应能解析");

        assert_eq!(history.total, 4);
        assert!(!history.truncated);
        assert_eq!(history.last_published_at.as_deref(), Some("2026-09-24T06:00:00.000Z"));
        // 最新发布的排最前；没有 time 记录的版本排最后
        assert_eq!(history.versions[0].version, "0.1.7-rc.1");
        assert_eq!(history.versions[1].version, "0.1.7-alpha.2");
        assert_eq!(history.versions[3].version, "0.1.4");
        assert_eq!(history.versions[3].deprecated.as_deref(), Some("改用 0.1.5-rc.3"));
        assert_eq!(history.versions[0].tags, vec!["next"]);
        assert_eq!(history.versions[0].downloads_last_week, None);
        assert_eq!(history.versions[2].tags, vec!["latest"]);
        assert_eq!(history.versions[2].downloads_last_week, Some(23210));
    }

    #[test]
    fn rejects_packument_without_versions() {
        assert!(parse_versions("x", "{}", &HashMap::new(), None, None).is_err());
        assert!(parse_versions("x", "<html>", &HashMap::new(), None, None).is_err());
    }

    /// 字段名必须与 src/types.ts 一致，改这里等于改前端契约
    #[test]
    fn new_fields_json_match_frontend_contract() {
        let packument = r#"{
          "dist-tags": {"latest":"1.0.0"},
          "time": {"modified":"2026-09-24T06:00:00.000Z","1.0.0":"2026-09-20T00:00:00.000Z"},
          "versions": {"1.0.0": {}}
        }"#;
        let history =
            parse_versions("demo", packument, &HashMap::new(), Some("npm".into()), None).unwrap();
        let json = serde_json::to_value(&history).unwrap();
        for key in [
            "name",
            "total",
            "lastPublishedAt",
            "versions",
            "truncated",
            "downloadsSource",
            "downloadsError",
        ] {
            assert!(json.get(key).is_some(), "VersionHistory 缺少 {key}");
        }
        for key in ["version", "tags", "publishedAt", "downloadsLastWeek", "deprecated"] {
            assert!(json["versions"][0].get(key).is_some(), "PackageVersion 缺少 {key}");
        }

        let tags = serde_json::to_value(parse_dist_tags(r#"{"latest":"1.0.0"}"#)).unwrap();
        assert!(tags[0].get("tag").is_some() && tags[0].get("version").is_some());

        let detail = serde_json::to_value(parse_detail(DETAIL_FIXTURE).unwrap()).unwrap();
        for key in ["distTags", "weeklySeries", "downloadsSource"] {
            assert!(detail.get(key).is_some(), "PackageDetail 缺少 {key}");
        }

        let hit = &serde_json::to_value(parse_search("vite", SEARCH_FIXTURE, 1, false).unwrap()).unwrap()
            ["hits"][0];
        assert!(hit.get("versionCount").is_some(), "SearchHit 缺少 versionCount");
    }

    #[test]
    fn encodes_names_but_keeps_scope_separator() {
        assert_eq!(urlencode("@vue/cli"), "@vue/cli");
        assert_eq!(urlencode("a b"), "a%20b");
        assert_eq!(urlencode("c++"), "c%2B%2B");
    }

    /// 作用域包名放在单段路径里必须转义：`/versions/@scope/name/last-week` 实测 404
    #[test]
    fn encodes_slash_when_name_must_be_one_path_segment() {
        assert_eq!(urlencode_segment("@vue/cli"), "@vue%2Fcli");
        assert_eq!(urlencode_segment("vue"), "vue");
    }

    #[test]
    fn rejects_broken_payloads() {
        assert!(parse_search("x", "<html>", 0, false).is_err());
        assert!(parse_detail("{}").is_err(), "缺少 name 应报错");
    }

    /// 真机联网验证：`cargo test -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_search_and_detail() {
        let options = || RequestOptions {
            registry: None,
            proxy: None,
            insecure: false,
        };

        let search = tauri::async_runtime::block_on(search("vite".to_string(), 5, 0, options()))
            .expect("搜索应成功");
        println!(
            "搜索 vite：命中 {} 个（总数 {}，耗时 {}ms，缓存 {}）",
            search.hits.len(),
            search.total,
            search.took_ms,
            search.cached
        );
        assert!(!search.hits.is_empty());

        let detail = tauri::async_runtime::block_on(detail("vue".to_string(), options()))
            .expect("详情应成功");
        println!(
            "详情 vue@{}：license={:?} 依赖 {} 个，周下载 {:?}（来源 {:?}，{} 天趋势），解压 {:?} 字节",
            detail.version,
            detail.license,
            detail.dependencies.len(),
            detail.weekly_downloads,
            detail.downloads_source,
            detail.weekly_series.len(),
            detail.unpacked_size
        );
        assert!(!detail.dependencies.is_empty());
        // 官方口径：vue 近一周下载量在千万级，镜像口径只有百万级，用数量级把口径钉住
        assert!(
            detail.weekly_downloads.unwrap_or(0) > 5_000_000,
            "周下载量应取官方口径（当前 {:?}，来源 {:?}）",
            detail.weekly_downloads,
            detail.downloads_source
        );
        assert_eq!(detail.weekly_series.len(), 7);
        assert!(
            detail.dist_tags.iter().any(|tag| tag.tag == "latest"),
            "应取到 latest 标签"
        );

        let readme =
            tauri::async_runtime::block_on(readme("vue".to_string(), detail.version.clone(), options()))
                .expect("README 应成功");
        println!("README 长度 {} 字节，来源 {}", readme.markdown.len(), readme.source_url);
        assert!(!readme.markdown.trim().is_empty());
    }

    /// 真机验证版本历史：完整 packument + 各版本下载量。
    #[test]
    #[ignore]
    fn real_version_history() {
        // typescript 是体积最坏情况（3833 个版本），用来确认截断与耗时
        for name in ["@deepseek-ai/dsh", "vue", "typescript"] {
            let started = Instant::now();
            let history = tauri::async_runtime::block_on(versions(
                name.to_string(),
                RequestOptions {
                    registry: None,
                    proxy: None,
                    insecure: false,
                },
            ))
            .expect("版本历史应成功");

            println!(
                "{name}：共 {} 个版本（截断 {}，下载来源 {:?}，错误 {:?}），最近发布 {:?}，回传 {} 条，耗时 {:?}",
                history.total,
                history.truncated,
                history.downloads_source,
                history.downloads_error,
                history.last_published_at,
                history.versions.len(),
                started.elapsed()
            );
            for item in history.versions.iter().take(3) {
                println!(
                    "  {} 标签={:?} 发布={:?} 周下载={:?}",
                    item.version, item.tags, item.published_at, item.downloads_last_week
                );
            }

            assert!(!history.versions.is_empty());
            assert!(
                history.versions.iter().any(|item| item.published_at.is_some()),
                "版本应带发布时间"
            );
            assert!(history.total >= history.versions.len());
            assert!(history.versions.len() <= MAX_VERSION_ROWS);
        }
    }

    /// 真机验证批量取最新版本，包含作用域包名（URL 里带 @ 与 /）。
    #[test]
    #[ignore]
    fn real_latest_versions() {
        let names = vec![
            "typescript".to_string(),
            "@nestjs/cli".to_string(),
            "vite".to_string(),
        ];
        let results = tauri::async_runtime::block_on(latest_versions(
            names,
            RequestOptions {
                registry: None,
                proxy: None,
                insecure: false,
            },
            3,
        ));
        for item in &results {
            println!("{} → 最新 {:?}（错误 {:?}）", item.name, item.latest, item.error);
        }
        assert_eq!(results.len(), 3);
        assert!(
            results.iter().any(|item| item.name == "@nestjs/cli" && item.latest.is_some()),
            "作用域包也应能取到版本"
        );
    }
}
