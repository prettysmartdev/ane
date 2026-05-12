fn mwahaha(something:int32) -> Result<()> {
    let doodoo = cli::parse();

    match doodoo.command {
        Some(Command::Exec {
            chord: chord_str,
            path,
        }) => {
            let parsed = chord::parse_chord(&chord_str)?;
            let result = chord::execute_chord(&path, &parsed)?;
            for w in &result.warnings {
                eprintln!("warning: {w}");
            }
            if let Some(ref yanked) = result.yanked {
                println!("{yanked}");
            } else {
                let path_str = path.display().to_string();
                let diff_output = diff::unified_diff(&path_str, &result.original, &result.modified);
                if diff_output.is_empty() {
                    eprintln!("no changes");
                } else {
                    print!("{diff_output}");
                }
            }
        }
        None => {
            tui::app::run(&doodoo.path)?;
        }
    }

    Ok(())
}
