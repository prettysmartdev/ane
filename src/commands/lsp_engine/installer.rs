use std::process::Command;

use anyhow::{bail, Result};

use crate::data::lsp::types::LspServerInfo;

pub fn is_installed(server: &LspServerInfo) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(server.check_command)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

pub fn install(server: &LspServerInfo) -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg(server.install_command)
        .status()?;

    if !status.success() {
        bail!(
            "failed to install {}: exit code {:?}",
            server.server_name,
            status.code()
        );
    }
    Ok(())
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
        assert!(install(&TRUE_SERVER).is_ok());
    }

    #[test]
    fn install_fails_when_command_exits_nonzero() {
        let err = install(&FAIL_SERVER).unwrap_err();
        assert!(err.to_string().contains("fail-server"));
    }
}
