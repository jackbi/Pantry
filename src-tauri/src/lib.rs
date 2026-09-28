//! Pantry 后端入口。
//!
//! 阶段 0 只暴露"技术验证"需要的命令：环境自检、流式执行、取消、网络探测。

mod actions;
mod brew;
mod links;
mod managers;
mod probe;
mod market;
mod packages;
mod runner;
mod shell_env;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

/// 解析并缓存登录 shell 的 PATH。
#[tauri::command]
async fn diagnose_environment() -> Result<shell_env::PathReport, String> {
    // 调试构建下打点到终端：用来确认前端确实跑起来并打通了 IPC
    #[cfg(debug_assertions)]
    eprintln!("[ipc] diagnose_environment 被调用");
    tauri::async_runtime::spawn_blocking(|| shell_env::report().clone())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn resolve_program(program: String) -> Option<String> {
    shell_env::resolve_program(&program).map(|path| path.display().to_string())
}

/// 启动子进程并立即返回任务 id；输出通过 `proc://event` 事件推送。
#[tauri::command]
fn run_command(
    app: AppHandle,
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<String, String> {
    let id = format!(
        "proc-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default()
    );
    let sink = app.clone();
    let emit: runner::Emitter = Arc::new(move |event| {
        let _ = sink.emit(runner::PROC_EVENT, event);
    });
    // 命令试跑是诊断工具：只带代理与证书开关，不动数据源，避免改变 npm 系命令的语义
    let envs: Vec<(String, String)> = actions::proxy_env(proxy.as_deref(), insecure.unwrap_or(false))
        .into_iter()
        .map(|item| (item.key, item.value))
        .collect();
    runner::runner().spawn(id.clone(), program, args, cwd, envs, emit)?;
    Ok(id)
}

#[tauri::command]
fn cancel_command(id: String) -> Result<bool, String> {
    runner::runner().kill(&id)
}

#[tauri::command]
fn command_status(id: String) -> bool {
    runner::runner().is_running(&id)
}

/// 采集各包管理器的已安装全局包。多个来源并行，单个失败不影响其他来源。
#[tauri::command]
async fn list_installed_packages(
    sources: Option<Vec<String>>,
) -> Result<packages::InstalledReport, String> {
    let report = tauri::async_runtime::spawn_blocking(move || packages::scan_all(sources))
        .await
        .map_err(|error| error.to_string())?;
    #[cfg(debug_assertions)]
    eprintln!(
        "[ipc] 采集到 {} 个包，{} 个来源失败",
        report.packages.len(),
        report.errors.len()
    );
    Ok(report)
}

/// 批量测量安装体积，用一次 du 调用完成，避免上百次进程启动。
#[tauri::command]
async fn measure_package_sizes(paths: Vec<String>) -> Result<Vec<packages::SizeEntry>, String> {
    let total = paths.len();
    let entries = tauri::async_runtime::spawn_blocking(move || packages::measure_sizes(paths))
        .await
        .map_err(|error| error.to_string())?;
    #[cfg(debug_assertions)]
    eprintln!("[ipc] 测量体积：{total} 条路径，命中 {}", entries.len());
    Ok(entries)
}

/// 搜索 npm 市场。默认走 npmmirror（官方源在本机直连不通）。
#[tauri::command]
async fn search_packages(
    query: String,
    size: Option<u32>,
    from: Option<u32>,
    registry: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<market::SearchResponse, String> {
    market::search(
        query,
        size.unwrap_or(20),
        from.unwrap_or(0),
        market::RequestOptions {
            registry,
            proxy,
            insecure: insecure.unwrap_or(false),
        },
    )
    .await
}

/// 包详情：`/latest` 清单 + 周下载量。README 单独按需加载。
#[tauri::command]
async fn package_detail(
    name: String,
    registry: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<market::PackageDetail, String> {
    market::detail(
        name,
        market::RequestOptions {
            registry,
            proxy,
            insecure: insecure.unwrap_or(false),
        },
    )
    .await
}

/// README 按需加载：从 jsDelivr 定向取单个文件，不拉完整 packument。
#[tauri::command]
async fn package_readme(
    name: String,
    version: String,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<market::ReadmeResponse, String> {
    market::readme(
        name,
        version,
        market::RequestOptions {
            registry: None,
            proxy,
            insecure: insecure.unwrap_or(false),
        },
    )
    .await
}

/// 版本历史：共多少个版本、发布时间、当前标签、各版本近一周下载量。
/// 与详情分开是因为完整 packument 体积大（gzip 后仍约 1–2MB），只在展开时拉。
#[tauri::command]
async fn package_versions(
    name: String,
    registry: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<market::VersionHistory, String> {
    market::versions(
        name,
        market::RequestOptions {
            registry,
            proxy,
            insecure: insecure.unwrap_or(false),
        },
    )
    .await
}

/// Homebrew 搜索：`brew search` 拿名字，再一次性 `brew info --json=v2` 补描述与安装状态。
/// brew 命令都带 `HOMEBREW_NO_AUTO_UPDATE=1`，避免自动更新把界面卡住几十秒。
#[tauri::command]
async fn brew_search(query: String, proxy: Option<String>) -> Result<brew::BrewSearchResponse, String> {
    brew::search(query, proxy).await
}

/// Homebrew 详情：formula 或 cask，含依赖、冲突、caveats、废弃告警、cask 的 .app 路径。
#[tauri::command]
async fn brew_detail(
    name: String,
    kind: String,
    proxy: Option<String>,
) -> Result<brew::BrewDetail, String> {
    brew::detail(name, kind, proxy).await
}

/// 打开应用包或在访达中显示。路径来自我们自己扫描的 cask 结果，仍会做存在性与绝对路径校验。
#[tauri::command]
fn open_local_target(path: String, mode: String) -> Result<String, String> {
    actions::open_target(&path, actions::OpenMode::parse(&mode)?)
}

/// 外链窗口的 label：固定一个，连点几个链接时是复用同一个窗口，而不是开一堆。
const LINK_WINDOW: &str = "link";

/// 在应用内的独立窗口里打开外链（主页 / 仓库）。
///
/// 用独立窗口而不是子 webview 或 iframe：Tauri 的 multiwebview 还在 `unstable` 特性后面，
/// 而窗口是稳定 API，行为也更好预期。安全边界靠两件事兜住——这里只放行 http/https；
/// 新窗口的 label 不在任何 capability 的 windows 列表里（见 capabilities/default.json），
/// 因此远端页面拿不到本应用的 IPC，只能被动浏览。
#[tauri::command]
async fn open_external_url(app: AppHandle, url: String, title: Option<String>) -> Result<(), String> {
    let parsed = link_target(&url)?;
    let title = link_title(title.as_deref(), &parsed);
    show_link_window(&app, parsed, title)
}

/// 只放行 http/https：其余（`file:`、自定义 scheme）一律拒绝，
/// 免得"打开外链"变成任意本地文件或任意协议的入口。
fn link_target(raw: &str) -> Result<tauri::Url, String> {
    let parsed = tauri::Url::parse(raw.trim()).map_err(|error| format!("链接不是合法 URL：{error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "只允许打开 http/https 链接（收到 {}）",
            parsed.scheme()
        ));
    }
    Ok(parsed)
}

/// 窗口标题：调用方给什么用什么（如「vite · 仓库」），没给就用域名。
fn link_title(title: Option<&str>, url: &tauri::Url) -> String {
    title
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| url.host_str().unwrap_or("链接").to_string())
}

/// 建窗或复用：已经有链接窗口就导航过去，而不是每点一个链接开一扇新窗。
///
/// 抽成对运行时泛型是为了能被测试覆盖——`tauri::test` 的 mock 运行时能真的走一遍建窗
/// 与复用，只是不渲染页面。
fn show_link_window<R: tauri::Runtime, M: tauri::Manager<R>>(
    manager: &M,
    url: tauri::Url,
    title: String,
) -> Result<(), String> {
    if let Some(window) = manager.get_webview_window(LINK_WINDOW) {
        window.navigate(url).map_err(|error| error.to_string())?;
        let _ = window.set_title(&title);
        let _ = window.set_focus();
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(manager, LINK_WINDOW, tauri::WebviewUrl::External(url))
        .title(title)
        .inner_size(1100.0, 820.0)
        .build()
        .map_err(|error| format!("打开链接窗口失败：{error}"))?;
    Ok(())
}

/// 近 30 天安装次数（Homebrew 官方统计，仅 formula）。单独命令，避免第三方统计拖慢详情。
#[tauri::command]
async fn brew_stats(
    name: String,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<brew::BrewStats, String> {
    Ok(brew::stats(name, proxy, insecure.unwrap_or(false)).await)
}

/// 包管理器自检：是否存在、版本、有没有新版本。
#[tauri::command]
async fn manager_status(
    registry: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<Vec<managers::ManagerStatus>, String> {
    let mut statuses = tauri::async_runtime::spawn_blocking(managers::local_statuses)
        .await
        .map_err(|error| error.to_string())?;
    let names = managers::upgrade_candidates(&statuses);
    if !names.is_empty() {
        let latest = market::latest_versions(
            names,
            market::RequestOptions {
                registry,
                proxy,
                insecure: insecure.unwrap_or(false),
            },
            5,
        )
        .await;
        let latest: Vec<(String, Option<String>)> = latest
            .into_iter()
            .map(|item| (item.name, item.latest))
            .collect();
        managers::merge_latest(&mut statuses, &latest);
    }
    Ok(statuses)
}

/// Homebrew 是否可用：决定侧栏是否出现 Homebrew 入口。
#[tauri::command]
async fn brew_availability() -> Result<brew::BrewAvailability, String> {
    tauri::async_runtime::spawn_blocking(brew::availability)
        .await
        .map_err(|error| error.to_string())
}

/// 只构造命令供用户确认，不执行。
#[tauri::command]
fn plan_package_action(
    source: String,
    action: String,
    name: String,
    version: Option<String>,
    kind: Option<String>,
    admin: Option<bool>,
    registry: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<actions::PlannedCommand, String> {
    actions::plan_with_network(
        &source,
        actions::Action::parse(&action)?,
        &name,
        version.as_deref(),
        kind.as_deref(),
        admin.unwrap_or(false),
        &actions::NetworkOptions {
            registry,
            proxy,
            insecure: insecure.unwrap_or(false),
        },
    )
}

/// 执行安装 / 卸载 / 升级：流式输出走 `proc://event`，同一时刻只允许一个任务。
#[tauri::command]
fn run_package_action(
    app: AppHandle,
    task_id: String,
    source: String,
    action: String,
    name: String,
    version: Option<String>,
    kind: Option<String>,
    admin: Option<bool>,
    registry: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<String, String> {
    let planned = actions::plan_with_network(
        &source,
        actions::Action::parse(&action)?,
        &name,
        version.as_deref(),
        kind.as_deref(),
        admin.unwrap_or(false),
        &actions::NetworkOptions {
            registry,
            proxy,
            insecure: insecure.unwrap_or(false),
        },
    )?;

    let sink = app.clone();
    let emit: runner::Emitter = Arc::new(move |event| {
        let _ = sink.emit(runner::PROC_EVENT, event);
    });
    actions::run(task_id.clone(), &planned, emit)?;
    Ok(task_id)
}

#[tauri::command]
fn running_package_action() -> Option<String> {
    actions::running_task()
}

/// 判断失败输出是否属于权限问题，用于提示"以管理员权限重试"。
#[tauri::command]
fn is_permission_issue(lines: Vec<String>) -> bool {
    actions::looks_like_permission_issue(&lines)
}

/// 判断 registry 上的 latest 是否**真的**比本机版本新。
///
/// 已安装列表原本用"不相等"判断，本地装了 canary / 更高版本时会把降级误报成可升级；
/// 这里与诊断自检共用 managers::compare_versions，前端不再自己比版本。
#[tauri::command]
fn newer_versions(probes: Vec<managers::VersionProbe>) -> Vec<managers::VersionVerdict> {
    managers::newer_versions(probes)
}

/// 批量查询 npm 系包在 registry 上的最新版本，用于标记可升级项。
/// 逐个请求 /latest 并限制并发，避免几十个包同时打过去。
#[tauri::command]
async fn latest_versions(
    names: Vec<String>,
    registry: Option<String>,
    proxy: Option<String>,
    insecure: Option<bool>,
) -> Result<Vec<market::LatestVersion>, String> {
    const MAX_NAMES: usize = 120;
    let names: Vec<String> = names.into_iter().take(MAX_NAMES).collect();
    if names.is_empty() {
        return Ok(Vec::new());
    }
    Ok(market::latest_versions(
        names,
        market::RequestOptions {
            registry,
            proxy,
            insecure: insecure.unwrap_or(false),
        },
        6,
    )
    .await)
}

#[tauri::command]
fn proxy_report(explicit: Option<String>) -> probe::ProxyReport {
    probe::resolve_proxy(explicit)
}

/// 内置默认数据源：设置页的占位与「恢复默认」都读它，避免前端写死地址。
#[tauri::command]
fn market_defaults() -> market::MarketDefaults {
    market::defaults()
}

#[tauri::command]
async fn probe_registry(
    url: String,
    proxy: Option<String>,
    insecure: bool,
    timeout_secs: Option<u64>,
) -> probe::ProbeResult {
    let resolved = probe::resolve_proxy(proxy);
    probe::probe(probe::ProbeRequest {
        url,
        proxy: resolved.proxy,
        insecure,
        timeout_secs: timeout_secs.unwrap_or(20),
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            diagnose_environment,
            resolve_program,
            run_command,
            cancel_command,
            command_status,
            list_installed_packages,
            measure_package_sizes,
            search_packages,
            package_detail,
            package_readme,
            package_versions,
            brew_search,
            brew_detail,
            open_local_target,
            open_external_url,
            brew_stats,
            brew_availability,
            manager_status,
            plan_package_action,
            run_package_action,
            running_package_action,
            is_permission_issue,
            newer_versions,
            latest_versions,
            proxy_report,
            market_defaults,
            probe_registry
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 命令行自检：`hengran-public-store --diagnose`
///
/// GUI 应用拿不到终端环境，"为什么找不到 npm" 这类问题在界面里很难定位，
/// 因此提供一个不启动窗口、直接把解析结果打成 JSON 的入口。
pub fn diagnose_cli() -> i32 {
    use serde_json::json;

    let report = shell_env::report();
    let managers: serde_json::Map<String, serde_json::Value> = ["npm", "pnpm", "bun", "deno", "brew"]
        .iter()
        .map(|name| {
            let resolved = shell_env::resolve_program(name)
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "未找到".to_string());
            (name.to_string(), json!(resolved))
        })
        .collect();

    let proxy = probe::resolve_proxy(None);
    let payload = json!({
        "shell": report.shell,
        "shellPath": report.shell_path,
        "source": report.source,
        "resolvedPath": report.resolved,
        "fallbacksAdded": report.fallbacks_added,
        "managers": managers,
        "proxy": proxy.proxy,
        "proxySource": proxy.source,
    });

    match serde_json::to_string_pretty(&payload) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("自检结果序列化失败：{error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    fn mock_app() -> tauri::App<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock app 应能建起来")
    }

    /// 只放行 http/https：`file:` 之类不能借"打开外链"之名被打开。
    #[test]
    fn link_target_only_accepts_http() {
        assert_eq!(
            link_target(" https://vite.dev/ ").unwrap().as_str(),
            "https://vite.dev/"
        );
        assert!(link_target("http://example.com").is_ok());

        for raw in ["file:///etc/passwd", "javascript:alert(1)", "vscode://x", ""] {
            assert!(link_target(raw).is_err(), "{raw} 不该被放行");
        }
    }

    #[test]
    fn link_title_falls_back_to_host() {
        let url = tauri::Url::parse("https://github.com/vitejs/vite").unwrap();
        assert_eq!(link_title(Some(" vite · 仓库 "), &url), "vite · 仓库");
        // 没给标题、或只给了空白，就用域名兜底
        assert_eq!(link_title(None, &url), "github.com");
        assert_eq!(link_title(Some("   "), &url), "github.com");
    }

    /// 真建窗：走一遍「建窗 → 再点一个链接时复用同一扇窗」。
    /// mock 运行时不渲染页面，但窗口创建与复用这条链路是真的被执行了。
    #[test]
    fn link_window_is_created_once_then_reused() {
        let app = mock_app();
        let repo = tauri::Url::parse("https://github.com/vitejs/vite").unwrap();
        show_link_window(&app, repo, "vite · 仓库".to_string()).expect("应能建出链接窗口");
        assert!(
            app.get_webview_window(LINK_WINDOW).is_some(),
            "链接窗口应存在"
        );

        // mock 运行时不会真的导航，标题也不回读，但"已有窗口就复用"这条分支确实被走到了
        let home = tauri::Url::parse("https://vite.dev").unwrap();
        show_link_window(&app, home, "vite · 主页".to_string()).expect("应能复用链接窗口");
        assert_eq!(app.webview_windows().len(), 1, "不该再多开一扇窗");
    }
}
