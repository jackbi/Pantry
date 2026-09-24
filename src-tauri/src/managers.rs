//! 包管理器自检：**是否可用、什么版本、有没有新版本**。
//!
//! 只做这三件事。PATH 解析、代理探测、命令试跑属于排障细节，放在诊断页的
//! 「环境详情」折叠区里，不该挡在默认视图前面。
//!
//! 版本对比走 registry 的 `/latest`（npm、pnpm、bun、deno 都在 npm 上有同名包）。
//! 两类例外：
//! - node 只是被 npm/pnpm 依赖的运行时，不参与版本对比（升级走 nvm / Homebrew 等）
//! - Homebrew 自己更新自己（`brew update`），也不参与版本对比，而且只在 macOS 上探测

use std::cmp::Ordering;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::brew::NO_AUTO_UPDATE;
use crate::shell_env;

/// 自检清单。node 排在最前：npm 与 pnpm 都跑在它上面。
const PROBED: [&str; 6] = ["node", "npm", "pnpm", "bun", "deno", "brew"];
/// 参与 registry 版本对比的：node 不在其列（npm 上的 `node` 是另一个包，版本号不是运行时版本）
const REGISTRY_COMPARED: [&str; 4] = ["npm", "pnpm", "bun", "deno"];
/// 自己更新自己，不做版本对比
const SELF_UPDATING: [&str; 1] = ["brew"];

/// brew 只在 macOS 上有意义，其它系统不探测、不显示。
pub fn is_relevant(name: &str, os: &str) -> bool {
    name != "brew" || os == "macos"
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCommand {
    pub program: String,
    pub args: Vec<String>,
    /// 展示给用户确认的完整命令
    pub display: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerStatus {
    pub name: String,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// registry 上的最新版本；不参与对比或取不到时为 None
    pub latest: Option<String>,
    pub upgrade_available: bool,
    /// true 表示这个工具自己更新自己（brew），界面不显示"可升级"
    pub self_updating: bool,
    /// 更新命令；node 没有（升级走 nvm / Homebrew 等）
    pub update: Option<UpdateCommand>,
    pub error: Option<String>,
}

/// 自己更新自己的命令。刻意不做"尽力而为"的猜测：写不出确定命令的就不给按钮。
fn update_command(name: &str) -> Option<UpdateCommand> {
    let (program, args): (&str, &[&str]) = match name {
        "npm" => ("npm", &["install", "-g", "npm@latest"]),
        "pnpm" => ("pnpm", &["add", "-g", "pnpm@latest"]),
        "bun" => ("bun", &["upgrade"]),
        "deno" => ("deno", &["upgrade"]),
        "brew" => ("brew", &["update"]),
        _ => return None,
    };
    let args: Vec<String> = args.iter().map(|item| item.to_string()).collect();
    Some(UpdateCommand {
        program: program.to_string(),
        display: std::iter::once(program.to_string())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" "),
        args,
    })
}

/// 取出版本号：各命令输出格式不同，统一规则是"第一个形如 x.y 的 token"。
/// - `10.9.8`（npm）
/// - `v24.9.0`（node，带 v 前缀）
/// - `Homebrew 7.0.4`（brew）
/// - `deno 2.5.0 (stable, release, aarch64-apple-darwin)`（deno）
pub fn parse_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .map(|token| {
            token.trim_end_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
        })
        .find(|token| {
            let body = token.trim_start_matches('v');
            body.chars().next().is_some_and(|c| c.is_ascii_digit()) && body.contains('.')
        })
        .map(|token| token.trim_start_matches('v').to_string())
}

/// 版本比较：按数字段比，预发布版本低于同号正式版（`1.0.0-rc.1` < `1.0.0`）。
///
/// 用比较而不是"不相等"来判断是否有新版：本地装了 canary、而 registry 上是稳定版时，
/// 简单的不相等会误报"可升级"。
pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let split = |value: &str| {
        let value = value.trim().trim_start_matches('v');
        let mut parts = value.splitn(2, '-');
        let release: Vec<u64> = parts
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|segment| segment.parse().unwrap_or(0))
            .collect();
        let prerelease = parts.next().unwrap_or_default().to_string();
        (release, prerelease)
    };

    let (left_release, left_pre) = split(left);
    let (right_release, right_pre) = split(right);
    for index in 0..left_release.len().max(right_release.len()) {
        let a = left_release.get(index).copied().unwrap_or(0);
        let b = right_release.get(index).copied().unwrap_or(0);
        match a.cmp(&b) {
            Ordering::Equal => continue,
            other => return other,
        }
    }

    match (left_pre.is_empty(), right_pre.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => left_pre.cmp(&right_pre),
    }
}

/// 一次版本判断的输入：本机装的版本 + registry 上的 latest。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionProbe {
    pub name: String,
    pub local: String,
    pub latest: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionVerdict {
    pub name: String,
    /// 只有 registry 的版本**确实更大**才算可升级
    pub newer: bool,
}

/// 给已安装列表用：本机装了 canary 或更高版本时，"不相等"会把降级误报成可升级。
///
/// 前端不做版本比较，统一走这里，与诊断自检共用同一个 `compare_versions`。
pub fn newer_versions(probes: Vec<VersionProbe>) -> Vec<VersionVerdict> {
    probes
        .into_iter()
        .map(|probe| VersionVerdict {
            newer: compare_versions(&probe.latest, &probe.local) == Ordering::Greater,
            name: probe.name,
        })
        .collect()
}

fn probe(name: &str) -> ManagerStatus {
    let self_updating = SELF_UPDATING.contains(&name);

    let Some(executable) = shell_env::resolve_program(name) else {
        return ManagerStatus {
            name: name.to_string(),
            available: false,
            path: None,
            version: None,
            latest: None,
            upgrade_available: false,
            self_updating,
            update: update_command(name),
            error: None,
        };
    };

    let mut command = Command::new(&executable);
    command.args(["--version"]).env("PATH", shell_env::path_env());
    // brew 的 --version 也会触发自动更新检查，必须一起关掉
    for (key, value) in NO_AUTO_UPDATE {
        command.env(key, value);
    }
    let output = command.output();
    let (version, error) = match output {
        Ok(output) => {
            let text = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            match parse_version(&text) {
                Some(version) => (Some(version), None),
                None => (None, Some("命令有输出，但读不出版本号".to_string())),
            }
        }
        Err(error) => (None, Some(format!("执行 {name} --version 失败：{error}"))),
    };

    ManagerStatus {
        name: name.to_string(),
        available: true,
        path: Some(executable.display().to_string()),
        version,
        latest: None,
        upgrade_available: false,
        self_updating,
        update: update_command(name),
        error,
    }
}

/// 并行探测本机版本，不联网。最新版本由上层再补（`market::latest_versions`）。
pub fn local_statuses() -> Vec<ManagerStatus> {
    let os = std::env::consts::OS;
    let wanted: Vec<&str> = PROBED
        .iter()
        .copied()
        .filter(|name| is_relevant(name, os))
        .collect();
    std::thread::scope(|scope| {
        let handles: Vec<_> = wanted
            .iter()
            .map(|name| ((*name).to_string(), scope.spawn(move || probe(name))))
            .collect();
        handles
            .into_iter()
            .map(|(name, handle)| {
                handle.join().unwrap_or_else(|_| {
                    let update = update_command(name.as_str());
                    ManagerStatus {
                        name,
                        available: false,
                        path: None,
                        version: None,
                        latest: None,
                        upgrade_available: false,
                        self_updating: false,
                        update,
                        error: Some("探测线程 panic".to_string()),
                    }
                })
            })
            .collect()
    })
}

/// 把 registry 上的最新版本并进自检结果，并判断是否可升级。
pub fn merge_latest(statuses: &mut [ManagerStatus], latest: &[(String, Option<String>)]) {
    for status in statuses.iter_mut() {
        let Some((_, Some(remote))) = latest.iter().find(|(name, _)| name == &status.name) else {
            continue;
        };
        status.latest = Some(remote.clone());
        status.upgrade_available = match &status.version {
            Some(local) => compare_versions(local, remote) == Ordering::Less,
            None => false,
        };
    }
}

/// 需要做版本对比的、且本机确实装了的命令。
pub fn upgrade_candidates(statuses: &[ManagerStatus]) -> Vec<String> {
    statuses
        .iter()
        .filter(|item| {
            item.available && !item.self_updating && REGISTRY_COMPARED.contains(&item.name.as_str())
        })
        .map(|item| item.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    #[test]
    fn parses_version_from_each_real_format() {
        assert_eq!(parse_version("10.9.8").as_deref(), Some("10.9.8"));
        // node 的 --version 带 v 前缀
        assert_eq!(parse_version("v24.9.0").as_deref(), Some("24.9.0"));
        assert_eq!(parse_version("Homebrew 7.0.4").as_deref(), Some("7.0.4"));
        assert_eq!(
            parse_version("deno 2.5.0 (stable, release, aarch64-apple-darwin)\nv8 14.0.0\n")
                .as_deref(),
            Some("2.5.0")
        );
        // bun 有时会先打印提示行，取第一个像版本的 token
        assert_eq!(parse_version("warn: something\n1.3.14\n").as_deref(), Some("1.3.14"));
        assert_eq!(parse_version("没有版本号"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compares_versions_by_numbers_not_strings() {
        // 字符串比较会把 "10.9.8" 判成大于 "12.1.0"
        assert_eq!(compare_versions("10.9.8", "12.1.0"), Ordering::Less);
        assert_eq!(compare_versions("12.1.0", "10.9.8"), Ordering::Greater);
        assert_eq!(compare_versions("1.3.14", "1.4.2"), Ordering::Less);
        assert_eq!(compare_versions("7.0.4", "7.0.4"), Ordering::Equal);
        // 1.0 与 1.0.0 视为同版本
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        // 预发布低于同号正式版
        assert_eq!(compare_versions("1.0.0-rc.1", "1.0.0"), Ordering::Less);
        assert_eq!(compare_versions("1.0.0", "1.0.0-rc.1"), Ordering::Greater);
        // 带 v 前缀也能比
        assert_eq!(compare_versions("v2.0.0", "2.0.1"), Ordering::Less);
    }

    /// 前端不做版本比较，统一走这个命令：装的是 canary / 更高版本时不能报"可升级"
    #[test]
    fn version_probe_json_matches_frontend_contract() {
        // 前端按 camelCase 组装 payload，改这里等于改前端契约
        let probes: Vec<VersionProbe> =
            serde_json::from_str(r#"[{"name":"vite","local":"5.0.0","latest":"6.0.0"}]"#)
                .expect("前端传的 payload 应能反序列化");
        let verdict = &newer_versions(probes)[0];
        let json = serde_json::to_value(verdict).unwrap();
        assert!(json.get("name").is_some(), "缺少 name：{json}");
        assert_eq!(json.get("newer"), Some(&serde_json::Value::Bool(true)));
    }

    #[test]
    fn newer_versions_rejects_downgrades_and_canary() {
        let verdicts = newer_versions(vec![
            VersionProbe {
                name: "a".into(),
                local: "1.9.0".into(),
                latest: "2.0.0".into(),
            },
            VersionProbe {
                name: "b".into(),
                local: "2.0.0".into(),
                latest: "2.0.0".into(),
            },
            // 本机装的是 canary，registry 上的稳定版更小 → 不能提示升级
            VersionProbe {
                name: "c".into(),
                local: "2.0.0-canary.1".into(),
                latest: "1.9.0".into(),
            },
            // 同号预发布：正式版更大，仍算可升级
            VersionProbe {
                name: "d".into(),
                local: "2.0.0-rc.1".into(),
                latest: "2.0.0".into(),
            },
        ]);
        let flags: Vec<(&str, bool)> = verdicts
            .iter()
            .map(|item| (item.name.as_str(), item.newer))
            .collect();
        assert_eq!(
            flags,
            vec![("a", true), ("b", false), ("c", false), ("d", true)]
        );
    }

    #[test]
    fn merge_marks_only_newer_remote_as_upgrade() {
        let mut statuses = vec![
            ManagerStatus {
                name: "npm".to_string(),
                available: true,
                path: Some("/usr/local/bin/npm".to_string()),
                version: Some("10.9.8".to_string()),
                latest: None,
                upgrade_available: false,
                self_updating: false,
                update: update_command("npm"),
                error: None,
            },
            ManagerStatus {
                name: "pnpm".to_string(),
                available: true,
                path: Some("/usr/local/bin/pnpm".to_string()),
                version: Some("12.6.0".to_string()),
                latest: None,
                upgrade_available: false,
                self_updating: false,
                update: update_command("npm"),
                error: None,
            },
            ManagerStatus {
                name: "brew".to_string(),
                available: true,
                path: Some("/opt/homebrew/bin/brew".to_string()),
                version: Some("7.0.4".to_string()),
                latest: None,
                upgrade_available: false,
                self_updating: true,
                update: update_command("brew"),
                error: None,
            },
        ];
        let latest = vec![
            ("npm".to_string(), Some("12.1.0".to_string())),
            // 本地比远端新：不报可升级
            ("pnpm".to_string(), Some("12.4.1".to_string())),
            ("brew".to_string(), Some("7.0.4".to_string())),
        ];
        merge_latest(&mut statuses, &latest);
        assert!(statuses[0].upgrade_available, "npm 有新版本");
        assert!(!statuses[1].upgrade_available, "本地更新时不该提示升级");
        assert!(!statuses[2].upgrade_available, "brew 自己更新，不做对比");
        assert_eq!(upgrade_candidates(&statuses), vec!["npm", "pnpm"]);
    }

    /// 字段名必须与 src/types.ts 一致
    #[test]
    fn manager_json_matches_frontend_contract() {
        let status = ManagerStatus {
            name: "npm".to_string(),
            available: true,
            path: Some("/usr/local/bin/npm".to_string()),
            version: Some("10.9.8".to_string()),
            latest: Some("12.1.0".to_string()),
            upgrade_available: true,
            self_updating: false,
            update: update_command("npm"),
            error: None,
        };
        let json = serde_json::to_value(&status).unwrap();
        for key in [
            "name",
            "available",
            "path",
            "version",
            "latest",
            "upgradeAvailable",
            "selfUpdating",
            "update",
            "error",
        ] {
            assert!(json.get(key).is_some(), "ManagerStatus 缺少 {key}");
        }
    }

    /// 更新命令是写死的确定命令，不做"尽力而为"的猜测：node 不给按钮
    #[test]
    fn update_commands_are_explicit() {
        let npm = update_command("npm").expect("npm 应有更新命令");
        assert_eq!(npm.program, "npm");
        assert_eq!(npm.args, vec!["install", "-g", "npm@latest"]);
        assert_eq!(npm.display, "npm install -g npm@latest");

        assert_eq!(update_command("pnpm").unwrap().display, "pnpm add -g pnpm@latest");
        assert_eq!(update_command("bun").unwrap().display, "bun upgrade");
        assert_eq!(update_command("deno").unwrap().display, "deno upgrade");
        assert_eq!(update_command("brew").unwrap().display, "brew update");
        assert!(update_command("node").is_none(), "node 由 nvm / Homebrew 等管理，不给更新按钮");

        let json = serde_json::to_value(npm).unwrap();
        for key in ["program", "args", "display"] {
            assert!(json.get(key).is_some(), "UpdateCommand 缺少 {key}");
        }
    }

    #[test]
    fn brew_is_probed_only_on_macos() {
        assert!(is_relevant("brew", "macos"));
        assert!(!is_relevant("brew", "linux"));
        assert!(!is_relevant("brew", "windows"));
        // 其余工具在哪个系统都探测
        for name in ["node", "npm", "pnpm", "bun", "deno"] {
            assert!(is_relevant(name, "linux"), "{name} 不该被平台过滤");
        }
    }

    /// 真机验证：`cargo test -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_manager_status() {
        let mut statuses = local_statuses();
        for item in &statuses {
            println!(
                "{:<5} 可用={} 版本={:?} 路径={:?} {:?}",
                item.name, item.available, item.version, item.path, item.error
            );
        }
        // 本机装了 npm 与 brew
        assert!(
            statuses
                .iter()
                .any(|item| item.name == "npm" && item.available && item.version.is_some()),
            "应能读出 npm 版本"
        );
        // brew 是自更新项，不参与版本对比
        merge_latest(
            &mut statuses,
            &[("brew".to_string(), Some("0.0.1".to_string()))],
        );
        let brew = statuses.iter().find(|item| item.name == "brew").unwrap();
        assert!(!brew.upgrade_available, "brew 不参与版本对比");
        assert_eq!(brew.latest.as_deref(), Some("0.0.1"), "latest 仍会记录");

        let started = Instant::now();
        let again = local_statuses();
        println!("二次探测耗时 {:?}（{} 项）", started.elapsed(), again.len());
        // macOS 上 node/npm/pnpm/bun/deno/brew 共 6 项
        assert_eq!(again.len(), 6);
    }
}
