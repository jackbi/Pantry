//! 解析用户的登录 shell 环境。
//!
//! 从 Finder 启动的 GUI 应用继承不到 `~/.zprofile`、`~/.zshrc` 里配置的 PATH，
//! 直接 `Command::new("npm")` 会报找不到可执行文件。这里在首次调用时起一个
//! 登录+交互式 shell 问一次 PATH，缓存结果，再补上常见的兜底目录。

use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const SENTINEL_START: &str = "__PUBLIC_STORE_PATH_START__";
const SENTINEL_END: &str = "__PUBLIC_STORE_PATH_END__";

static REPORT: OnceLock<PathReport> = OnceLock::new();

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathReport {
    pub shell: String,
    /// 登录 shell 报告的原始 PATH，解析失败时为 None
    pub shell_path: Option<String>,
    pub resolved: Vec<String>,
    /// 登录 shell 里没有、由兜底逻辑补上的目录
    pub fallbacks_added: Vec<String>,
    pub source: String,
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn fallback_dirs() -> Vec<String> {
    let home = home_dir();
    let fixed = [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        "/usr/local/sbin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ];
    fixed
        .iter()
        .map(|dir| dir.to_string())
        .chain([
            format!("{home}/.vite-plus/bin"),
            format!("{home}/.bun/bin"),
            format!("{home}/.cargo/bin"),
            format!("{home}/.deno/bin"),
            format!("{home}/.local/bin"),
        ])
        .collect()
}

fn query_login_shell(shell: &str) -> Option<String> {
    // 用哨兵包裹，避免 shell 启动时打印的问候语污染结果
    let script = format!("printf '%s%s%s' '{SENTINEL_START}' \"$PATH\" '{SENTINEL_END}'");
    let output = Command::new(shell).arg("-lic").arg(&script).output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let start = stdout.find(SENTINEL_START)? + SENTINEL_START.len();
    let end = stdout[start..].find(SENTINEL_END)? + start;
    let path = stdout[start..end].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn build_report() -> PathReport {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let shell_path = query_login_shell(&shell);

    let mut seen: HashSet<String> = HashSet::new();
    let mut resolved: Vec<String> = Vec::new();

    if let Some(path) = &shell_path {
        for dir in path.split(':').filter(|dir| !dir.is_empty()) {
            if seen.insert(dir.to_string()) {
                resolved.push(dir.to_string());
            }
        }
    }

    let mut fallbacks_added = Vec::new();
    for dir in fallback_dirs() {
        if !Path::new(&dir).is_dir() {
            continue;
        }
        if seen.insert(dir.clone()) {
            fallbacks_added.push(dir.clone());
            resolved.push(dir);
        }
    }

    let source = match (shell_path.is_some(), fallbacks_added.is_empty()) {
        (true, true) => "loginShell",
        (true, false) => "loginShell+fallback",
        (false, _) => "fallback",
    }
    .to_string();

    PathReport {
        shell,
        shell_path,
        resolved,
        fallbacks_added,
        source,
    }
}

/// 返回缓存的解析结果，首次调用会启动一次登录 shell。
pub fn report() -> &'static PathReport {
    REPORT.get_or_init(build_report)
}

/// 供子进程使用的 PATH 字符串。
pub fn path_env() -> String {
    report().resolved.join(":")
}

fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) => meta.is_file() && meta.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// 在解析出的 PATH 里查找可执行文件，替代系统的 `which`。
pub fn resolve_program(program: &str) -> Option<PathBuf> {
    let candidate = PathBuf::from(program);
    if candidate.is_absolute() {
        return is_executable(&candidate).then_some(candidate);
    }
    report()
        .resolved
        .iter()
        .map(|dir| PathBuf::from(dir).join(program))
        .find(|path| is_executable(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_contains_system_dirs() {
        let report = report();
        assert!(!report.resolved.is_empty(), "PATH 解析结果不应为空");
        assert!(
            report.resolved.iter().any(|dir| dir == "/usr/bin"),
            "应包含 /usr/bin，实际：{:?}",
            report.resolved
        );
        println!("source = {}", report.source);
        println!("shell = {}，登录 shell PATH = {:?}", report.shell, report.shell_path);
        println!("兜底补入 = {:?}", report.fallbacks_added);
    }

    #[test]
    fn resolves_common_programs() {
        assert!(resolve_program("sh").is_some(), "应能解析到 sh");
        assert!(resolve_program("/bin/sh").is_some(), "绝对路径应直接可用");
        assert!(
            resolve_program("definitely-not-a-real-binary-xyz").is_none(),
            "不存在的程序应返回 None"
        );
    }

    #[test]
    fn report_json_matches_frontend_contract() {
        let json = serde_json::to_string(report()).unwrap();
        for key in ["shellPath", "fallbacksAdded", "resolved", "source"] {
            assert!(json.contains(key), "缺少字段 {key}，实际：{json}");
        }
    }

    /// 模拟 Finder 启动：清空所有环境变量，只留 HOME 与 SHELL。
    /// 这是本阶段最关键的假设验证——如果登录 shell 在干净环境下拿不到完整 PATH，
    /// 整套"用登录 shell 解析 PATH"的方案就不成立。
    #[test]
    fn login_shell_works_with_minimal_env() {
        let home = home_dir();
        let output = Command::new("/bin/zsh")
            .arg("-lic")
            .arg("printf '%s%s%s' '__S__' \"$PATH\" '__E__'")
            .env_clear()
            .env("HOME", &home)
            .env("SHELL", "/bin/zsh")
            .output()
            .expect("应能启动 zsh");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let start = stdout.find("__S__").expect("应输出哨兵起始标记") + 5;
        let end = stdout[start..].find("__E__").expect("应输出哨兵结束标记") + start;
        let path = &stdout[start..end];
        println!("干净环境下的 PATH = {path}");

        assert!(
            path.contains("/opt/homebrew/bin") || path.contains("/usr/local/bin"),
            "干净环境下也应包含 Homebrew 目录，实际：{path}"
        );
        assert!(
            path.contains("/.vite-plus/bin") || path.contains("/.bun/bin"),
            "干净环境下也应包含自定义工具目录，实际：{path}"
        );
    }
}
