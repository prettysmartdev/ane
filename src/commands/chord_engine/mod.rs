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
}
