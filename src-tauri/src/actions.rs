//! 安装 / 卸载 / 升级的命令构造与执行。
//!
//! 安全前提：正常路径一律用**参数数组**直接传 argv，不拼 shell 字符串。
//! 唯一的例外是需要管理员权限的 brew cask——osascript 要求把整条命令作为
//! 一个字符串传入，因此那条路径会先做严格字符白名单校验，不通过就拒绝执行，
//! 而不是"尽力转义"。

use std::sync::Mutex;

use serde::Serialize;

use crate::runner;
use crate::shell_env;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    Uninstall,
    Upgrade,
}

impl Action {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "install" => Ok(Self::Install),
            "uninstall" => Ok(Self::Uninstall),
            "upgrade" => Ok(Self::Upgrade),
            other => Err(format!("未知操作：{other}")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Uninstall => "uninstall",
            Self::Upgrade => "upgrade",
        }
    }

    fn is_destructive(self) -> bool {
        matches!(self, Self::Uninstall | Self::Upgrade)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedCommand {
    pub source: String,
    pub action: String,
    /// 最终执行的程序；需要管理员权限时会变成 osascript
    pub program: String,
    pub args: Vec<String>,
    /// 传给子进程的额外环境变量（数据源、代理、证书开关）
    pub env: Vec<EnvVar>,
    /// 展示给用户确认的完整命令
    pub display: String,
    pub requires_admin: bool,
    pub destructive: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
}

/// 设置页里的网络配置，原样传给安装类命令。
///
/// 全部留空表示"不干预"——包管理器按自己的 `.npmrc` / 环境走，行为与命令行一致。
#[derive(Clone, Debug, Default)]
pub struct NetworkOptions {
    pub registry: Option<String>,
    pub proxy: Option<String>,
    pub insecure: bool,
}

// ---------------------------------------------------------------- 包名校验

const NPM_NAME_MAX: usize = 214;

/// npm 包名规则：可选的 `@scope/` 前缀，其余只允许小写字母数字与 `-_.~`。
/// 校验失败即拒绝——不要试图"清洗"用户输入后继续执行。
pub fn validate_npm_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > NPM_NAME_MAX {
        return Err("包名长度不合法".to_string());
    }

    let body = match name.strip_prefix('@') {
        Some(rest) => {
            let (scope, package) = rest
                .split_once('/')
                .ok_or_else(|| "作用域包名缺少 /".to_string())?;
            validate_chars(scope, "作用域名")?;
            validate_chars(package, "包名")?;
            if package.contains('/') {
                return Err("包名中出现多余的 /".to_string());
            }
            return Ok(());
        }
        None => name,
    };
    validate_chars(body, "包名")
}

fn validate_chars(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label}不能为空"));
    }
    if value.starts_with('.') || value.starts_with('_') {
        return Err(format!("{label}不能以 . 或 _ 开头"));
    }
    for ch in value.chars() {
        let allowed = ch.is_ascii_lowercase()
            || ch.is_ascii_digit()
            || matches!(ch, '-' | '_' | '.' | '~');
        if !allowed {
            return Err(format!("{label}包含不允许的字符：{ch}"));
        }
    }
    Ok(())
}

/// brew 名称：允许 tap 路径（`homebrew/cask/foo`）与版本后缀（`node@18`、`gcc@13`）。
pub fn validate_brew_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("名称长度不合法".to_string());
    }
    if name.starts_with('/') || name.starts_with('.') {
        return Err("名称不能以 / 或 . 开头".to_string());
    }
    for ch in name.chars() {
        let allowed = ch.is_ascii_alphanumeric()
            || matches!(ch, '-' | '_' | '.' | '@' | '+' | '/');
        if !allowed {
            return Err(format!("名称包含不允许的字符：{ch}"));
        }
    }
    Ok(())
}

/// 版本号 / 标签：允许 `1.2.3`、`latest`、`^1.2.3`、`~1.2` 等常见写法。
pub fn validate_version(version: &str) -> Result<(), String> {
    if version.is_empty() || version.len() > 64 {
        return Err("版本号长度不合法".to_string());
    }
    for ch in version.chars() {
        let allowed = ch.is_ascii_alphanumeric()
            || matches!(ch, '.' | '-' | '+' | '^' | '~' | '*' | 'x' | 'X');
        if !allowed {
            return Err(format!("版本号包含不允许的字符：{ch}"));
        }
    }
    Ok(())
}

// ------------------------------------------------------------ 命令构造

struct Spec {
    program: String,
    args: Vec<String>,
}

fn spec_for(source: &str, action: Action, name: &str) -> Result<Spec, String> {
    let target = name.to_string();
    Ok(match (source, action) {
        ("npm", Action::Install) | ("npm", Action::Upgrade) => Spec {
            program: "npm".into(),
            args: vec!["install".into(), "-g".into(), target],
        },
        ("npm", Action::Uninstall) => Spec {
            program: "npm".into(),
            args: vec!["uninstall".into(), "-g".into(), target],
        },
        ("pnpm", Action::Install) | ("pnpm", Action::Upgrade) => Spec {
            program: "pnpm".into(),
            args: vec!["add".into(), "-g".into(), target],
        },
        ("pnpm", Action::Uninstall) => Spec {
            program: "pnpm".into(),
            args: vec!["remove".into(), "-g".into(), target],
        },
        ("bun", Action::Install) | ("bun", Action::Upgrade) => Spec {
            program: "bun".into(),
            args: vec!["add".into(), "-g".into(), target],
        },
        ("bun", Action::Uninstall) => Spec {
            program: "bun".into(),
            args: vec!["remove".into(), "-g".into(), target],
        },
        // deno 的全局安装必须显式授权，用 -A；界面上要提示这一点
        ("deno", Action::Install) | ("deno", Action::Upgrade) => Spec {
            program: "deno".into(),
            args: vec!["install".into(), "-g".into(), "-A".into(), target],
        },
        ("deno", Action::Uninstall) => Spec {
            program: "deno".into(),
            args: vec!["uninstall".into(), "-g".into(), target],
        },
        _ => return Err(format!("{source} 不支持{action:?}")),
    })
}

fn brew_spec(action: Action, name: &str, kind: Option<&str>) -> Spec {
    let is_cask = kind == Some("cask");
    let mut args: Vec<String> = Vec::new();
    match action {
        Action::Install => {
            args.push("install".into());
            if is_cask {
                args.push("--cask".into());
            }
        }
        Action::Upgrade => {
            args.push("upgrade".into());
            if is_cask {
                args.push("--cask".into());
            }
        }
        Action::Uninstall => {
            args.push("uninstall".into());
            if is_cask {
                args.push("--cask".into());
            }
        }
    }
    args.push(name.to_string());
    Spec {
        program: "brew".into(),
        args,
    }
}

/// 为管理员路径拼接 shell 命令。这里的输入必须已经通过白名单校验。
///
/// `export PATH=...` 与后面的程序之间**必须有 `;`**：`export` 是普通命令，
/// 写成 `export PATH=/x 'brew' 'install' 'docker'` 时后面的词会被当成要导出的
/// 变量名，程序根本不会被执行——本机 /bin/sh 实测要么报
/// `not a valid identifier`（参数里带 `-`），要么静默退出 0（全为合法标识符）。
fn shell_join(spec: &Spec) -> String {
    let mut parts = vec![format!("export PATH={};", shell_quote(&shell_env::path_env()))];
    parts.push(shell_quote(&spec.program));
    for arg in &spec.args {
        parts.push(shell_quote(arg));
    }
    parts.join(" ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// 构造将要执行的命令。`admin` 为真时改用 osascript 取得管理员权限。
pub fn plan(
    source: &str,
    action: Action,
    name: &str,
    version: Option<&str>,
    kind: Option<&str>,
    admin: bool,
) -> Result<PlannedCommand, String> {
    let target = match (source, version) {
        ("brew", _) => {
            validate_brew_name(name)?;
            name.to_string()
        }
        (_, Some(version)) if !version.is_empty() => {
            validate_npm_name(name)?;
            validate_version(version)?;
            if action == Action::Uninstall {
                name.to_string()
            } else {
                format!("{name}@{version}")
            }
        }
        _ => {
            validate_npm_name(name)?;
            name.to_string()
        }
    };

    let spec = if source == "brew" {
        brew_spec(action, &target, kind)
    } else {
        spec_for(source, action, &target)?
    };

    let requires_admin = admin && source == "brew";
    let (program, args, display) = if requires_admin {
        let inner = shell_join(&spec);
        (
            "osascript".to_string(),
            vec![
                "-e".to_string(),
                format!(
                    "do shell script \"{}\" with administrator privileges",
                    applescript_escape(&inner)
                ),
            ],
            inner,
        )
    } else {
        let display = std::iter::once(spec.program.clone())
            .chain(spec.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        (spec.program.clone(), spec.args.clone(), display)
    };

    Ok(PlannedCommand {
        source: source.to_string(),
        action: action.as_str().to_string(),
        program,
        args,
        env: Vec::new(),
        display,
        requires_admin,
        destructive: action.is_destructive(),
    })
}

/// 按来源决定要注入哪些环境变量。
///
/// - npm 系（npm / pnpm / bun / deno）统一认 `NPM_CONFIG_REGISTRY`
/// - brew 的"源"是 tap 仓库而不是 registry，证书开关对它也没意义，因此只传代理
/// - 代理同时写 `HTTPS_PROXY` 与 `HTTP_PROXY`：三个包管理器都按这两个变量走
pub fn env_for(source: &str, network: &NetworkOptions) -> Vec<EnvVar> {
    let mut env = proxy_env(network.proxy.as_deref(), network.insecure);
    if source == "brew" {
        return env;
    }
    if let Some(registry) = trimmed(&network.registry) {
        push_env(&mut env, "NPM_CONFIG_REGISTRY", registry);
    }
    env
}

/// 代理与证书开关：所有要联网的子进程都该带上，包括诊断页的命令试跑
/// （`brew update` 这类命令在访达启动的 GUI 里同样拿不到终端变量）。
pub fn proxy_env(proxy: Option<&str>, insecure: bool) -> Vec<EnvVar> {
    let mut env = Vec::new();
    if let Some(proxy) = proxy.map(str::trim).filter(|value| !value.is_empty()) {
        // 三个包管理器都读这两个变量，缺一个就会出现"http 走了代理、https 没走"
        push_env(&mut env, "HTTPS_PROXY", proxy);
        push_env(&mut env, "HTTP_PROXY", proxy);
    }
    if insecure {
        // node 系的工具靠它跳过自签 / 拦截证书的校验，与探测里的开关同一个意思
        push_env(&mut env, "NODE_TLS_REJECT_UNAUTHORIZED", "0");
    }
    env
}

fn trimmed(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|item| !item.is_empty())
}

fn push_env(env: &mut Vec<EnvVar>, key: &str, value: &str) {
    env.push(EnvVar {
        key: key.to_string(),
        value: value.to_string(),
    });
}

/// 与 `plan` 相同，但把设置里的数据源 / 代理一起写进命令环境。
///
/// 为什么不让包管理器自己读 `.npmrc`：GUI 从访达启动时连终端的环境变量都拿不到，
/// 不显式传就会出现"应用按设置查询、安装却走了另一套源"的分裂。
pub fn plan_with_network(
    source: &str,
    action: Action,
    name: &str,
    version: Option<&str>,
    kind: Option<&str>,
    admin: bool,
    network: &NetworkOptions,
) -> Result<PlannedCommand, String> {
    let mut planned = plan(source, action, name, version, kind, admin)?;
    let env = env_for(source, network);
    if env.is_empty() {
        return Ok(planned);
    }

    if planned.requires_admin {
        // osascript 路径：变量必须写进真正被执行的 shell 字符串里，否则不会生效。
        // 这里同样要 `export ...;` 而不能写成「变量前缀 + 命令」——display 以
        // `export PATH=...;` 开头，前缀写法会让后面的程序变成 export 的参数。
        let assignments = env
            .iter()
            .map(|item| format!("{}={}", item.key, shell_quote(&item.value)))
            .collect::<Vec<_>>()
            .join(" ");
        let inner = format!("export {assignments}; {}", planned.display);
        planned.args[1] = format!(
            "do shell script \"{}\" with administrator privileges",
            applescript_escape(&inner)
        );
        planned.display = inner;
    } else {
        let prefix = env
            .iter()
            .map(|item| format!("{}={}", item.key, item.value))
            .collect::<Vec<_>>()
            .join(" ");
        planned.display = format!("{prefix} {}", planned.display);
    }

    planned.env = env;
    Ok(planned)
}

/// 打开本地目标：`open` 为启动应用，`reveal` 为在访达中显示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Open,
    Reveal,
}

impl OpenMode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "open" => Ok(Self::Open),
            "reveal" => Ok(Self::Reveal),
            other => Err(format!("未知的打开方式：{other}")),
        }
    }
}

/// 校验待打开的路径。
///
/// `open <path>` 会按文件类型交给系统处理，等于把"执行权"交给了系统默认处理器，
/// 因此这里只放行我们真正会用的两类目标：应用包目录（.app），以及确实存在的路径
/// （在访达中显示）。路径必须绝对、必须存在，且一律作为 argv 传入，不经过 shell。
pub fn validate_open_target(path: &str, mode: OpenMode) -> Result<std::path::PathBuf, String> {
    if path.trim().is_empty() {
        return Err("路径为空".to_string());
    }
    let candidate = std::path::PathBuf::from(path);
    if !candidate.is_absolute() {
        return Err(format!("只允许绝对路径：{path}"));
    }
    let resolved = candidate
        .canonicalize()
        .map_err(|error| format!("路径不存在或不可访问（{path}）：{error}"))?;
    if mode == OpenMode::Open && !path.ends_with(".app") {
        return Err(format!("只能直接打开 .app 应用包，收到：{path}"));
    }
    Ok(resolved)
}

pub fn open_target(path: &str, mode: OpenMode) -> Result<String, String> {
    let resolved = validate_open_target(path, mode)?;
    let mut command = std::process::Command::new("open");
    if mode == OpenMode::Reveal {
        command.arg("-R");
    }
    let output = command
        .arg(&resolved)
        .output()
        .map_err(|error| format!("调用 open 失败：{error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("open 失败：{}", stderr.trim()));
    }
    Ok(resolved.display().to_string())
}

// ------------------------------------------------------------ 并发锁

fn active_task() -> &'static Mutex<Option<String>> {
    static ACTIVE: std::sync::OnceLock<Mutex<Option<String>>> = std::sync::OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(None))
}

/// 同一时刻只允许一个安装类任务在跑。前端也会禁用按钮，但这里才是真正的约束。
pub fn running_task() -> Option<String> {
    active_task().lock().ok().and_then(|guard| guard.clone())
}

fn acquire(task_id: &str) -> Result<(), String> {
    let mut guard = active_task().lock().map_err(|_| "任务锁不可用".to_string())?;
    if let Some(existing) = guard.clone() {
        return Err(format!("已有任务正在进行中（{existing}），请先等待或取消"));
    }
    *guard = Some(task_id.to_string());
    Ok(())
}

fn release(task_id: &str) {
    if let Ok(mut guard) = active_task().lock() {
        if guard.as_deref() == Some(task_id) {
            *guard = None;
        }
    }
}

// ------------------------------------------------------------ 执行

pub type Emit = runner::Emitter;

pub fn run(
    task_id: String,
    planned: &PlannedCommand,
    emit: Emit,
) -> Result<u32, String> {
    acquire(&task_id)?;
    let hook_id = task_id.clone();
    let envs: Vec<(String, String)> = planned
        .env
        .iter()
        .map(|item| (item.key.clone(), item.value.clone()))
        .collect();

    let result = runner::runner().spawn_with_hook(
        task_id.clone(),
        planned.program.clone(),
        planned.args.clone(),
        None,
        envs,
        emit,
        Some(Box::new(move |_code| release(&hook_id))),
    );
    if result.is_err() {
        // 启动失败要立刻放锁，否则后续操作全被挡住
        release(&task_id);
    }
    result
}

/// 从输出里粗略判断是不是权限问题，用于决定要不要提示"以管理员权限重试"。
pub fn looks_like_permission_issue(lines: &[String]) -> bool {
    let keywords = [
        "permission denied",
        "requires a password",
        "password required",
        "sudo",
        "not writable",
        "operation not permitted",
    ];
    lines.iter().any(|line| {
        let lower = line.to_lowercase();
        keywords.iter().any(|keyword| lower.contains(keyword))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_injection_attempts() {
        for evil in [
            "foo; rm -rf /",
            "foo && curl evil.sh",
            "foo`whoami`",
            "foo$(id)",
            "foo|cat /etc/passwd",
            "foo name",
            "../../etc/passwd",
            ".hidden",
            "_private",
            "",
        ] {
            assert!(
                validate_npm_name(evil).is_err(),
                "应当拒绝非法包名：{evil:?}"
            );
        }
    }

    #[test]
    fn accepts_real_package_names() {
        for good in ["vite", "@vue/cli", "typescript", "node-pty", "lodash.merge", "a1", "@scope/a.b_c"] {
            assert!(validate_npm_name(good).is_ok(), "应当接受合法包名：{good}");
        }
    }

    #[test]
    fn validates_versions() {
        for good in ["1.2.3", "latest", "^1.2.3", "~1.2", "8.0.16"] {
            assert!(validate_version(good).is_ok(), "应接受版本：{good}");
        }
        for bad in ["1.2.3; rm -rf /", "1.2.3 && id", "$(id)", "1.2.3/x"] {
            assert!(validate_version(bad).is_err(), "应拒绝版本：{bad}");
        }
    }

    #[test]
    fn validates_brew_names() {
        for good in ["ripgrep", "node@18", "homebrew/cask/docker", "gcc@13", "python@3.12"] {
            assert!(validate_brew_name(good).is_ok(), "应接受：{good}");
        }
        for bad in ["ripgrep; id", "rip grep", "/bin/sh", "../x"] {
            assert!(validate_brew_name(bad).is_err(), "应拒绝：{bad}");
        }
    }

    #[test]
    fn maps_commands_per_manager() {
        let npm = plan("npm", Action::Install, "vite", None, None, false).unwrap();
        assert_eq!(npm.program, "npm");
        assert_eq!(npm.args, vec!["install", "-g", "vite"]);
        assert!(!npm.requires_admin);
        assert!(!npm.destructive);

        let pnpm = plan("pnpm", Action::Install, "vite", None, None, false).unwrap();
        assert_eq!(pnpm.args, vec!["add", "-g", "vite"]);

        let bun = plan("bun", Action::Install, "vite", None, None, false).unwrap();
        assert_eq!(bun.args, vec!["add", "-g", "vite"]);

        let deno = plan("deno", Action::Install, "vite", None, None, false).unwrap();
        assert_eq!(deno.args, vec!["install", "-g", "-A", "vite"]);

        let uninstall = plan("npm", Action::Uninstall, "vite", None, None, false).unwrap();
        assert_eq!(uninstall.args, vec!["uninstall", "-g", "vite"]);
        assert!(uninstall.destructive);
    }

    #[test]
    fn appends_version_only_for_install_and_upgrade() {
        let install = plan("npm", Action::Install, "vite", Some("8.0.16"), None, false).unwrap();
        assert_eq!(install.args, vec!["install", "-g", "vite@8.0.16"]);
        assert_eq!(install.display, "npm install -g vite@8.0.16");

        let upgrade = plan("pnpm", Action::Upgrade, "vite", Some("latest"), None, false).unwrap();
        assert_eq!(upgrade.args, vec!["add", "-g", "vite@latest"]);

        // 卸载不带版本
        let remove = plan("npm", Action::Uninstall, "vite", Some("8.0.16"), None, false).unwrap();
        assert_eq!(remove.args, vec!["uninstall", "-g", "vite"]);
    }

    #[test]
    fn brew_cask_gets_flag() {
        let formula = plan("brew", Action::Upgrade, "ripgrep", None, Some("formula"), false).unwrap();
        assert_eq!(formula.args, vec!["upgrade", "ripgrep"]);

        let cask = plan("brew", Action::Uninstall, "docker", None, Some("cask"), false).unwrap();
        assert_eq!(cask.args, vec!["uninstall", "--cask", "docker"]);

        let install = plan("brew", Action::Install, "docker", None, Some("cask"), false).unwrap();
        assert_eq!(install.args, vec!["install", "--cask", "docker"]);
    }

    #[test]
    fn admin_path_uses_osascript_and_quotes_arguments() {
        let planned = plan("brew", Action::Install, "docker", None, Some("cask"), true).unwrap();
        assert_eq!(planned.program, "osascript");
        assert!(planned.requires_admin);
        let script = &planned.args[1];
        assert!(
            script.contains("with administrator privileges"),
            "脚本应请求管理员权限：{script}"
        );
        // 参数被单引号包裹，路径里的空格不会拆成两个参数
        assert!(script.contains("'brew'"), "脚本应引用 brew：{script}");
        assert!(script.contains("'--cask'"), "脚本应引用 --cask：{script}");
        assert!(script.contains("'docker'"), "脚本应引用包名：{script}");
        // export 之后必须换命令，否则 shell 会把程序当成要导出的变量名
        assert!(
            script.contains("'; 'brew'") || script.contains("'; '"),
            "PATH 导出与程序之间要有分隔符：{script}"
        );
    }

    #[test]
    fn admin_flag_ignored_for_non_brew() {
        let planned = plan("npm", Action::Install, "vite", None, None, true).unwrap();
        assert!(!planned.requires_admin);
        assert_eq!(planned.program, "npm");
    }

    #[test]
    fn refuses_admin_when_name_is_unsafe() {
        // 校验在拼接之前发生，非法名根本进不到 shell 字符串里
        assert!(plan("brew", Action::Install, "x; rm -rf /", None, None, true).is_err());
    }

    #[test]
    fn detects_permission_issues_in_output() {
        assert!(looks_like_permission_issue(&[
            "Error: Permission denied @ dir_s_mkdir".to_string()
        ]));
        assert!(looks_like_permission_issue(&[
            "sudo: a password is required".to_string()
        ]));
        assert!(!looks_like_permission_issue(&["Downloaded 1 file".to_string()]));
    }

    /// 用无害命令走完整执行链路：加锁 → 流式输出 → 退出回调释放锁。
    /// 刻意不调用任何真实包管理器，避免测试改动本机全局环境。
    #[test]
    fn run_locks_during_execution_and_releases_after() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};

        let collected: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = collected.clone();
        let emit: Emit = Arc::new(move |event| {
            if let runner::ProcEvent::Output { line, .. } = event {
                sink.lock().unwrap().push(line);
            }
        });

        let planned = PlannedCommand {
            source: "test".into(),
            action: "install".into(),
            program: "sh".into(),
            args: vec!["-c".into(), "echo hello; sleep 0.3; echo done".into()],
            env: Vec::new(),
            display: "sh -c ...".into(),
            requires_admin: false,
            destructive: false,
        };

        run("test-lock-1".to_string(), &planned, emit.clone()).expect("应能启动");
        assert_eq!(
            running_task().as_deref(),
            Some("test-lock-1"),
            "执行期间应持有锁"
        );

        let second = run("test-lock-2".to_string(), &planned, emit);
        assert!(second.is_err(), "并发第二个任务应被拒绝");

        let deadline = Instant::now() + Duration::from_secs(10);
        while running_task().is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(running_task().is_none(), "退出后应释放锁");

        let lines = collected.lock().unwrap().clone();
        assert!(lines.iter().any(|line| line == "hello"), "实际输出：{lines:?}");
        assert!(lines.iter().any(|line| line == "done"), "实际输出：{lines:?}");
    }

    /// 设置里的数据源与代理要变成子进程的环境变量：npm 系统一读 NPM_CONFIG_REGISTRY，
    /// brew 的源是 tap（git 仓库），只认代理。
    #[test]
    fn injects_registry_and_proxy_per_source() {
        let network = NetworkOptions {
            registry: Some("https://registry.npmjs.org/".into()),
            proxy: Some("http://127.0.0.1:7890".into()),
            insecure: true,
        };

        let npm = env_for("npm", &network);
        let keys: Vec<&str> = npm.iter().map(|item| item.key.as_str()).collect();
        assert!(keys.contains(&"NPM_CONFIG_REGISTRY"), "npm 应收到 registry：{keys:?}");
        assert!(keys.contains(&"HTTPS_PROXY"), "npm 应收到代理：{keys:?}");
        assert!(keys.contains(&"HTTP_PROXY"), "代理要同时写 HTTP 与 HTTPS：{keys:?}");
        assert!(keys.contains(&"NODE_TLS_REJECT_UNAUTHORIZED"), "跳过证书开关应传下去");

        let brew = env_for("brew", &network);
        let brew_keys: Vec<&str> = brew.iter().map(|item| item.key.as_str()).collect();
        assert!(!brew_keys.contains(&"NPM_CONFIG_REGISTRY"), "brew 不该收到 npm 的源");
        assert!(brew_keys.contains(&"HTTPS_PROXY"), "brew 仍应走代理");

        // 留空表示不干预：交给包管理器自己的配置
        assert!(env_for("pnpm", &NetworkOptions::default()).is_empty(), "默认配置不该注入变量");
        let blank = env_for(
            "pnpm",
            &NetworkOptions {
                registry: Some("  ".into()),
                proxy: None,
                insecure: false,
            },
        );
        assert!(blank.is_empty(), "空白字符串应按未设置处理");
    }

    /// 确认框展示的必须是真正会执行的那条命令，环境变量同样要露出来。
    #[test]
    fn planned_display_shows_environment_prefix() {
        let network = NetworkOptions {
            registry: Some("https://registry.npmjs.org".into()),
            proxy: None,
            insecure: false,
        };
        let planned = plan_with_network("npm", Action::Install, "vite", None, None, false, &network)
            .expect("应能构造");
        assert_eq!(
            planned.display,
            "NPM_CONFIG_REGISTRY=https://registry.npmjs.org npm install -g vite"
        );
        assert_eq!(planned.args, vec!["install", "-g", "vite"], "argv 不变，配置只走环境");

    }

    /// 管理员路径拼出来的字符串必须是「先 export，再执行命令」。
    ///
    /// 只断言字符串里含 `'brew'` 是不够的：`export PATH=... 'brew' 'install' 'docker'`
    /// 同样含它，但 /bin/sh 会把程序与参数当成 export 的变量名，命令根本不执行
    /// （实测退出码 0、没有任何输出）。所以这里把 brew 换成 /bin/echo 后用真实的
    /// /bin/sh 跑一遍——osascript 的 `do shell script` 用的就是它。
    #[test]
    fn admin_shell_string_really_runs_the_command() {
        let network = NetworkOptions {
            registry: None,
            proxy: Some("http://127.0.0.1:7890".into()),
            insecure: false,
        };
        let planned = plan_with_network(
            "brew",
            Action::Install,
            "docker",
            None,
            Some("cask"),
            true,
            &network,
        )
        .expect("应能构造");
        assert!(planned.requires_admin);
        assert!(planned.args[1].contains("with administrator privileges"), "{}", planned.args[1]);

        // 代理值要带引号写进脚本，且必须出现在分隔符之前
        let assignments = planned.display.split(';').next().unwrap_or_default();
        assert!(
            assignments.contains("HTTPS_PROXY='http://127.0.0.1:7890'"),
            "变量应在命令之前导出：{}",
            planned.display
        );

        // 真正执行一次：把 brew 换成无害的 /bin/echo，再补一句打印环境变量。
        //
        // 替换必须先确认生效：这段字符串最后是要交给 shell 跑的，
        // 万一以后 shell_join 改了引号方式导致 replace 落空，这里就会真的
        // 执行 `brew install --cask docker`——把断言放在 spawn 之前是唯一的闸门。
        let substituted = planned.display.replace("'brew'", "'/bin/echo'");
        assert!(
            substituted.contains("'/bin/echo' 'install' '--cask' 'docker'"),
            "替换未生效，绝不能把这条命令交给 shell：{substituted}"
        );
        let script = format!("{substituted} && /bin/echo \"PROXY=${{HTTPS_PROXY}}\"");
        let output = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&script)
            .output()
            .expect("应能调用 /bin/sh");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "命令应当执行成功，脚本：{script}；stderr：{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            stdout.contains("install --cask docker"),
            "程序与参数应当原样传给子进程，实际输出：{stdout:?}"
        );
        assert!(
            stdout.contains("PROXY=http://127.0.0.1:7890"),
            "导出的变量应当对同一条命令可见，实际输出：{stdout:?}"
        );
    }

    /// 只打印真实命令，不执行——用于人工核对各组合的最终命令是否正确。
    #[test]
    #[ignore]
    fn print_real_plans_without_executing() {
        let cases = [
            ("npm", Action::Install, "vite", Some("8.0.16"), None, false),
            ("pnpm", Action::Install, "vite", None, None, false),
            ("bun", Action::Install, "opencode-ai", None, None, false),
            ("deno", Action::Install, "vite", None, None, false),
            ("npm", Action::Uninstall, "typescript", None, None, false),
            ("pnpm", Action::Uninstall, "@pnpm/exe", None, None, false),
            ("bun", Action::Uninstall, "opencode-ai", None, None, false),
            ("deno", Action::Uninstall, "some-cli", None, None, false),
            ("brew", Action::Upgrade, "ripgrep", None, Some("formula"), false),
            ("brew", Action::Uninstall, "docker", None, Some("cask"), false),
            ("brew", Action::Install, "docker", None, Some("cask"), true),
        ];
        for (source, action, name, version, kind, admin) in cases {
            let planned = plan(source, action, name, version, kind, admin).expect("应能构造");
            println!(
                "{} {} {:<14} → {}（管理员：{}，破坏性：{}）",
                source,
                planned.action,
                name,
                planned.display,
                planned.requires_admin,
                planned.destructive
            );
        }
    }

    /// 打开目标只放行绝对路径，且「直接打开」仅限 .app——open 会按类型交给系统处理
    #[test]
    fn open_target_validates_path_and_mode() {
        // 相对路径一律拒绝
        let relative = validate_open_target("Applications/Docker.app", OpenMode::Open)
            .expect_err("相对路径应拒绝");
        assert!(relative.contains("绝对路径"), "{relative}");

        // 不存在的路径拒绝
        let missing = validate_open_target("/Applications/NoSuchApp-12345.app", OpenMode::Reveal)
            .expect_err("不存在的路径应拒绝");
        assert!(missing.contains("不存在"), "{missing}");

        // 存在但不是 .app 时只能「在访达中显示」，不能直接打开
        let dir = std::env::temp_dir();
        let resolved = dir.canonicalize().expect("临时目录应可解析");
        let as_string = resolved.display().to_string();
        assert!(validate_open_target(&as_string, OpenMode::Reveal).is_ok());
        let refused = validate_open_target(&as_string, OpenMode::Open).expect_err("非 .app 应拒绝");
        assert!(refused.contains(".app"), "{refused}");

        assert!(OpenMode::parse("open").is_ok() && OpenMode::parse("reveal").is_ok());
        assert!(OpenMode::parse("delete").is_err());
    }

    /// 真机验证：真实存在的 .app 才能通过「直接打开」
    #[test]
    fn open_target_accepts_existing_app_bundle() {
        let candidates = [
            "/System/Applications/Calculator.app",
            "/Applications/Safari.app",
        ];
        let Some(target) = candidates.iter().find(|path| std::path::Path::new(path).exists()) else {
            println!("本机没有可用于验证的 .app，跳过");
            return;
        };
        let resolved = validate_open_target(target, OpenMode::Open).expect("应放行 .app");
        assert!(resolved.display().to_string().ends_with(".app"));
        // 在访达中显示对同一路径同样成立（只是不加 -R 之外的差别）
        assert!(validate_open_target(target, OpenMode::Reveal).is_ok());
    }
}
