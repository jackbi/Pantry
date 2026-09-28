//! 外链地址的归一化。
//!
//! npm 生态的 `homepage` / `repository` 与 Homebrew 的 `urls.head` 写法五花八门：
//! `git+https://…`、`{ "type": "git", "url": … }`、`github:user/repo`、
//! `git@github.com:user/repo.git`。界面上这些行是要做成可点击外链的，因此统一在这里
//! 收敛成能直接点开的 http(s) 地址——转不成的一律返回 None，宁可那一行不显示，
//! 也不要给一个点了没反应的地址。

/// 常见代码托管站：地址落在这些站上时可以判定为"仓库"而不是某个文件下载页。
const VCS_HOSTS: [&str; 6] = [
    "github.com",
    "gitlab.com",
    "bitbucket.org",
    "codeberg.org",
    "sr.ht",
    "sourcehut.org",
];

/// 把 repository / homepage 的各种写法收敛成能直接点开的 http(s) 地址。
/// 转不成（相对路径、`file:`、纯文本）就返回 None。
pub fn web_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    for (prefix, host) in [
        ("github:", "github.com"),
        ("gitlab:", "gitlab.com"),
        ("bitbucket:", "bitbucket.org"),
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(strip_git_suffix(format!(
                "https://{host}/{}",
                rest.trim_matches('/')
            )));
        }
    }
    // git@github.com:user/repo.git 这种 SSH 写法在浏览器里同样点不开
    if let Some(rest) = trimmed.strip_prefix("git@") {
        if let Some((host, path)) = rest.split_once(':') {
            return Some(strip_git_suffix(format!(
                "https://{host}/{}",
                path.trim_start_matches('/')
            )));
        }
    }
    let without_prefix = trimmed.strip_prefix("git+").unwrap_or(trimmed);
    if without_prefix.starts_with("http://") || without_prefix.starts_with("https://") {
        return Some(strip_git_suffix(without_prefix.to_string()));
    }
    None
}

/// 去掉末尾的 `/` 与 `.git`：`https://github.com/a/b.git` → `https://github.com/a/b`。
pub fn strip_git_suffix(url: String) -> String {
    url.trim_end_matches('/').trim_end_matches(".git").to_string()
}

/// 地址是否落在常见代码托管站上。
pub fn is_vcs_host(url: &str) -> bool {
    VCS_HOSTS
        .iter()
        .any(|host| url.starts_with(&format!("https://{host}/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_repository_forms() {
        let cases = [
            ("git+https://github.com/a/b.git", "https://github.com/a/b"),
            ("https://github.com/a/b.git", "https://github.com/a/b"),
            ("https://github.com/a/b/", "https://github.com/a/b"),
            ("github:a/b", "https://github.com/a/b"),
            ("gitlab:a/b", "https://gitlab.com/a/b"),
            ("git@github.com:a/b.git", "https://github.com/a/b"),
            ("https://vite.dev/", "https://vite.dev"),
        ];
        for (raw, expected) in cases {
            assert_eq!(web_url(raw).as_deref(), Some(expected), "{raw}");
        }
    }

    #[test]
    fn refuses_addresses_that_cannot_be_opened() {
        for raw in ["", "   ", "../local", "file:///tmp/x", "见 README", "npm:foo"] {
            assert_eq!(web_url(raw), None, "{raw}");
        }
    }

    #[test]
    fn detects_vcs_hosts() {
        assert!(is_vcs_host("https://github.com/a/b"));
        assert!(is_vcs_host("https://codeberg.org/a/b"));
        // 前缀像但域名不同的不算：那种是钓鱼式域名
        assert!(!is_vcs_host("https://github.com.evil.com/a/b"));
        assert!(!is_vcs_host("https://example.com/a/b"));
    }
}
