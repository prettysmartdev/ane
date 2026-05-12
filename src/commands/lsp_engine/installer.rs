use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::Arc;

use anyhow::{bail, Result};

use crate::data::lsp::types::LspServerInfo;

pub trait InstallProgress: Send + Sync {
    fn on_stdout(&self, line: &str);
    fn on_stderr(&self, line: &str);
    fn on_failed(&self, message: &str);
    fn on_complete(&self);
}

pub fn is_installed(server: &LspServerInfo) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(server.check_command)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

pub fn install(server: &LspServerInfo, progress: Option<&Arc<dyn InstallProgress>>) -> Result<()> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(server.install_command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    let prog_out = progress.cloned();
    let prog_err = progress.cloned();

    let out_handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(ref p) = prog_out {
                p.on_stdout(&line);
            }
            lines.push(line);
        }
        lines
    });

    let err_handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            if let Some(ref p) = prog_err {
                p.on_stderr(&line);
            }
            lines.push(line);
        }
        lines
    });

    let status = child.wait()?;
    let stdout_lines = out_handle.join().unwrap_or_default();
    let stderr_lines = err_handle.join().unwrap_or_default();

    if !status.success() {
        let log_path = write_install_log(server.server_name, &stdout_lines, &stderr_lines);
        let msg = match log_path {
            Some(p) => format!(
                "install of {} failed (exit {}); log: {}",
                server.server_name,
                status.code().unwrap_or(-1),
                p,
            ),
            None => format!(
                "install of {} failed (exit {})",
                server.server_name,
                status.code().unwrap_or(-1),
            ),
        };
        if let Some(p) = progress {
            p.on_failed(&msg);
        }
        bail!("{msg}");
    }

    if let Some(p) = progress {
        p.on_complete();
    }

    Ok(())
}

fn write_install_log(
    server_name: &str,
    stdout_lines: &[String],
    stderr_lines: &[String],
) -> Option<String> {
    use std::io::Write;
    let path = std::env::temp_dir().join(format!("ane-install-{server_name}.log"));
    let mut f = std::fs::File::create(&path).ok()?;
    if !stdout_lines.is_empty() {
        let _ = writeln!(f, "=== stdout ===");
        for line in stdout_lines {
            let _ = writeln!(f, "{line}");
        }
    }
    if !stderr_lines.is_empty() {
        let _ = writeln!(f, "=== stderr ===");
        for line in stderr_lines {
            let _ = writeln!(f, "{line}");
        }
    }
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::lsp::types::Language;

    static MISSING_SERVER: LspServerInfo = LspServerInfo {
        language: Language::Rust,
        server_name: "missing-server",
        binary_name: "ane-test-binary-does-not-exist-xyz",
        install_command: "exit 0",
        check_command: "ane-test-binary-does-not-exist-xyz --version",
        default_args: &[],
        init_options_json: "",
    };

    static TRUE_SERVER: LspServerInfo = LspServerInfo {
        language: Language::Rust,
        server_name: "true-server",
        binary_name: "true",
        install_command: "true",
        check_command: "true",
        default_args: &[],
        init_options_json: "",
    };

    static FAIL_SERVER: LspServerInfo = LspServerInfo {
        language: Language::Rust,
        server_name: "fail-server",
        binary_name: "false",
        install_command: "false",
        check_command: "false",
        default_args: &[],
        init_options_json: "",
    };

    #[test]
    fn not_installed_when_binary_missing() {
        assert!(!is_installed(&MISSING_SERVER));
    }

    #[test]
    fn installed_when_check_command_succeeds() {
        assert!(is_installed(&TRUE_SERVER));
    }

    #[test]
    fn not_installed_when_check_command_fails() {
        assert!(!is_installed(&FAIL_SERVER));
    }

    #[test]
    fn install_succeeds_when_command_exits_zero() {
        assert!(install(&TRUE_SERVER, None).is_ok());
    }

    #[test]
    fn install_fails_when_command_exits_nonzero() {
        let err = install(&FAIL_SERVER, None).unwrap_err();
        assert!(err.to_string().contains("fail-server"));
    }

    use std::sync::Mutex;

    struct TestProgress {
        lines: Mutex<Vec<String>>,
        failed: Mutex<Option<String>>,
        completed: Mutex<bool>,
    }

    impl TestProgress {
        fn new() -> Self {
            Self {
                lines: Mutex::new(Vec::new()),
                failed: Mutex::new(None),
                completed: Mutex::new(false),
            }
        }
    }

    impl InstallProgress for TestProgress {
        fn on_stdout(&self, line: &str) {
            self.lines.lock().unwrap().push(format!("out: {line}"));
        }
        fn on_stderr(&self, line: &str) {
            self.lines.lock().unwrap().push(format!("err: {line}"));
        }
        fn on_failed(&self, message: &str) {
            *self.failed.lock().unwrap() = Some(message.to_string());
        }
        fn on_complete(&self) {
            *self.completed.lock().unwrap() = true;
        }
    }

    #[test]
    fn install_streams_lines_via_progress_trait() {
        static ECHO_SERVER: LspServerInfo = LspServerInfo {
            language: Language::Rust,
            server_name: "echo-server",
            binary_name: "true",
            install_command: "echo hello && echo world",
            check_command: "true",
            default_args: &[],
            init_options_json: "",
        };
        let prog = Arc::new(TestProgress::new());
        let dyn_prog: Arc<dyn InstallProgress> = Arc::clone(&prog) as Arc<dyn InstallProgress>;
        install(&ECHO_SERVER, Some(&dyn_prog)).unwrap();
        let lines = prog.lines.lock().unwrap();
        assert!(lines.contains(&"out: hello".to_string()));
        assert!(lines.contains(&"out: world".to_string()));
        assert!(*prog.completed.lock().unwrap());
    }

    #[test]
    fn install_failure_calls_on_failed_with_log_path() {
        static NOISY_FAIL: LspServerInfo = LspServerInfo {
            language: Language::Rust,
            server_name: "noisy-fail",
            binary_name: "false",
            install_command: "echo out-line && echo err-line >&2 && exit 1",
            check_command: "false",
            default_args: &[],
            init_options_json: "",
        };
        let prog = Arc::new(TestProgress::new());
        let dyn_prog: Arc<dyn InstallProgress> = Arc::clone(&prog) as Arc<dyn InstallProgress>;
        let err = install(&NOISY_FAIL, Some(&dyn_prog)).unwrap_err();
        assert!(err.to_string().contains("noisy-fail"));
        let failed = prog.failed.lock().unwrap();
        let msg = failed.as_ref().unwrap();
        assert!(msg.contains("log:"));
        assert!(msg.contains("noisy-fail"));
    }
}
