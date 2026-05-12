pub mod errors;
pub mod parser;
pub mod patcher;
pub mod resolver;
pub mod text;
pub mod types;

use std::collections::HashMap;

use anyhow::Result;

use crate::commands::lsp_engine::LspEngine;
use crate::data::buffer::Buffer;

use types::{ChordAction, ChordQuery, ResolvedChord};

pub struct ChordEngine;

impl ChordEngine {
    pub fn execute(
        chord_input: &str,
        buffers: &HashMap<String, Buffer>,
        lsp: &mut LspEngine,
    ) -> Result<HashMap<String, ChordAction>> {
        let query = Self::parse(chord_input)?;
        let resolved = Self::resolve(&query, buffers, lsp)?;
        Self::patch(&resolved, buffers)
    }

    pub fn parse(chord_input: &str) -> Result<ChordQuery> {
        parser::parse(chord_input)
    }

    pub fn resolve(
        query: &ChordQuery,
        buffers: &HashMap<String, Buffer>,
        lsp: &mut LspEngine,
    ) -> Result<ResolvedChord> {
        resolver::resolve(query, buffers, lsp)
    }

    pub fn patch(
        resolved: &ResolvedChord,
        buffers: &HashMap<String, Buffer>,
    ) -> Result<HashMap<String, ChordAction>> {
        patcher::patch(resolved, buffers)
    }

    pub fn try_auto_submit_short(
        input: &str,
        cursor_line: usize,
        cursor_col: usize,
    ) -> Option<ChordQuery> {
        if input.len() != 4 {
            return None;
        }
        if input.chars().next().map_or(true, |c| c.is_uppercase()) {
            return None;
        }
        let mut query = match Self::parse(input) {
            Ok(q) => q,
            Err(_) => return None,
        };
        query.args.cursor_pos = Some((cursor_line, cursor_col));
        Some(query)
    }
}

pub fn parens_balanced(input: &str) -> bool {
    let mut depth = 0i32;
    for c in input.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::chord_types::{Action, Component, Positional, Scope};

    #[test]
    fn try_auto_submit_short_valid_chord_sets_cursor_pos() {
        let query =
            ChordEngine::try_auto_submit_short("cifn", 3, 7).expect("cifn is a valid 4-char chord");
        assert_eq!(query.action, Action::Change);
        assert_eq!(query.positional, Positional::Inside);
        assert_eq!(query.scope, Scope::Function);
        assert_eq!(query.component, Component::Name);
        assert_eq!(query.args.cursor_pos, Some((3, 7)));
        assert!(query.requires_lsp);
    }

    #[test]
    fn try_auto_submit_short_invalid_chord_returns_none() {
        assert!(ChordEngine::try_auto_submit_short("xxxx", 0, 0).is_none());
    }

    #[test]
    fn try_auto_submit_short_too_short_returns_none() {
        assert!(ChordEngine::try_auto_submit_short("", 0, 0).is_none());
        assert!(ChordEngine::try_auto_submit_short("c", 0, 0).is_none());
        assert!(ChordEngine::try_auto_submit_short("ci", 0, 0).is_none());
        assert!(ChordEngine::try_auto_submit_short("cif", 0, 0).is_none());
    }

    #[test]
    fn try_auto_submit_short_uppercase_first_char_returns_none() {
        assert!(ChordEngine::try_auto_submit_short("Cifn", 0, 0).is_none());
        assert!(ChordEngine::try_auto_submit_short("CIFN", 0, 0).is_none());
    }

    #[test]
    fn parens_balanced_handles_simple_and_nested() {
        assert!(parens_balanced(""));
        assert!(parens_balanced("Cif()"));
        assert!(parens_balanced("Cif(value:\"foo()\")"));
        assert!(!parens_balanced("Cif(value:\"foo()\""));
        assert!(!parens_balanced("Cif)("));
        assert!(!parens_balanced("Cif("));
    }
}
