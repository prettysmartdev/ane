use anyhow::Result;

use ane::commands::lsp_engine::{LspEngine, LspEngineConfig};
use ane::commands::{chord, diff};
use ane::frontend::cli::{self, Command};
use ane::frontend::tui;

fn main() -> Result<()> {
    let args = cli::parse();

    match args.command {
        Some(Command::Exec {
            chord: chord_str,
            path,
        }) => {
            if let Err(e) = run_exec(&chord_str, &path) {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        Some(Command::Init { agent }) => {
            if let Err(e) = run_init(&agent) {
                eprintln!("error: {e:#}");
                std::process::exit(1);
            }
        }
        None => {
            tui::app::run(&args.path)?;
        }
    }

    Ok(())
}

fn run_init(agent_name: &str) -> Result<()> {
    let config = ane::data::init::init_agent(agent_name, std::path::Path::new("."))?;
    let skill_path = std::path::Path::new(config.skill_dir).join(config.skill_filename);
    println!("wrote ane skill to {}", skill_path.display());
    if let Some(note) = config.manual_note {
        println!("note: {note}");
    }
    Ok(())
}

fn run_exec(chord_str: &str, path: &std::path::Path) -> Result<()> {
    let parsed = chord::parse_chord(chord_str)?;
    let mut lsp = LspEngine::new(LspEngineConfig::default());
    lsp.set_install_progress(ane::frontend::cli_frontend::cli_install_progress());
    let cli = ane::frontend::cli_frontend::CliFrontend::new();
    let result = chord::execute_chord(&cli, path, &parsed, &mut lsp)?;

    for w in &result.warnings {
        eprintln!("warning: {w}");
    }

    if let Some(ref yanked) = result.yanked {
        println!("{yanked}");
    } else {
        let path_str = path.display().to_string();
        let diff_output = diff::unified_diff(&path_str, &result.original, &result.modified);
        if !diff_output.is_empty() {
            print!("{diff_output}");
        }
    }

    Ok(())
}
