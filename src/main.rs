use anyhow::Result;

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
        None => {
            tui::app::run(&args.path)?;
        }
    }

    Ok(())
}

fn run_exec(chord_str: &str, path: &std::path::Path) -> Result<()> {
    let parsed = chord::parse_chord(chord_str)?;
    let result = chord::execute_chord(path, &parsed)?;

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
