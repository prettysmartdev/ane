//! Reproduce WI-16's conversion/merge measurements without an LSP server.
use ane::commands::syntax_engine::{merge, tree_sitter_parse};
use ane::data::lsp::types::{Language, SemanticToken};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    println!(
        "os={} arch={} profile={}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    for lines in [1_000, 5_000, 10_000] {
        let source = "let value = 123;\n".repeat(lines);
        for round in 1..=3 {
            let start = Instant::now();
            let tokens = black_box(tree_sitter_parse::parse(Language::Rust, black_box(&source)));
            println!(
                "parse lines={lines} bytes={} tokens={} round={round} micros={}",
                source.len(),
                tokens.len(),
                start.elapsed().as_micros()
            );
        }
    }
    for count in [1_000, 5_000, 10_000, 50_000] {
        let ts: Vec<_> = (0..count)
            .map(|line| SemanticToken {
                line,
                start_col: 0,
                length: 3,
                token_type: "keyword".into(),
            })
            .collect();
        let lsp: Vec<_> = (0..count)
            .map(|line| SemanticToken {
                line,
                start_col: 10,
                length: 3,
                token_type: "variable".into(),
            })
            .collect();
        let start = Instant::now();
        let result = black_box(merge::merge(black_box(&ts), black_box(&lsp)));
        println!(
            "merge tokens_per_source={count} result={} micros={}",
            result.len(),
            start.elapsed().as_micros()
        );
    }
}
