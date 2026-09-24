//! 已安装全局包的采集。
//!
//! 五种来源各有各的输出格式，全部解析成统一的 `InstalledPackage`：
//! - npm / pnpm：JSON（注意两者即使有 ELSPROBLEMS 也会输出合法 JSON，不要看退出码）
//! - bun：只有文本树，得自己解析
//! - deno：没有列出全局包的命令，只能扫安装目录，因此没有版本信息
//! - brew：`brew info --json=v2 --installed` 一次拿全量
//!
//! 单个来源失败不影响其他来源，失败原因通过 `SourceError` 如实返回。

use std::collections::HashMap;
use std::process::Command;

use serde::Serialize;

use crate::brew::cask_app_path;
use crate::shell_env;

/// 仅测试里用于遍历来源；生产代码里来源名直接写在各自解析器中
#[cfg(test)]
pub const SOURCES: [&str; 5] = ["npm", "pnpm", "bun", "deno", "brew"];

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPackage {
    pub name: String,
    /// 无法确定版本时为空字符串（deno 的脚本没有版本元数据）
    pub version: String,
    pub source: String,
    pub description: Option<String>,
    pub homepage: Option<String>,
    /// brew 的 formula / cask，其他来源为 None
    pub kind: Option<String>,
    pub path: Option<String>,
    /// cask 安装出来的 .app 包路径（取自 cask artifacts 的 target），
    /// 用于「打开」与「在访达中显示」；非 cask 或只装字体/二进制的 cask 为 None
    pub app_path: Option<String>,
    pub outdated: bool,
    /// outdated 但版本号没变：Homebrew 的 formula 修订号更新，需要重建而不是升级版本
    pub revision_outdated: bool,
    pub latest: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceError {
    pub source: String,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledReport {
    pub packages: Vec<InstalledPackage>,
    pub errors: Vec<SourceError>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SizeEntry {
    pub path: String,
    pub bytes: u64,
}

fn run(program: &str, args: &[&str]) -> Result<String, String> {
    run_with_env(program, args, &[])
}

/// 带额外环境变量执行外部命令。
///
/// brew 一律要带 `HOMEBREW_NO_AUTO_UPDATE=1`：自动更新一天最多触发一次，
/// 但一旦触发就是几十秒白等（界面表现为"点了没反应"）。需要新索引时由用户显式触发。
pub fn run_with_env(
    program: &str,
    args: &[&str],
    extra_env: &[(&str, &str)],
) -> Result<String, String> {
    let executable = shell_env::resolve_program(program).ok_or_else(|| format!("未找到 {program}"))?;
    let mut command = Command::new(executable);
    command.args(args).env("PATH", shell_env::path_env());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|error| format!("执行 {program} 失败：{error}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        return Err(if detail.is_empty() {
            format!("{program} 没有输出")
        } else {
            detail.to_string()
        });
    }
    Ok(stdout)
}

fn home_dir() -> String {
    std::env::var("HOME").unwrap_or_default()
}

// ---------------------------------------------------------------- npm / pnpm

/// `npm ls -g --depth=0 --json` → `{ "dependencies": { name: { version } } }`
pub fn parse_npm(json: &str) -> Result<Vec<InstalledPackage>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("npm 输出不是合法 JSON：{error}"))?;
    let root = Paths::new();
    let dependencies = value.get("dependencies").and_then(|item| item.as_object());

    let mut packages = Vec::new();
    let Some(dependencies) = dependencies else {
        return Ok(packages);
    };
    for (name, info) in dependencies {
        packages.push(InstalledPackage {
            name: name.clone(),
            version: info
                .get("version")
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .to_string(),
            source: "npm".to_string(),
            description: None,
            homepage: None,
            kind: None,
            path: Some(root.npm.join(name).display().to_string()),
            app_path: None,
            outdated: false,
            revision_outdated: false,
            latest: None,
        });
    }
    Ok(packages)
}

/// `pnpm ls -g --depth=0 --json` → `[{ path, dependencies: { name: { version, path } } }]`
pub fn parse_pnpm(json: &str) -> Result<Vec<InstalledPackage>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("pnpm 输出不是合法 JSON：{error}"))?;

    let mut packages = Vec::new();
    let groups = value.as_array().cloned().unwrap_or_default();
    for group in groups {
        let Some(dependencies) = group.get("dependencies").and_then(|item| item.as_object()) else {
            continue;
        };
        for (name, info) in dependencies {
            // pnpm 会把全局目录自身列出来（如 @pnpm/exe 在 v11 目录里），跳过它自己
            if name == "@pnpm/exe" || name.starts_with("@pnpm/exe.") {
                continue;
            }
            packages.push(InstalledPackage {
                name: name.clone(),
                version: info
                    .get("version")
                    .and_then(|item| item.as_str())
                    .unwrap_or_default()
                    .to_string(),
                source: "pnpm".to_string(),
                description: None,
                homepage: None,
                kind: None,
                path: info
                    .get("path")
                    .and_then(|item| item.as_str())
                    .map(|value| value.to_string()),
                app_path: None,
                outdated: false,
                revision_outdated: false,
                latest: None,
            });
        }
    }
    Ok(packages)
}

// ---------------------------------------------------------------------- bun

/// `bun pm ls -g` 只有文本树：
/// ```text
/// /Users/you/.bun/install/global node_modules (2)
/// ├── opencode-ai@1.18.32
/// └── @scope/pkg@1.0.0
/// ```
pub fn parse_bun(text: &str) -> Vec<InstalledPackage> {
    let root = Paths::new().bun;
    let mut packages = Vec::new();

    for line in text.lines() {
        let Some(index) = line.find("── ") else {
            continue;
        };
        let entry = line[index + "── ".len()..].trim();
        if entry.is_empty() {
            continue;
        }
        // 从右往左找 @，兼容 @scope/name@version
        let (name, version) = match entry.rfind('@') {
            Some(position) if position > 0 => {
                (&entry[..position], &entry[position + 1..])
            }
            _ => (entry, ""),
        };
        packages.push(InstalledPackage {
            name: name.to_string(),
            version: version.to_string(),
            source: "bun".to_string(),
            description: None,
            homepage: None,
            kind: None,
            path: Some(root.join("node_modules").join(name).display().to_string()),
            app_path: None,
            outdated: false,
            revision_outdated: false,
            latest: None,
        });
    }
    packages
}

// --------------------------------------------------------------------- deno

/// deno 没有列出全局包的命令，只能扫安装目录里的可执行文件，因此拿不到版本。
pub fn scan_deno_bin() -> Vec<InstalledPackage> {
    let root = Paths::new().deno;
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };

    let mut packages = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        packages.push(InstalledPackage {
            name: name.to_string(),
            version: String::new(),
            source: "deno".to_string(),
            description: None,
            homepage: None,
            kind: None,
            path: Some(path.display().to_string()),
            app_path: None,
            outdated: false,
            revision_outdated: false,
            latest: None,
        });
    }
    packages
}

// --------------------------------------------------------------------- brew

/// `brew info --json=v2 --installed` → `{ formulae: [...], casks: [...] }`
pub fn parse_brew(json: &str) -> Result<Vec<InstalledPackage>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|error| format!("brew 输出不是合法 JSON：{error}"))?;
    let cellar = brew_prefix().map(|prefix| prefix.join("Cellar"));
    let mut packages = Vec::new();

    for item in value
        .get("formulae")
        .and_then(|node| node.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let Some(name) = item.get("name").and_then(|node| node.as_str()) else {
            continue;
        };
        let installed_version = item
            .get("installed")
            .and_then(|node| node.as_array())
            .and_then(|list| list.first())
            .and_then(|entry| entry.get("version"))
            .and_then(|node| node.as_str())
            .or_else(|| item.get("installed_version").and_then(|node| node.as_str()))
            .unwrap_or_default()
            .to_string();
        let outdated = item
            .get("outdated")
            .and_then(|node| node.as_bool())
            .unwrap_or(false);
        // brew 的 outdated 有两种含义：版本号变了，或只是 revision 变了（需要重建）。
        // llvm 就是后者的真实例子：versions.stable 与已安装版本都是 23.1.1，但 revision 已更新。
        let stable = item
            .get("versions")
            .and_then(|node| node.get("stable"))
            .and_then(|node| node.as_str())
            .unwrap_or_default()
            .to_string();
        let version_changed = !stable.is_empty() && stable != installed_version;

        packages.push(InstalledPackage {
            name: name.to_string(),
            version: installed_version,
            source: "brew".to_string(),
            description: item
                .get("desc")
                .and_then(|node| node.as_str())
                .map(str::to_string),
            homepage: item
                .get("homepage")
                .and_then(|node| node.as_str())
                .map(str::to_string),
            kind: Some("formula".to_string()),
            path: cellar
                .as_ref()
                .map(|base| base.join(name).display().to_string()),
            app_path: None,
            outdated,
            // 只有版本号真的不同才算"可升级"；否则不给出无意义的 x → x
            latest: (outdated && version_changed).then_some(stable),
            revision_outdated: outdated && !version_changed,
        });
    }

    for item in value
        .get("casks")
        .and_then(|node| node.as_array())
        .cloned()
        .unwrap_or_default()
    {
        let Some(token) = item.get("token").and_then(|node| node.as_str()) else {
            continue;
        };
        let app_path = cask_app_path(&item);
        let outdated = item
            .get("outdated")
            .and_then(|node| node.as_bool())
            .unwrap_or(false);
        let installed = item
            .get("installed")
            .and_then(|node| node.as_str())
            .unwrap_or_default()
            .to_string();
        let available = item
            .get("version")
            .and_then(|node| node.as_str())
            .map(str::to_string);
        let version_changed = available
            .as_deref()
            .is_some_and(|value| !value.is_empty() && value != installed);
        packages.push(InstalledPackage {
            name: token.to_string(),
            version: installed,
            source: "brew".to_string(),
            description: item
                .get("desc")
                .and_then(|node| node.as_str())
                .map(str::to_string),
            homepage: item
                .get("homepage")
                .and_then(|node| node.as_str())
                .map(str::to_string),
            kind: Some("cask".to_string()),
            path: app_path.clone(),
            app_path,
            outdated,
            latest: if outdated && version_changed { available } else { None },
            revision_outdated: outdated && !version_changed,
        });
    }

    Ok(packages)
}

fn brew_prefix() -> Option<std::path::PathBuf> {
    run_with_env("brew", &["--prefix"], crate::brew::NO_AUTO_UPDATE)
        .ok()
        .map(std::path::PathBuf::from)
}

// ------------------------------------------------------------------- 路径表

/// 各来源的安装根目录，用于推导包路径。
struct Paths {
    npm: std::path::PathBuf,
    bun: std::path::PathBuf,
    deno: std::path::PathBuf,
}

impl Paths {
    fn new() -> Self {
        let home = home_dir();
        let npm = run("npm", &["root", "-g"])
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(format!("{home}/.npm-global/lib/node_modules")));
        let bun = std::env::var("BUN_INSTALL")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(format!("{home}/.bun/install/global")));
        let deno = std::env::var("DENO_INSTALL_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(format!("{home}/.deno")))
            .join("bin");
        Self { npm, bun, deno }
    }
}

// -------------------------------------------------------------------- 采集

/// 并行采集全部来源；单个来源失败只记录错误，不影响其余来源。
///
/// `sources` 为 None 时采集全部。Node 页不需要 brew（`brew info` 要一两秒），
/// Homebrew 页也不需要 npm 系，所以允许只采集相关来源。
pub fn scan_all(sources: Option<Vec<String>>) -> InstalledReport {
    let wanted = |name: &str| {
        sources
            .as_ref()
            .is_none_or(|list| list.iter().any(|item| item == name))
    };
    let results: Vec<(&str, Result<Vec<InstalledPackage>, String>)> = std::thread::scope(|scope| {
        let npm = wanted("npm").then(|| scope.spawn(|| {
            run("npm", &["ls", "-g", "--depth=0", "--json"]).and_then(|text| parse_npm(&text))
        }));
        let pnpm = wanted("pnpm").then(|| scope.spawn(|| {
            run("pnpm", &["ls", "-g", "--depth=0", "--json"]).and_then(|text| parse_pnpm(&text))
        }));
        let bun =
            wanted("bun").then(|| scope.spawn(|| run("bun", &["pm", "ls", "-g"]).map(|text| parse_bun(&text))));
        let deno = wanted("deno").then(|| scope.spawn(|| {
            // deno 未安装时给出明确信号，而不是静默返回 0 个，
            // 否则界面上无法区分"没装 deno"和"装了但没有全局包"
            let root = Paths::new().deno;
            if shell_env::resolve_program("deno").is_none() && !root.exists() {
                return Err("未检测到 deno（未安装或不在 PATH 中）".to_string());
            }
            Ok(scan_deno_bin())
        }));
        let brew = wanted("brew").then(|| scope.spawn(|| {
            run_with_env(
                "brew",
                &["info", "--json=v2", "--installed"],
                crate::brew::NO_AUTO_UPDATE,
            )
            .and_then(|text| parse_brew(&text))
        }));

        let mut results = Vec::new();
        for (name, handle) in [
            ("npm", npm),
            ("pnpm", pnpm),
            ("bun", bun),
            ("deno", deno),
            ("brew", brew),
        ] {
            if let Some(handle) = handle {
                results.push((
                    name,
                    handle.join().unwrap_or_else(|_| Err("采集线程 panic".to_string())),
                ));
            }
        }
        results
    });

    let mut packages = Vec::new();
    let mut errors = Vec::new();
    for (source, result) in results {
        match result {
            Ok(mut found) => packages.append(&mut found),
            Err(message) => errors.push(SourceError {
                source: source.to_string(),
                message,
            }),
        }
    }
    packages.sort_by(|left, right| {
        left.source
            .cmp(&right.source)
            .then_with(|| left.name.cmp(&right.name))
    });

    InstalledReport { packages, errors }
}

/// `du -sk` 并行路数：本机 npm 的 46 个全局包串行要 7.2s，4 路降到 3.3s、8 路 2.8s
/// （瓶颈是文件数带来的 stat，不是带宽）。取 4 路，够快也不至于把磁盘打满。
const DU_LANES: usize = 4;

/// 用 `du -sk` 批量测量体积，分几路并行。
/// 路径不存在或权限不足时该项会被跳过，前端保持显示占位而不是显示错误的 0。
pub fn measure_sizes(paths: Vec<String>) -> Vec<SizeEntry> {
    if paths.is_empty() {
        return Vec::new();
    }
    let Some(executable) = shell_env::resolve_program("du") else {
        return Vec::new();
    };

    let lanes = DU_LANES.min(paths.len()).max(1);
    let per_lane = paths.len().div_ceil(lanes);
    let mut entries: Vec<SizeEntry> = std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(per_lane)
            .map(|chunk| {
                let executable = executable.clone();
                scope.spawn(move || measure_chunk(&executable, chunk))
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    });

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn measure_chunk(executable: &std::path::Path, paths: &[String]) -> Vec<SizeEntry> {
    let output = Command::new(executable)
        .arg("-sk")
        .args(paths)
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };

    let sizes: HashMap<String, u64> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (size, path) = line.split_once('\t')?;
            Some((path.to_string(), size.trim().parse::<u64>().ok()? * 1024))
        })
        .collect();

    paths
        .iter()
        .filter_map(|path| {
            sizes.get(path).map(|bytes| SizeEntry {
                path: path.clone(),
                bytes: *bytes,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_npm_json() {
        let json = r#"{
          "name": "lib",
          "dependencies": {
            "@nestjs/cli": { "version": "11.0.5" },
            "typescript": { "version": "6.0.3" }
          }
        }"#;
        let packages = parse_npm(json).expect("应能解析");
        assert_eq!(packages.len(), 2);
        let names: Vec<&str> = packages.iter().map(|item| item.name.as_str()).collect();
        assert!(names.contains(&"typescript"));
        let ts = packages.iter().find(|item| item.name == "typescript").unwrap();
        assert_eq!(ts.version, "6.0.3");
        assert_eq!(ts.source, "npm");
        assert!(ts.path.as_ref().unwrap().ends_with("node_modules/typescript"));
    }

    #[test]
    fn parses_pnpm_json_and_skips_internal() {
        let json = r#"[{
          "path": "/Users/you/Library/pnpm/global/v11",
          "private": true,
          "dependencies": {
            "@pnpm/exe": { "version": "11.25.0" },
            "typescript": { "version": "6.0.3", "path": "/Users/you/Library/pnpm/global/v11/node_modules/typescript" }
          }
        }]"#;
        let packages = parse_pnpm(json).expect("应能解析");
        assert_eq!(packages.len(), 1, "应跳过 pnpm 自身");
        assert_eq!(packages[0].name, "typescript");
        assert!(packages[0].path.as_ref().unwrap().contains("global/v11"));
    }

    #[test]
    fn parses_bun_tree_text() {
        let text = "/Users/you/.bun/install/global node_modules (13)\n└── opencode-ai@1.18.32\n└── @scope/tool@0.4.0\n";
        let packages = parse_bun(text);
        assert_eq!(packages.len(), 2);
        assert_eq!(packages[0].name, "opencode-ai");
        assert_eq!(packages[0].version, "1.18.32");
        // 作用域包要按最后一个 @ 切分
        assert_eq!(packages[1].name, "@scope/tool");
        assert_eq!(packages[1].version, "0.4.0");
    }

    #[test]
    fn parses_brew_json() {
        let json = r#"{
          "formulae": [{
            "name": "ripgrep",
            "desc": "Search tool like grep and The Silver Searcher",
            "homepage": "https://github.com/BurntSushi/ripgrep",
            "installed": [{ "version": "14.1.0" }],
            "outdated": true,
            "versions": { "stable": "15.0.0" }
          }, {
            "name": "llvm",
            "desc": "Next-gen compiler infrastructure",
            "installed": [{ "version": "23.1.1" }],
            "outdated": true,
            "revision": 1,
            "versions": { "stable": "23.1.1" }
          }],
          "casks": [{
            "token": "docker",
            "desc": "App to build and share containerized applications",
            "homepage": "https://www.docker.com/products/docker-desktop",
            "installed": "4.30.0",
            "outdated": false
          }]
        }"#;
        let packages = parse_brew(json).expect("应能解析");
        assert_eq!(packages.len(), 3);

        let formula = packages.iter().find(|item| item.name == "ripgrep").unwrap();
        assert_eq!(formula.version, "14.1.0");
        assert_eq!(formula.kind.as_deref(), Some("formula"));
        assert!(formula.outdated);
        assert_eq!(formula.latest.as_deref(), Some("15.0.0"));
        assert!(!formula.revision_outdated, "版本号变了就不算修订更新");
        assert!(formula.description.is_some());

        let cask = packages.iter().find(|item| item.name == "docker").unwrap();
        assert_eq!(cask.kind.as_deref(), Some("cask"));
        assert!(!cask.outdated);
        assert_eq!(cask.latest, None);
        assert!(!cask.revision_outdated);

        // 回归：brew 报 outdated 但版本号没变（formula 修订号更新），
        // 不能再给出 "23.1.1 → 23.1.1" 这种无意义的升级提示
        let rebuilt = packages.iter().find(|item| item.name == "llvm").unwrap();
        assert!(rebuilt.outdated, "brew 确实认为它过期");
        assert_eq!(rebuilt.version, "23.1.1");
        assert_eq!(rebuilt.latest, None, "版本号相同就不该有 latest");
        assert!(rebuilt.revision_outdated, "应标记为需要重建");
    }

    #[test]
    fn rejects_broken_json() {
        assert!(parse_npm("{ not json").is_err());
        assert!(parse_pnpm("nope").is_err());
        assert!(parse_brew("<html>").is_err());
    }

    /// 并行分片测量必须和串行结果一致（顺序无关、按路径取回）
    #[test]
    #[ignore]
    fn real_measure_sizes_matches_across_lanes() {
        let paths: Vec<String> = (0..8)
            .map(|_| std::env::temp_dir().display().to_string())
            .collect();
        let many = measure_sizes(paths.clone());
        assert!(!many.is_empty(), "临时目录应能测出体积");
        // 同一个路径重复出现时，每片各自解析，结果条数等于入参条数
        assert_eq!(many.len(), paths.len());
        assert!(many.windows(2).all(|pair| pair[0].bytes == pair[1].bytes));

        let single = measure_sizes(vec![std::env::temp_dir().display().to_string()]);
        assert_eq!(single.len(), 1);
        assert_eq!(single[0].bytes, many[0].bytes, "单路与多路结果应一致");
    }

    /// 真机验证：`cargo test -- --ignored --nocapture` 会实际调用各包管理器。
    #[test]
    #[ignore]
    fn real_scan_respects_source_filter() {
        // Homebrew 页只请求 brew：不该顺带跑 npm 系，也不该出现 deno 的采集错误
        let report = scan_all(Some(vec!["brew".to_string()]));
        assert!(
            report.packages.iter().all(|item| item.source == "brew"),
            "只请求 brew 时不该混入其它来源"
        );
        assert!(
            report.errors.iter().all(|error| error.source != "deno"),
            "不该为没请求的来源报错"
        );
        println!("只采 brew：{} 个包，{} 个错误", report.packages.len(), report.errors.len());

        // Node 页只请求 npm 系：不该跑 brew（那条命令要一两秒）
        let node = scan_all(Some(vec![
            "npm".to_string(),
            "pnpm".to_string(),
            "bun".to_string(),
            "deno".to_string(),
        ]));
        assert!(
            node.packages.iter().all(|item| item.source != "brew"),
            "Node 页不该出现 brew 包"
        );
        println!("只采 Node 系：{} 个包", node.packages.len());
    }

    #[test]
    #[ignore]
    fn real_scan_reports_counts() {
        let report = scan_all(None);
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for package in &report.packages {
            *counts.entry(package.source.as_str()).or_default() += 1;
        }
        for source in SOURCES {
            println!("{source}: {} 个", counts.get(source).copied().unwrap_or(0));
        }
        for error in &report.errors {
            println!("{} 采集失败：{}", error.source, error.message);
        }
        // cask 的 App 路径：本机 cc-switch 有，纯字体/安装器类 cask 没有
        let casks: Vec<&InstalledPackage> = report
            .packages
            .iter()
            .filter(|item| item.kind.as_deref() == Some("cask"))
            .collect();
        for cask in &casks {
            println!("cask {:<20} App 路径 {:?}", cask.name, cask.app_path);
        }
        if let Some(cc) = casks.iter().find(|item| item.name == "cc-switch") {
            assert_eq!(cc.app_path.as_deref(), Some("/Applications/CC Switch.app"));
        }
        assert!(
            casks.iter().any(|item| item.app_path.is_none()),
            "字体或安装器类 cask 不应有 App 路径"
        );
        // 过期项要能区分"版本升级"与"仅修订更新"，否则界面会出现 x → x
        for item in report.packages.iter().filter(|item| item.outdated) {
            let kind = match &item.latest {
                Some(latest) => format!("升级 {latest}"),
                None if item.revision_outdated => "仅修订更新（版本号未变，需重建）".to_string(),
                None => "原因未知".to_string(),
            };
            println!("过期：{} {} {} → {kind}", item.source, item.name, item.version);
        }
        if let Some(sample) = report.packages.iter().find(|item| item.source == "brew") {
            println!("brew 样例：{} {} —— {:?}", sample.name, sample.version, sample.description);
        }
        assert!(
            counts.get("npm").copied().unwrap_or(0) > 0,
            "本机应能采到 npm 全局包"
        );
        assert!(
            counts.get("brew").copied().unwrap_or(0) > 0,
            "本机应能采到 brew 包"
        );
    }

    #[test]
    #[ignore]
    fn real_sizes_measured() {
        let report = scan_all(None);
        let paths: Vec<String> = report
            .packages
            .iter()
            .filter(|item| item.source == "brew")
            .take(3)
            .filter_map(|item| item.path.clone())
            .collect();
        let sizes = measure_sizes(paths.clone());
        for entry in &sizes {
            println!("{} = {} KB", entry.path, entry.bytes / 1024);
        }
        assert!(!paths.is_empty(), "应至少有三个 brew 包路径");
        assert!(!sizes.is_empty(), "du 应至少测出一个体积");
    }
}
