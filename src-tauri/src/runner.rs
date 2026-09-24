//! 子进程执行：流式输出 + 取消。
//!
//! 阶段 0 的核心验证点。设计取舍：
//! - 用 `std::process::Command` 自己管，而不是 `tauri-plugin-shell`：需要拿到进程组、
//!   自己控制取消与输出切分，插件层的封装反而挡路。
//! - 每个子进程放进独立进程组（`process_group(0)`），取消时对整组发信号。
//!   npm/pnpm 会再派生 node 子进程，只杀父进程会留下孤儿。
//! - 输出按 `\n` 和 `\r` 双分隔切分，这样 npm、brew 的进度条也能实时刷出来。

use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use crate::shell_env;

pub const PROC_EVENT: &str = "proc://event";

/// 单行日志上限，防止某个子进程疯狂刷屏把前端打挂
const MAX_LINE_BYTES: usize = 8192;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProcEvent {
    #[serde(rename_all = "camelCase")]
    Started {
        id: String,
        program: String,
        executable: String,
        args: Vec<String>,
        pid: u32,
    },
    #[serde(rename_all = "camelCase")]
    Output {
        id: String,
        stream: String,
        line: String,
    },
    #[serde(rename_all = "camelCase")]
    Exit {
        id: String,
        code: Option<i32>,
        killed: bool,
        elapsed_ms: u64,
    },
}

pub type Emitter = Arc<dyn Fn(ProcEvent) + Send + Sync>;

/// 子进程退出后回调，参数是退出码（被信号终止时为 None）。
pub type ExitHook = Box<dyn FnOnce(Option<i32>) + Send + 'static>;

struct Entry {
    child: Arc<Mutex<Child>>,
    killed: Arc<AtomicBool>,
    started: Instant,
}

pub struct Runner {
    entries: Mutex<HashMap<String, Entry>>,
}

static RUNNER: OnceLock<Runner> = OnceLock::new();

pub fn runner() -> &'static Runner {
    RUNNER.get_or_init(|| Runner {
        entries: Mutex::new(HashMap::new()),
    })
}

impl Runner {
    /// 启动子进程并立即返回，输出与退出状态通过 `emit` 回调推送。
    pub fn spawn(
        &self,
        id: String,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        envs: Vec<(String, String)>,
        emit: Emitter,
    ) -> Result<u32, String> {
        self.spawn_with_hook(id, program, args, cwd, envs, emit, None)
    }

    /// 同上，但可以额外注入环境变量，并在进程退出后执行一次回调（用于释放并发锁）。
    ///
    /// 环境变量由调用方决定：安装类命令要把设置里的数据源与代理显式传给
    /// 包管理器，否则从访达启动的 GUI 会让"查到的是 A 源、装的是 B 源"。
    pub fn spawn_with_hook(
        &self,
        id: String,
        program: String,
        args: Vec<String>,
        cwd: Option<String>,
        envs: Vec<(String, String)>,
        emit: Emitter,
        on_exit: Option<ExitHook>,
    ) -> Result<u32, String> {
        let executable = shell_env::resolve_program(&program)
            .ok_or_else(|| format!("未找到可执行文件：{program}（已在解析出的 PATH 中查找）"))?;

        let mut command = Command::new(&executable);
        command
            .args(&args)
            .env("PATH", shell_env::path_env())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if let Ok(home) = std::env::var("HOME") {
            command.env("HOME", home);
        }
        for (key, value) in &envs {
            command.env(key, value);
        }
        if let Some(dir) = &cwd {
            command.current_dir(dir);
        }

        let mut child = command
            .spawn()
            .map_err(|error| format!("启动子进程失败：{error}"))?;
        let pid = child.id();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        emit(ProcEvent::Started {
            id: id.clone(),
            program,
            executable: executable.display().to_string(),
            args,
            pid,
        });

        if let Some(out) = stdout {
            spawn_reader(out, "stdout", id.clone(), emit.clone());
        }
        if let Some(err) = stderr {
            spawn_reader(err, "stderr", id.clone(), emit.clone());
        }

        let child = Arc::new(Mutex::new(child));
        let killed = Arc::new(AtomicBool::new(false));
        self.entries.lock().unwrap().insert(
            id.clone(),
            Entry {
                child: child.clone(),
                killed: killed.clone(),
                started: Instant::now(),
            },
        );

        let wait_id = id;
        thread::spawn(move || {
            let status = loop {
                // 不长期持锁，否则取消时拿不到 child 去发信号
                let result = { child.lock().unwrap().try_wait() };
                match result {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) => thread::sleep(Duration::from_millis(80)),
                    Err(_) => break None,
                }
            };
            let elapsed_ms = runner()
                .entries
                .lock()
                .unwrap()
                .remove(&wait_id)
                .map(|entry| entry.started.elapsed().as_millis() as u64)
                .unwrap_or_default();
            let code = status.and_then(|status| status.code());
            if let Some(hook) = on_exit {
                hook(code);
            }
            emit(ProcEvent::Exit {
                id: wait_id,
                code,
                killed: killed.load(Ordering::SeqCst),
                elapsed_ms,
            });
        });

        Ok(pid)
    }

    /// 对进程组先发 SIGTERM，宽限期后仍在则补 SIGKILL。
    pub fn kill(&self, id: &str) -> Result<bool, String> {
        let (pid, killed) = {
            let entries = self.entries.lock().unwrap();
            match entries.get(id) {
                Some(entry) => (
                    entry.child.lock().unwrap().id(),
                    entry.killed.clone(),
                ),
                None => return Ok(false),
            }
        };
        killed.store(true, Ordering::SeqCst);
        // 负数 pid 表示整个进程组
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }

        let wait_id = id.to_string();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(2500));
            if runner().entries.lock().unwrap().contains_key(&wait_id) {
                unsafe {
                    libc::kill(-(pid as i32), libc::SIGKILL);
                }
            }
        });
        Ok(true)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.entries.lock().unwrap().contains_key(id)
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: &'static str,
    id: String,
    emit: Emitter,
) {
    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        let mut pending = String::new();
        let flush = |pending: &mut String| {
            let line = pending.trim_end().to_string();
            pending.clear();
            if !line.is_empty() {
                emit(ProcEvent::Output {
                    id: id.clone(),
                    stream: stream.to_string(),
                    line,
                });
            }
        };

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    for ch in String::from_utf8_lossy(&buffer[..read]).chars() {
                        if ch == '\n' || ch == '\r' {
                            flush(&mut pending);
                        } else if pending.len() < MAX_LINE_BYTES {
                            pending.push(ch);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        flush(&mut pending);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    fn collector() -> (Emitter, Arc<StdMutex<Vec<ProcEvent>>>) {
        let events: Arc<StdMutex<Vec<ProcEvent>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink = events.clone();
        let emit: Emitter = Arc::new(move |event| sink.lock().unwrap().push(event));
        (emit, events)
    }

    /// 等待退出事件。注意不能用 find_map 取 code：进程被信号杀死时 code 本身就是
    /// None，外层再包一层 Option 会把"已退出"误判成"还没退出"。
    fn wait_for_exit(
        events: &Arc<StdMutex<Vec<ProcEvent>>>,
        timeout: Duration,
    ) -> Option<ProcEvent> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let found = events
                .lock()
                .unwrap()
                .iter()
                .find(|event| matches!(event, ProcEvent::Exit { .. }))
                .cloned();
            if found.is_some() {
                return found;
            }
            thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn exit_parts(event: ProcEvent) -> (Option<i32>, bool) {
        match event {
            ProcEvent::Exit { code, killed, .. } => (code, killed),
            other => panic!("期望退出事件，实际：{other:?}"),
        }
    }

    #[test]
    fn streams_stdout_and_stderr() {
        let (emit, events) = collector();
        runner()
            .spawn(
                "test-stream".into(),
                "sh".into(),
                vec![
                    "-c".into(),
                    "echo 第一行; echo 第二行 1>&2".into(),
                ],
                None,
                Vec::new(),
                emit,
            )
            .expect("应能启动 sh");

        let exit = wait_for_exit(&events, Duration::from_secs(10)).expect("应收到退出事件");
        let (code, _) = exit_parts(exit);
        assert_eq!(code, Some(0));

        let events = events.lock().unwrap();
        let lines: Vec<(String, String)> = events
            .iter()
            .filter_map(|event| match event {
                ProcEvent::Output { stream, line, .. } => {
                    Some((stream.clone(), line.clone()))
                }
                _ => None,
            })
            .collect();
        assert!(
            lines.iter().any(|(stream, line)| stream == "stdout" && line == "第一行"),
            "应捕获 stdout，实际：{lines:?}"
        );
        assert!(
            lines.iter().any(|(stream, line)| stream == "stderr" && line.contains("第二行")),
            "应捕获 stderr 并区分流，实际：{lines:?}"
        );
    }

    /// 环境变量要真的传进子进程：安装命令靠这条链路把数据源与代理带下去。
    /// 用 `sh` 验证，不调用任何真实包管理器。
    #[test]
    fn passes_environment_to_child() {
        let (emit, events) = collector();
        runner()
            .spawn(
                "test-env".into(),
                "sh".into(),
                vec!["-c".into(), "echo \"$NPM_CONFIG_REGISTRY|$HTTPS_PROXY\"".into()],
                None,
                vec![
                    ("NPM_CONFIG_REGISTRY".into(), "https://registry.example".into()),
                    ("HTTPS_PROXY".into(), "http://127.0.0.1:7890".into()),
                ],
                emit,
            )
            .expect("应能启动 sh");

        let exit = wait_for_exit(&events, Duration::from_secs(10)).expect("应收到退出事件");
        let (code, _) = exit_parts(exit);
        assert_eq!(code, Some(0));

        let lines: Vec<String> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                ProcEvent::Output { line, .. } => Some(line.clone()),
                _ => None,
            })
            .collect();
        assert!(
            lines
                .iter()
                .any(|line| line == "https://registry.example|http://127.0.0.1:7890"),
            "子进程应读到注入的变量，实际输出：{lines:?}"
        );
    }

    #[test]
    fn reports_missing_program() {
        let (emit, _events) = collector();
        let result = runner().spawn(
            "test-missing".into(),
            "definitely-not-a-real-binary-xyz".into(),
            vec![],
            None,
            Vec::new(),
            emit,
        );
        assert!(result.is_err(), "不存在的程序应返回错误");
    }

    /// 前端按 camelCase 读取字段，这里把序列化后的键名钉死，
    /// 避免改了 Rust 结构体字段却悄悄让界面读不到值。
    #[test]
    fn event_json_matches_frontend_contract() {
        let started = serde_json::to_string(&ProcEvent::Started {
            id: "p1".into(),
            program: "npm".into(),
            executable: "/usr/local/bin/npm".into(),
            args: vec!["--version".into()],
            pid: 42,
        })
        .unwrap();
        assert!(started.contains(r#""kind":"started""#), "实际：{started}");
        assert!(started.contains(r#""executable""#), "实际：{started}");

        let output = serde_json::to_string(&ProcEvent::Output {
            id: "p1".into(),
            stream: "stdout".into(),
            line: "hello".into(),
        })
        .unwrap();
        assert!(output.contains(r#""kind":"output""#), "实际：{output}");

        let exit = serde_json::to_string(&ProcEvent::Exit {
            id: "p1".into(),
            code: None,
            killed: true,
            elapsed_ms: 1234,
        })
        .unwrap();
        assert!(exit.contains(r#""kind":"exit""#), "实际：{exit}");
        assert!(exit.contains(r#""elapsedMs":1234"#), "实际：{exit}");
        assert!(exit.contains(r#""code":null"#), "实际：{exit}");
    }

    #[test]
    fn kill_terminates_process_group() {
        let (emit, events) = collector();
        // 子 shell 里再 sleep，验证整组被杀，而不只是父进程
        runner()
            .spawn(
                "test-kill".into(),
                "sh".into(),
                vec!["-c".into(), "while true; do echo tick; sleep 0.2; done".into()],
                None,
                Vec::new(),
                emit,
            )
            .expect("应能启动 sh");

        thread::sleep(Duration::from_millis(400));
        assert!(runner().kill("test-kill").unwrap(), "应能找到并取消该进程");

        let exit = wait_for_exit(&events, Duration::from_secs(15)).expect("取消后仍应收到退出事件");
        let (_, killed) = exit_parts(exit);
        assert!(killed, "退出事件应标记 killed");
        assert!(!runner().is_running("test-kill"), "进程应从注册表移除");
    }
}
