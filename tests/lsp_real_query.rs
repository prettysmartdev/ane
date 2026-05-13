use std::io::Write;
use std::path::Path;

use ane::commands::lsp_engine::{LspEngine, LspEngineConfig};

fn temp_rs_file(content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::Builder::new().suffix(".rs").tempfile().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

fn wait_for_lsp(lsp: &mut LspEngine, path: &Path) {
    let timeout = std::time::Duration::from_secs(15);
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            panic!("LSP startup timeout");
        }
        match lsp.document_symbols(path) {
            Ok(_) => return,
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(500)),
        }
    }
}

#[test]
fn dump_lsp_data_for_variable() {
    let f = temp_rs_file("fn main() {\n    let cmon = hello();\n}\n");
    let path = f.path();
    let config = LspEngineConfig::default();
    let mut lsp = LspEngine::new(config);
    let files: Vec<&Path> = vec![path];
    if lsp
        .start_for_context(path.parent().unwrap(), &files)
        .is_err()
    {
        eprintln!("Could not start LSP");
        return;
    }
    wait_for_lsp(&mut lsp, path);

    // documentSymbol
    match lsp.document_symbols(path) {
        Ok(syms) => {
            eprintln!("\n=== documentSymbol ===");
            print_syms(&syms, 0);
        }
        Err(e) => eprintln!("documentSymbol error: {e}"),
    }

    // selectionRange at every column on line 1 (0-indexed)
    // "    let cmon = hello();"
    //  0123456789...
    eprintln!("\n=== selectionRange at each column on line 1 ===");
    for col in 0..23 {
        match lsp.selection_range(path, 1, col) {
            Ok(sel) => {
                eprint!("col {col:2}: ");
                print_sel_flat(&sel);
                eprintln!();
            }
            Err(e) => eprintln!("col {col:2}: error: {e}"),
        }
    }

    // Also try selectionRange at position (1,8) which is start of 'cmon'
    // to see the full hierarchy when queried at the variable symbol's own position
    eprintln!("\n=== detailed selectionRange at (1,8) - var name start ===");
    if let Ok(sel) = lsp.selection_range(path, 1, 8) {
        print_sel(&sel, 0);
    }
}

fn print_syms(syms: &[ane::data::lsp::types::DocumentSymbol], depth: usize) {
    for sym in syms {
        let indent = "  ".repeat(depth);
        let r = &sym.range;
        let sr_str = if let Some(ref sr) = sym.selection_range {
            format!(
                " selRange=({},{})..({},{})",
                sr.start_line, sr.start_col, sr.end_line, sr.end_col
            )
        } else {
            String::new()
        };
        eprintln!(
            "{indent}{} (kind={:?}) range=({},{})..({},{}){}",
            sym.name, sym.kind, r.start_line, r.start_col, r.end_line, r.end_col, sr_str
        );
        print_syms(&sym.children, depth + 1);
    }
}

fn print_sel(sel: &ane::data::lsp::types::SelectionRange, depth: usize) {
    let indent = "  ".repeat(depth);
    let r = &sel.range;
    eprintln!(
        "{indent}({},{})..({},{})",
        r.start_line, r.start_col, r.end_line, r.end_col
    );
    if let Some(ref parent) = sel.parent {
        print_sel(parent, depth + 1);
    }
}

fn print_sel_flat(sel: &ane::data::lsp::types::SelectionRange) {
    let r = &sel.range;
    eprint!(
        "({},{})..({},{})",
        r.start_line, r.start_col, r.end_line, r.end_col
    );
    if let Some(ref parent) = sel.parent {
        eprint!(" → ");
        print_sel_flat(parent);
    }
}
