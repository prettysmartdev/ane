use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "ane", version, about = "Agent Native Editor")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// File or directory to open in the TUI
    #[arg(default_value = ".")]
    pub path: PathBuf,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Execute a chord on a file or directory (no TUI, outputs diff)
    Exec {
        /// The chord to execute (e.g. "cala 5 new text")
        #[arg(short, long)]
        chord: String,

        /// Target file path
        #[arg()]
        path: PathBuf,
    },
    /// Initialize ane skill for a code agent (writes skill file to agent's directory)
    Init {
        /// Agent name (claude, codex, gemini, opencode, cline, maki, charm)
        #[arg()]
        agent: String,
    },
}

pub fn parse() -> Cli {
    Cli::parse()
}
