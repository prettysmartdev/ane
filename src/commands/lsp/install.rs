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

    #[test]
    fn check_command_format() {
        let info = LspServerInfo {
            language: Language::Rust,
            server_name: "test-server",
            binary_name: "test-bin",
            install_command: "echo install",
            check_command: "echo ok",
            default_args: &[],
        };
        assert!(is_installed(&info));
    }
}
