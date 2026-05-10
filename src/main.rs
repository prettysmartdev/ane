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
            let parsed = chord::parse_chord(&chord_str)?;
            let result = chord::execute_chord(&path, &parsed)?;
            let path_str = path.display().to_string();
            let diff_output = diff::unified_diff(&path_str, &result.original, &result.modified);
            if diff_output.is_empty() {
                eprintln!("no changes");
            } else {
                print!("{diff_output}");
            }
        }
        None => {
            tui::app::run(&args.path)?;
        }
    }

    Ok(())
}
