use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use crate::commands::lsp_engine::LspEngine;
use crate::data::buffer::Buffer;
use crate::data::chord_types::{Action, Component, Positional, Scope};
use crate::data::lsp::types::{DocumentSymbol, SymbolKind};

use super::errors::ChordError;
use super::text::{char_to_byte, extract_range_text, line_char_count};
use super::types::{
    BufferResolution, ChordQuery, CursorPosition, EditorMode, ResolvedChord, TextRange,
};

pub fn resolve(
    query: &ChordQuery,
    buffers: &HashMap<String, Buffer>,
    lsp: &mut LspEngine,
) -> Result<ResolvedChord> {
    let mut resolutions = HashMap::new();

    for (name, buffer) in buffers {
        let resolution = resolve_buffer(query, name, buffer, lsp)?;
        resolutions.insert(name.clone(), resolution);
    }

    Ok(ResolvedChord {
        query: query.clone(),
        resolutions,
    })
}

fn resolve_buffer(
    query: &ChordQuery,
    buffer_name: &str,
    buffer: &Buffer,
    lsp: &mut LspEngine,
) -> Result<BufferResolution> {
    let scope_range = resolve_scope(query, buffer_name, buffer, lsp)?;
    let component_range = resolve_component(query, buffer, &scope_range, buffer_name, lsp)?;
    let target_ranges =
        apply_positional(query, buffer, &scope_range, &component_range, buffer_name)?;

    let replacement = query.args.value.clone();
    let primary = target_ranges.first().copied().unwrap_or(TextRange::point(
        scope_range.start_line,
        scope_range.start_col,
    ));
    let (cursor_destination, mode_after) = resolve_cursor_and_mode(query, &primary);

    Ok(BufferResolution {
        target_ranges,
        scope_range,
        component_range,
        replacement,
        cursor_destination,
        mode_after,
    })
}

fn resolve_scope(
    query: &ChordQuery,
    buffer_name: &str,
    buffer: &Buffer,
    lsp: &mut LspEngine,
) -> Result<TextRange> {
    match query.scope {
        Scope::Line => resolve_line_scope(query, buffer, buffer_name),
        Scope::Buffer => resolve_buffer_scope(buffer),
        Scope::Function => resolve_lsp_scope(
            query,
            buffer_name,
            buffer,
            lsp,
            &[SymbolKind::Function, SymbolKind::Method],
        ),
        Scope::Variable => resolve_lsp_scope(
            query,
            buffer_name,
            buffer,
            lsp,
            &[SymbolKind::Variable, SymbolKind::Const],
        ),
        Scope::Struct => resolve_lsp_scope(
            query,
            buffer_name,
            buffer,
            lsp,
            &[SymbolKind::Struct, SymbolKind::Enum],
        ),
        Scope::Member => resolve_member_scope(query, buffer_name, buffer, lsp),
    }
}

fn resolve_line_scope(query: &ChordQuery, buffer: &Buffer, buffer_name: &str) -> Result<TextRange> {
    let line = match query
        .args
        .target_line
        .or(query.args.cursor_pos.map(|(l, _)| l))
    {
        Some(l) => l,
        None => {
            return Err(ChordError::resolve(
                buffer_name,
                "line scope requires either 'line:' arg or a cursor position",
            )
            .into());
        }
    };

    if buffer.line_count() == 0 {
        return Err(
            ChordError::resolve(buffer_name, "cannot resolve line scope on empty buffer").into(),
        );
    }

    if line >= buffer.line_count() {
        return Err(ChordError::resolve(
            buffer_name,
            format!(
                "line {line} out of range (file has {} lines)",
                buffer.line_count()
            ),
        )
        .into());
    }

    let line_len = line_char_count(&buffer.lines[line]);
    Ok(TextRange {
        start_line: line,
        start_col: 0,
        end_line: line,
        end_col: line_len,
    })
}

fn resolve_buffer_scope(buffer: &Buffer) -> Result<TextRange> {
    let last_line = buffer.line_count().saturating_sub(1);
    let last_col = buffer
        .lines
        .get(last_line)
        .map(|l| line_char_count(l))
        .unwrap_or(0);
    Ok(TextRange {
        start_line: 0,
        start_col: 0,
        end_line: last_line,
        end_col: last_col,
    })
}

fn resolve_lsp_scope(
    query: &ChordQuery,
    buffer_name: &str,
    _buffer: &Buffer,
    lsp: &mut LspEngine,
    target_kinds: &[SymbolKind],
) -> Result<TextRange> {
    let path = Path::new(buffer_name);
    let symbols = lsp.document_symbols(path).map_err(|e| {
        ChordError::resolve(
            buffer_name,
            format!("LSP not ready: {e}; LSP-scoped chords need an active language server"),
        )
    })?;

    if let Some(ref name) = query.args.target_name {
        if let Some(sym) = find_symbol_by_name_and_kind(&symbols, name, target_kinds) {
            return Ok(symbol_to_range(&sym.range));
        }
        let available: Vec<String> = collect_symbols_by_kind(&symbols, target_kinds);
        return Err(ChordError::resolve_with_symbols(
            buffer_name,
            format!("symbol '{name}' not found"),
            available,
        )
        .into());
    }

    if let Some((line, col)) = query.args.cursor_pos {
        if matches!(query.positional, Positional::Next | Positional::Previous) {
            if let Some(sym) =
                find_neighbor_symbol(&symbols, line, col, target_kinds, query.positional)
            {
                return Ok(symbol_to_range(&sym.range));
            }
            return Err(ChordError::resolve(
                buffer_name,
                format!(
                    "no {} symbol found from cursor ({line}, {col})",
                    if query.positional == Positional::Next {
                        "next"
                    } else {
                        "previous"
                    }
                ),
            )
            .into());
        }

        if let Some(sym) = find_symbol_at_position_by_kind(&symbols, line, col, target_kinds) {
            return Ok(symbol_to_range(&sym.range));
        }
        return Err(ChordError::resolve(
            buffer_name,
            format!("no matching symbol at cursor position ({line}, {col})"),
        )
        .into());
    }

    Err(ChordError::resolve(
        buffer_name,
        "LSP scope requires either a target name or cursor position",
    )
    .into())
}

fn resolve_member_scope(
    query: &ChordQuery,
    buffer_name: &str,
    _buffer: &Buffer,
    lsp: &mut LspEngine,
) -> Result<TextRange> {
    let path = Path::new(buffer_name);
    let symbols = lsp.document_symbols(path).map_err(|e| {
        ChordError::resolve(
            buffer_name,
            format!("LSP not ready: {e}; member-scoped chords need an active language server"),
        )
    })?;

    let parent_kinds = &[SymbolKind::Struct, SymbolKind::Enum];

    if let Some(ref name) = query.args.target_name {
        if let Some(parent_name) = query.args.parent_name.as_deref() {
            if let Some(parent) = find_symbol_by_name_and_kind(&symbols, parent_name, parent_kinds)
            {
                if let Some(child) = parent.children.iter().find(|c| c.name == *name) {
                    return Ok(symbol_to_range(&child.range));
                }
                let available: Vec<String> =
                    parent.children.iter().map(|c| c.name.clone()).collect();
                return Err(ChordError::resolve_with_symbols(
                    buffer_name,
                    format!("member '{name}' not found in '{parent_name}'"),
                    available,
                )
                .into());
            }
            return Err(ChordError::resolve(
                buffer_name,
                format!("parent struct/enum '{parent_name}' not found"),
            )
            .into());
        }

        let mut matches: Vec<(&DocumentSymbol, &DocumentSymbol)> = Vec::new();
        collect_member_matches(&symbols, name, parent_kinds, &mut matches);
        match matches.len() {
            0 => {
                return Err(ChordError::resolve(
                    buffer_name,
                    format!("member '{name}' not found in any struct or enum"),
                )
                .into());
            }
            1 => return Ok(symbol_to_range(&matches[0].1.range)),
            _ => {
                let parents: Vec<String> = matches.iter().map(|(p, _)| p.name.clone()).collect();
                return Err(ChordError::resolve_with_symbols(
                    buffer_name,
                    format!(
                        "member '{name}' is ambiguous (defined in {}); pass parent:<name> to disambiguate",
                        parents.join(", ")
                    ),
                    parents,
                )
                .into());
            }
        }
    }

    if let Some((line, col)) = query.args.cursor_pos {
        if let Some(member) = find_member_at_cursor(&symbols, line, col, parent_kinds) {
            return Ok(symbol_to_range(&member.range));
        }
        return Err(ChordError::resolve(
            buffer_name,
            format!("no member found at cursor position ({line}, {col})"),
        )
        .into());
    }

    Err(ChordError::resolve(
        buffer_name,
        "member scope requires either a target name or cursor position",
    )
    .into())
}

fn resolve_component(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
    lsp: &mut LspEngine,
) -> Result<TextRange> {
    match query.component {
        Component::Beginning => Ok(TextRange::point(
            scope_range.start_line,
            scope_range.start_col,
        )),
        Component::End => Ok(TextRange::point(scope_range.end_line, scope_range.end_col)),
        Component::Self_ => Ok(*scope_range),
        Component::Name => resolve_name_component(query, buffer_name, lsp, scope_range),
        Component::Value => resolve_value_component(query, buffer, scope_range, buffer_name),
        Component::Parameters => resolve_parameters_component(buffer, scope_range, buffer_name),
        Component::Arguments => {
            resolve_arguments_component(query, buffer, scope_range, buffer_name)
        }
    }
}

fn resolve_name_component(
    query: &ChordQuery,
    buffer_name: &str,
    lsp: &mut LspEngine,
    scope_range: &TextRange,
) -> Result<TextRange> {
    if query.scope == Scope::Line || query.scope == Scope::Buffer {
        return Ok(TextRange::point(
            scope_range.start_line,
            scope_range.start_col,
        ));
    }

    let path = Path::new(buffer_name);
    let symbols = lsp.document_symbols(path).map_err(|e| {
        ChordError::resolve(
            buffer_name,
            format!("LSP not ready: {e}; cannot resolve Name component"),
        )
    })?;

    if let Some(ref name) = query.args.target_name {
        if let Some(sym) = find_symbol_by_name_recursive(&symbols, name) {
            return Ok(symbol_to_range(&sym.range));
        }
        return Err(ChordError::resolve(
            buffer_name,
            format!("symbol '{name}' not found for Name component"),
        )
        .into());
    }

    if let Some((line, col)) = query.args.cursor_pos {
        if let Some(sym) = find_innermost_symbol(&symbols, line, col) {
            return Ok(symbol_to_range(&sym.range));
        }
        return Err(ChordError::resolve(
            buffer_name,
            format!("no symbol at cursor ({line}, {col}) for Name component"),
        )
        .into());
    }

    Err(ChordError::resolve(
        buffer_name,
        "Name component requires either a target name or cursor position",
    )
    .into())
}

fn resolve_value_component(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    match query.scope {
        Scope::Function | Scope::Struct => find_brace_range(buffer, scope_range, buffer_name),
        Scope::Variable => find_assignment_rhs(buffer, scope_range, buffer_name),
        Scope::Member => find_member_value(buffer, scope_range, buffer_name),
        Scope::Line | Scope::Buffer => Ok(*scope_range),
    }
}

fn resolve_parameters_component(
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    find_paren_range(buffer, scope_range, buffer_name)
}

/// Arguments component: search the buffer for a call expression (name`(`) and
/// return the parenthesized argument list of that call. Requires a target_name
/// or that the scope points at a named function (scope Function with a name
/// known via cursor).
fn resolve_arguments_component(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    let name = query.args.target_name.clone().ok_or_else(|| {
        ChordError::resolve(
            buffer_name,
            "Arguments component requires a target name to locate a call expression",
        )
    })?;

    let content = buffer.content();
    let needle = format!("{name}(");

    let mut search_from = 0;
    let last_line = buffer.line_count().saturating_sub(1);
    let buffer_end = TextRange {
        start_line: 0,
        start_col: 0,
        end_line: last_line,
        end_col: buffer
            .lines
            .get(last_line)
            .map(|l| line_char_count(l))
            .unwrap_or(0),
    };

    while let Some(rel) = content[search_from..].find(&needle) {
        let byte_pos = search_from + rel;
        let paren_byte = byte_pos + needle.len() - 1;
        let prefix = &content[..paren_byte];
        let line = prefix.matches('\n').count();
        let line_start = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = content[line_start..paren_byte].chars().count();

        let in_scope = (line > scope_range.start_line
            || (line == scope_range.start_line && col >= scope_range.start_col))
            && (line < scope_range.end_line
                || (line == scope_range.end_line && col <= scope_range.end_col));
        if in_scope {
            search_from = byte_pos + needle.len();
            continue;
        }

        let signature_scope = TextRange {
            start_line: line,
            start_col: col,
            end_line: buffer_end.end_line,
            end_col: buffer_end.end_col,
        };
        return find_paren_range(buffer, &signature_scope, buffer_name);
    }

    Err(ChordError::resolve(
        buffer_name,
        format!("no call site for '{name}' found in buffer"),
    )
    .into())
}

fn apply_positional(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    component_range: &TextRange,
    buffer_name: &str,
) -> Result<Vec<TextRange>> {
    match query.positional {
        Positional::Inside => {
            if component_range.is_empty() {
                Ok(vec![*component_range])
            } else {
                Ok(vec![shrink_range(buffer, component_range)])
            }
        }
        Positional::Entire => Ok(vec![*component_range]),
        Positional::After => Ok(vec![TextRange {
            start_line: component_range.end_line,
            start_col: component_range.end_col,
            end_line: scope_range.end_line,
            end_col: scope_range.end_col,
        }]),
        Positional::Before => Ok(vec![TextRange {
            start_line: scope_range.start_line,
            start_col: scope_range.start_col,
            end_line: component_range.start_line,
            end_col: component_range.start_col,
        }]),
        Positional::Until => {
            let cursor = query.args.cursor_pos.ok_or_else(|| {
                ChordError::resolve(buffer_name, "'Until' positional requires a cursor position")
            })?;
            Ok(vec![TextRange {
                start_line: cursor.0,
                start_col: cursor.1,
                end_line: component_range.start_line,
                end_col: component_range.start_col,
            }])
        }
        Positional::Outside => Ok(outside_ranges(scope_range, component_range)),
        Positional::Next | Positional::Previous => {
            // For Line scope the component range was computed at the cursor's
            // line; resolve_lsp_scope already moved the scope for LSP scopes.
            // For now just return the component itself; resolve_lsp_scope
            // already errors when no cursor was supplied for an LSP scope.
            if matches!(query.scope, Scope::Line) && query.args.cursor_pos.is_none() {
                return Err(ChordError::resolve(
                    buffer_name,
                    format!(
                        "'{}' positional on Line scope requires a cursor position",
                        if query.positional == Positional::Next {
                            "Next"
                        } else {
                            "Previous"
                        }
                    ),
                )
                .into());
            }
            Ok(vec![*component_range])
        }
    }
}

fn outside_ranges(scope: &TextRange, component: &TextRange) -> Vec<TextRange> {
    let mut out = Vec::new();
    let head = TextRange {
        start_line: scope.start_line,
        start_col: scope.start_col,
        end_line: component.start_line,
        end_col: component.start_col,
    };
    if !head.is_empty() {
        out.push(head);
    }
    let tail = TextRange {
        start_line: component.end_line,
        start_col: component.end_col,
        end_line: scope.end_line,
        end_col: scope.end_col,
    };
    if !tail.is_empty() {
        out.push(tail);
    }
    if out.is_empty() {
        out.push(TextRange::point(scope.start_line, scope.start_col));
    }
    out
}

fn resolve_cursor_and_mode(
    query: &ChordQuery,
    target_range: &TextRange,
) -> (Option<CursorPosition>, Option<EditorMode>) {
    match query.action {
        Action::Change => {
            let cursor = CursorPosition {
                line: target_range.start_line,
                col: target_range.start_col,
            };
            if query.args.value.is_some() {
                (Some(cursor), Some(EditorMode::Chord))
            } else {
                (Some(cursor), Some(EditorMode::Edit))
            }
        }
        Action::Delete => {
            let cursor = CursorPosition {
                line: target_range.start_line,
                col: target_range.start_col,
            };
            (Some(cursor), Some(EditorMode::Chord))
        }
        Action::Append | Action::Prepend | Action::Insert => {
            let cursor = CursorPosition {
                line: target_range.start_line,
                col: target_range.start_col,
            };
            if query.args.value.is_some() {
                (Some(cursor), Some(EditorMode::Chord))
            } else {
                (Some(cursor), Some(EditorMode::Edit))
            }
        }
        Action::Replace => {
            let cursor = CursorPosition {
                line: target_range.start_line,
                col: target_range.start_col,
            };
            (Some(cursor), Some(EditorMode::Chord))
        }
        Action::Yank => (None, None),
    }
}

// --- Helper functions ---

fn symbol_to_range(sr: &crate::data::lsp::types::SymbolRange) -> TextRange {
    TextRange {
        start_line: sr.start_line,
        start_col: sr.start_col,
        end_line: sr.end_line,
        end_col: sr.end_col,
    }
}

fn contains_position(
    range: &crate::data::lsp::types::SymbolRange,
    line: usize,
    col: usize,
) -> bool {
    if line < range.start_line || line > range.end_line {
        return false;
    }
    if line == range.start_line && col < range.start_col {
        return false;
    }
    if line == range.end_line && col > range.end_col {
        return false;
    }
    true
}

fn matches_kind(kind: &SymbolKind, targets: &[SymbolKind]) -> bool {
    targets.contains(kind)
}

fn find_symbol_by_name_and_kind<'a>(
    symbols: &'a [DocumentSymbol],
    name: &str,
    kinds: &[SymbolKind],
) -> Option<&'a DocumentSymbol> {
    for sym in symbols {
        if sym.name == name && matches_kind(&sym.kind, kinds) {
            return Some(sym);
        }
        if let Some(found) = find_symbol_by_name_and_kind(&sym.children, name, kinds) {
            return Some(found);
        }
    }
    None
}

fn find_symbol_by_name_recursive<'a>(
    symbols: &'a [DocumentSymbol],
    name: &str,
) -> Option<&'a DocumentSymbol> {
    for sym in symbols {
        if sym.name == name {
            return Some(sym);
        }
        if let Some(found) = find_symbol_by_name_recursive(&sym.children, name) {
            return Some(found);
        }
    }
    None
}

fn find_symbol_at_position_by_kind<'a>(
    symbols: &'a [DocumentSymbol],
    line: usize,
    col: usize,
    kinds: &[SymbolKind],
) -> Option<&'a DocumentSymbol> {
    let mut best: Option<&'a DocumentSymbol> = None;

    for sym in symbols {
        if contains_position(&sym.range, line, col) {
            if matches_kind(&sym.kind, kinds) {
                best = Some(sym);
            }
            if let Some(child) = find_symbol_at_position_by_kind(&sym.children, line, col, kinds) {
                best = Some(child);
            }
        }
    }

    best
}

fn find_innermost_symbol(
    symbols: &[DocumentSymbol],
    line: usize,
    col: usize,
) -> Option<&DocumentSymbol> {
    for sym in symbols {
        if contains_position(&sym.range, line, col) {
            if let Some(child) = find_innermost_symbol(&sym.children, line, col) {
                return Some(child);
            }
            return Some(sym);
        }
    }
    None
}

fn find_neighbor_symbol<'a>(
    symbols: &'a [DocumentSymbol],
    line: usize,
    col: usize,
    kinds: &[SymbolKind],
    positional: Positional,
) -> Option<&'a DocumentSymbol> {
    let mut flat: Vec<&'a DocumentSymbol> = Vec::new();
    flatten_by_kind(symbols, kinds, &mut flat);
    flat.sort_by(|a, b| {
        a.range
            .start_line
            .cmp(&b.range.start_line)
            .then(a.range.start_col.cmp(&b.range.start_col))
    });

    match positional {
        Positional::Next => flat.into_iter().find(|s| {
            s.range.start_line > line || (s.range.start_line == line && s.range.start_col > col)
        }),
        Positional::Previous => flat.into_iter().rev().find(|s| {
            s.range.end_line < line || (s.range.end_line == line && s.range.end_col < col)
        }),
        _ => None,
    }
}

fn flatten_by_kind<'a>(
    symbols: &'a [DocumentSymbol],
    kinds: &[SymbolKind],
    out: &mut Vec<&'a DocumentSymbol>,
) {
    for sym in symbols {
        if matches_kind(&sym.kind, kinds) {
            out.push(sym);
        }
        flatten_by_kind(&sym.children, kinds, out);
    }
}

fn collect_member_matches<'a>(
    symbols: &'a [DocumentSymbol],
    name: &str,
    parent_kinds: &[SymbolKind],
    out: &mut Vec<(&'a DocumentSymbol, &'a DocumentSymbol)>,
) {
    for sym in symbols {
        if matches_kind(&sym.kind, parent_kinds) {
            for child in &sym.children {
                if child.name == name {
                    out.push((sym, child));
                }
            }
        }
        collect_member_matches(&sym.children, name, parent_kinds, out);
    }
}

fn find_member_at_cursor<'a>(
    symbols: &'a [DocumentSymbol],
    line: usize,
    col: usize,
    parent_kinds: &[SymbolKind],
) -> Option<&'a DocumentSymbol> {
    for sym in symbols {
        if matches_kind(&sym.kind, parent_kinds) && contains_position(&sym.range, line, col) {
            for child in &sym.children {
                if contains_position(&child.range, line, col) {
                    return Some(child);
                }
            }
        }
        if let Some(deeper) = find_member_at_cursor(&sym.children, line, col, parent_kinds) {
            return Some(deeper);
        }
    }
    None
}

fn collect_symbols_by_kind(symbols: &[DocumentSymbol], kinds: &[SymbolKind]) -> Vec<String> {
    let mut result = Vec::new();
    for sym in symbols {
        if matches_kind(&sym.kind, kinds) {
            result.push(sym.name.clone());
        }
        result.extend(collect_symbols_by_kind(&sym.children, kinds));
    }
    result
}

fn find_brace_range(
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    if let Some(range) = scan_balanced(buffer, scope_range, '{', '}') {
        Ok(range)
    } else {
        Err(ChordError::resolve(buffer_name, "no brace block found in scope").into())
    }
}

fn find_paren_range(
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    if let Some(range) = scan_balanced(buffer, scope_range, '(', ')') {
        Ok(range)
    } else {
        Err(ChordError::resolve(buffer_name, "no parenthesized list found in scope").into())
    }
}

fn scan_balanced(
    buffer: &Buffer,
    scope_range: &TextRange,
    open: char,
    close: char,
) -> Option<TextRange> {
    let last = buffer.line_count().saturating_sub(1);
    let start_line = scope_range.start_line.min(last);
    let end_line = scope_range.end_line.min(last);
    let mut depth = 0i32;
    let mut start: Option<(usize, usize)> = None;

    for line_idx in start_line..=end_line {
        let line = &buffer.lines[line_idx];
        let line_chars: Vec<char> = line.chars().collect();
        let from = if line_idx == start_line {
            scope_range.start_col.min(line_chars.len())
        } else {
            0
        };
        let to = if line_idx == end_line {
            scope_range.end_col.min(line_chars.len())
        } else {
            line_chars.len()
        };

        for (col, ch) in line_chars.iter().enumerate().take(to).skip(from) {
            let ch = *ch;
            if ch == open {
                if depth == 0 {
                    start = Some((line_idx, col));
                }
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    if let Some((sl, sc)) = start {
                        return Some(TextRange {
                            start_line: sl,
                            start_col: sc,
                            end_line: line_idx,
                            end_col: col + 1,
                        });
                    }
                }
            }
        }
    }
    None
}

/// Find the first standalone `=` (not part of `==`, `<=`, `>=`, `!=`) in the
/// scope and return the range from after it to the end of the scope.
fn find_assignment_rhs(
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    let content = extract_range_text(buffer, scope_range);
    let chars: Vec<char> = content.chars().collect();
    let mut byte_offset = 0usize;
    let mut char_idx = 0usize;
    while char_idx < chars.len() {
        let c = chars[char_idx];
        if c == '=' {
            let prev = if char_idx > 0 {
                Some(chars[char_idx - 1])
            } else {
                None
            };
            let next = chars.get(char_idx + 1).copied();
            let is_compound = matches!(
                prev,
                Some('!' | '<' | '>' | '=' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^')
            ) || next == Some('=');
            if !is_compound {
                let lines_before = content[..byte_offset].matches('\n').count();
                let line_start = content[..byte_offset]
                    .rfind('\n')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let col_in_line = content[line_start..byte_offset].chars().count();
                let abs_line = scope_range.start_line + lines_before;
                let abs_col = if lines_before == 0 {
                    scope_range.start_col + col_in_line
                } else {
                    col_in_line
                };
                return Ok(TextRange {
                    start_line: abs_line,
                    start_col: abs_col + 1,
                    end_line: scope_range.end_line,
                    end_col: scope_range.end_col,
                });
            }
        }
        byte_offset += c.len_utf8();
        char_idx += 1;
    }
    Err(ChordError::resolve(buffer_name, "variable has no value (no assignment found)").into())
}

/// For a member range (struct field or enum variant):
/// - If contains `:` → struct field; value is the type after `:`.
/// - Else if contains `(` first → tuple-variant; value is the tuple content.
/// - Else if contains `{` first → struct-variant; value is the brace content.
/// - Otherwise → no value (unit variant).
fn find_member_value(
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    let content = extract_range_text(buffer, scope_range);

    let mut depth = 0i32;
    let chars: Vec<char> = content.chars().collect();
    let mut byte_offset = 0usize;
    let mut field_colon: Option<usize> = None;
    let mut variant_open: Option<(char, usize)> = None;
    for (idx, c) in chars.iter().enumerate() {
        if depth == 0 {
            if *c == ':' {
                field_colon = Some(idx);
                break;
            }
            if (*c == '(' || *c == '{') && variant_open.is_none() {
                variant_open = Some((*c, byte_offset));
            }
        }
        if *c == '(' || *c == '{' {
            depth += 1;
        } else if *c == ')' || *c == '}' {
            depth -= 1;
        }
        byte_offset += c.len_utf8();
    }

    if let Some(colon_char_idx) = field_colon {
        let _ = colon_char_idx;
        let colon_byte = char_to_byte(&content, colon_char_idx);
        let lines_before = content[..colon_byte].matches('\n').count();
        let line_start = content[..colon_byte]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let col_in_line = content[line_start..colon_byte].chars().count();
        let abs_line = scope_range.start_line + lines_before;
        let abs_col = if lines_before == 0 {
            scope_range.start_col + col_in_line
        } else {
            col_in_line
        };
        return Ok(TextRange {
            start_line: abs_line,
            start_col: abs_col + 1,
            end_line: scope_range.end_line,
            end_col: scope_range.end_col,
        });
    }

    if let Some((_open_ch, open_byte)) = variant_open {
        let lines_before = content[..open_byte].matches('\n').count();
        let line_start = content[..open_byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_in_line = content[line_start..open_byte].chars().count();
        let abs_line = scope_range.start_line + lines_before;
        let abs_col = if lines_before == 0 {
            scope_range.start_col + col_in_line
        } else {
            col_in_line
        };
        return Ok(TextRange {
            start_line: abs_line,
            start_col: abs_col,
            end_line: scope_range.end_line,
            end_col: scope_range.end_col,
        });
    }

    Err(ChordError::resolve(buffer_name, "member has no value").into())
}

fn shrink_range(buffer: &Buffer, range: &TextRange) -> TextRange {
    let content = extract_range_text(buffer, range);

    let first_char = content.chars().next();
    let last_char = content.chars().last();

    let is_delimited = matches!(
        (first_char, last_char),
        (Some('('), Some(')')) | (Some('{'), Some('}')) | (Some('['), Some(']'))
    );

    if is_delimited {
        let total_chars = content.chars().count();
        if total_chars >= 2 {
            let inner_lines = content.matches('\n').count();
            let start_line = range.start_line;
            let start_col = range.start_col + 1;

            let end_line;
            let end_col;
            if inner_lines == 0 {
                end_line = start_line;
                end_col = range.end_col.saturating_sub(1);
            } else {
                end_line = range.end_line;
                end_col = range.end_col.saturating_sub(1);
            }

            return TextRange {
                start_line,
                start_col,
                end_line,
                end_col,
            };
        }
    }
    *range
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use crate::commands::chord_engine::types::{
        ChordArgs, ChordQuery, CursorPosition, EditorMode, TextRange,
    };
    use crate::commands::lsp_engine::{LspEngine, LspEngineConfig};
    use crate::data::buffer::Buffer;
    use crate::data::chord_types::{Action, Component, Positional, Scope};
    use crate::data::lsp::types::{DocumentSymbol, SymbolKind, SymbolRange};

    use super::*;

    fn buf(lines: &[&str]) -> Buffer {
        Buffer {
            path: PathBuf::from("/test/file.rs"),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            dirty: false,
            trailing_newline: false,
        }
    }

    fn named_buf(path: &str, lines: &[&str]) -> Buffer {
        Buffer {
            path: PathBuf::from(path),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            dirty: false,
            trailing_newline: false,
        }
    }

    fn query(
        action: Action,
        pos: Positional,
        scope: Scope,
        comp: Component,
        target_name: Option<&str>,
        target_line: Option<usize>,
        cursor_pos: Option<(usize, usize)>,
    ) -> ChordQuery {
        ChordQuery {
            action,
            positional: pos,
            scope,
            component: comp,
            args: ChordArgs {
                target_name: target_name.map(String::from),
                parent_name: None,
                target_line,
                cursor_pos,
                value: None,
                find: None,
                replace: None,
            },
            requires_lsp: scope.requires_lsp(),
        }
    }

    fn sym(
        name: &str,
        kind: SymbolKind,
        sl: usize,
        sc: usize,
        el: usize,
        ec: usize,
    ) -> DocumentSymbol {
        DocumentSymbol {
            name: name.to_string(),
            kind,
            range: SymbolRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            },
            children: vec![],
        }
    }

    fn sym_with_children(
        name: &str,
        kind: SymbolKind,
        sl: usize,
        sc: usize,
        el: usize,
        ec: usize,
        children: Vec<DocumentSymbol>,
    ) -> DocumentSymbol {
        DocumentSymbol {
            name: name.to_string(),
            kind,
            range: SymbolRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            },
            children,
        }
    }

    fn mock_lsp(path: &str, symbols: Vec<DocumentSymbol>) -> LspEngine {
        let mut lsp = LspEngine::new(LspEngineConfig::default());
        lsp.inject_test_symbols(PathBuf::from(path), symbols);
        lsp
    }

    fn no_lsp() -> LspEngine {
        LspEngine::new(LspEngineConfig::default())
    }

    // --- resolve_line_scope ---

    #[test]
    fn line_scope_by_explicit_line_number() {
        let buffer = buf(&["first", "second", "third"]);
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            Some(1),
            None,
        );
        let range = resolve_line_scope(&q, &buffer, "/buf").unwrap();
        assert_eq!(range.start_line, 1);
        assert_eq!(range.start_col, 0);
        assert_eq!(range.end_line, 1);
        assert_eq!(range.end_col, 6);
    }

    #[test]
    fn line_scope_by_cursor_position() {
        let buffer = buf(&["alpha", "beta", "gamma"]);
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            None,
            Some((2, 3)),
        );
        let range = resolve_line_scope(&q, &buffer, "/buf").unwrap();
        assert_eq!(range.start_line, 2);
        assert_eq!(range.end_line, 2);
        assert_eq!(range.end_col, 5);
    }

    #[test]
    fn line_scope_without_input_errors() {
        let buffer = buf(&["only", "line"]);
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            None,
            None,
        );
        let err = resolve_line_scope(&q, &buffer, "/buf").unwrap_err();
        assert!(format!("{err}").contains("requires"));
    }

    #[test]
    fn line_scope_out_of_bounds_errors() {
        let buffer = buf(&["one", "two"]);
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            Some(5),
            None,
        );
        assert!(resolve_line_scope(&q, &buffer, "/buf").is_err());
    }

    #[test]
    fn line_scope_unicode_char_count() {
        let buffer = buf(&["héllo wörld"]);
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            Some(0),
            None,
        );
        let range = resolve_line_scope(&q, &buffer, "/buf").unwrap();
        assert_eq!(range.end_col, 11);
    }

    // --- resolve_buffer_scope ---

    #[test]
    fn buffer_scope_entire_file() {
        let buffer = buf(&["line one", "line two", "line three"]);
        let range = resolve_buffer_scope(&buffer).unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.start_col, 0);
        assert_eq!(range.end_line, 2);
        assert_eq!(range.end_col, 10);
    }

    #[test]
    fn buffer_scope_empty() {
        let buffer = Buffer {
            path: PathBuf::from("/test"),
            lines: vec![],
            dirty: false,
            trailing_newline: false,
        };
        let range = resolve_buffer_scope(&buffer).unwrap();
        assert_eq!(range.end_line, 0);
        assert_eq!(range.end_col, 0);
    }

    // --- find_brace_range ---

    #[test]
    fn find_brace_range_single_line() {
        let buffer = buf(&["fn foo() { 42 }"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 15,
        };
        let range = find_brace_range(&buffer, &scope, "/buf").unwrap();
        assert_eq!(range.start_col, 9);
        assert_eq!(range.end_col, 15);
    }

    #[test]
    fn find_brace_range_multi_line() {
        let buffer = buf(&["fn foo() {", "    42", "}"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 2,
            end_col: 1,
        };
        let range = find_brace_range(&buffer, &scope, "/buf").unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.start_col, 9);
        assert_eq!(range.end_line, 2);
        assert_eq!(range.end_col, 1);
    }

    #[test]
    fn find_brace_range_no_braces_errors() {
        let buffer = buf(&["no braces here"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 14,
        };
        assert!(find_brace_range(&buffer, &scope, "/buf").is_err());
    }

    // --- find_paren_range ---

    #[test]
    fn find_paren_range_simple() {
        let buffer = buf(&["fn foo(x: i32, y: i32) -> i32 {}"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 33,
        };
        let range = find_paren_range(&buffer, &scope, "/buf").unwrap();
        assert_eq!(range.start_col, 6);
        assert_eq!(range.end_col, 22);
    }

    #[test]
    fn find_paren_range_no_params_empty() {
        let buffer = buf(&["fn foo() {}"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 11,
        };
        let range = find_paren_range(&buffer, &scope, "/buf").unwrap();
        assert_eq!(range.start_col, 6);
        assert_eq!(range.end_col, 8);
        let inside = shrink_range(&buffer, &range);
        assert!(inside.is_empty());
    }

    #[test]
    fn find_paren_range_multi_line() {
        let buffer = buf(&["fn foo(", "    x: i32,", "    y: i32,", ") {}"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 3,
            end_col: 4,
        };
        let range = find_paren_range(&buffer, &scope, "/buf").unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.start_col, 6);
        assert_eq!(range.end_line, 3);
        assert_eq!(range.end_col, 1);
    }

    #[test]
    fn find_paren_range_no_parens_errors() {
        let buffer = buf(&["struct Foo { x: i32 }"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 21,
        };
        assert!(find_paren_range(&buffer, &scope, "/buf").is_err());
    }

    // --- find_assignment_rhs ---

    #[test]
    fn find_assignment_rhs_simple() {
        let buffer = buf(&["let x = 42;"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 11,
        };
        let range = find_assignment_rhs(&buffer, &scope, "/buf").unwrap();
        assert!(range.start_col > 6);
    }

    #[test]
    fn find_assignment_rhs_no_value_errors() {
        let buffer = buf(&["let x: i32;"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 11,
        };
        assert!(find_assignment_rhs(&buffer, &scope, "/buf").is_err());
    }

    #[test]
    fn find_assignment_rhs_skips_double_eq() {
        let buffer = buf(&["let x = if a == b { 1 } else { 2 };"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 35,
        };
        let range = find_assignment_rhs(&buffer, &scope, "/buf").unwrap();
        assert_eq!(range.start_col, 7); // position after the first `=` (idx 6)
    }

    // --- find_member_value ---

    #[test]
    fn find_member_value_struct_field() {
        let buffer = buf(&["    field: i32,"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 4,
            end_line: 0,
            end_col: 15,
        };
        let range = find_member_value(&buffer, &scope, "/buf").unwrap();
        assert!(range.start_col > scope.start_col);
    }

    #[test]
    fn find_member_value_unit_variant_errors() {
        let buffer = buf(&["    None,"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 4,
            end_line: 0,
            end_col: 9,
        };
        assert!(find_member_value(&buffer, &scope, "/buf").is_err());
    }

    #[test]
    fn find_member_value_tuple_variant() {
        let buffer = buf(&["    Some(T),"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 4,
            end_line: 0,
            end_col: 11,
        };
        let range = find_member_value(&buffer, &scope, "/buf").unwrap();
        assert!(range.start_col >= 8);
    }

    #[test]
    fn find_member_value_struct_variant() {
        let buffer = buf(&["    Variant { field: T },"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 4,
            end_line: 0,
            end_col: 24,
        };
        let range = find_member_value(&buffer, &scope, "/buf").unwrap();
        assert!(range.start_col >= 12);
    }

    // --- shrink_range ---

    #[test]
    fn shrink_range_removes_braces() {
        let buffer = buf(&["{hello}"]);
        let range = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 7,
        };
        let shrunk = shrink_range(&buffer, &range);
        assert_eq!(shrunk.start_col, 1);
        assert_eq!(shrunk.end_col, 6);
    }

    #[test]
    fn shrink_range_removes_parens() {
        let buffer = buf(&["(abc)"]);
        let range = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let shrunk = shrink_range(&buffer, &range);
        assert_eq!(shrunk.start_col, 1);
        assert_eq!(shrunk.end_col, 4);
    }

    #[test]
    fn shrink_range_no_delimiters_unchanged() {
        let buffer = buf(&["hello"]);
        let range = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let shrunk = shrink_range(&buffer, &range);
        assert_eq!(shrunk, range);
    }

    // --- outside_ranges ---

    #[test]
    fn outside_returns_two_ranges_when_component_in_middle() {
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 20,
        };
        let comp = TextRange {
            start_line: 0,
            start_col: 5,
            end_line: 0,
            end_col: 10,
        };
        let ranges = outside_ranges(&scope, &comp);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].end_col, 5);
        assert_eq!(ranges[1].start_col, 10);
    }

    #[test]
    fn outside_returns_single_range_when_component_at_start() {
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 20,
        };
        let comp = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 10,
        };
        let ranges = outside_ranges(&scope, &comp);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].start_col, 10);
    }

    #[test]
    fn outside_returns_empty_point_when_component_equals_scope() {
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 20,
        };
        let ranges = outside_ranges(&scope, &scope);
        assert_eq!(ranges.len(), 1);
        assert!(ranges[0].is_empty());
    }

    // --- apply_positional ---

    #[test]
    fn positional_entire_returns_component() {
        let buffer = buf(&["fn foo(x: i32) {}"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 18,
        };
        let comp = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 14,
        };
        let q = query(
            Action::Yank,
            Positional::Entire,
            Scope::Function,
            Component::Parameters,
            None,
            None,
            None,
        );
        let result = apply_positional(&q, &buffer, &scope, &comp, "/buf").unwrap();
        assert_eq!(result, vec![comp]);
    }

    #[test]
    fn positional_outside_excludes_component() {
        let buffer = buf(&["fn foo(x: i32) {}"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 17,
        };
        let comp = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 14,
        };
        let q = query(
            Action::Change,
            Positional::Outside,
            Scope::Function,
            Component::Parameters,
            None,
            None,
            None,
        );
        let result = apply_positional(&q, &buffer, &scope, &comp, "/buf").unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].end_col, 6);
        assert_eq!(result[1].start_col, 14);
    }

    #[test]
    fn positional_until_no_cursor_errors() {
        let buffer = buf(&["abc"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 3,
        };
        let comp = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 3,
        };
        let q = query(
            Action::Change,
            Positional::Until,
            Scope::Line,
            Component::Self_,
            None,
            None,
            None,
        );
        assert!(apply_positional(&q, &buffer, &scope, &comp, "/buf").is_err());
    }

    #[test]
    fn positional_inside_shrinks_delimited_range() {
        let buffer = buf(&["(hello)"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 7,
        };
        let comp = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 7,
        };
        let q = query(
            Action::Change,
            Positional::Inside,
            Scope::Line,
            Component::Self_,
            None,
            None,
            None,
        );
        let result = apply_positional(&q, &buffer, &scope, &comp, "/buf").unwrap();
        assert_eq!(result[0].start_col, 1);
        assert_eq!(result[0].end_col, 6);
    }

    #[test]
    fn positional_until_from_cursor_to_component() {
        let buffer = buf(&["fn foo(x: i32) {}"]);
        let scope = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 18,
        };
        let comp = TextRange {
            start_line: 0,
            start_col: 6,
            end_line: 0,
            end_col: 14,
        };
        let q = query(
            Action::Change,
            Positional::Until,
            Scope::Function,
            Component::Parameters,
            None,
            None,
            Some((0, 2)),
        );
        let result = apply_positional(&q, &buffer, &scope, &comp, "/buf").unwrap();
        assert_eq!(result[0].start_col, 2);
        assert_eq!(result[0].end_col, 6);
    }

    // --- resolve_cursor_and_mode ---

    #[test]
    fn cursor_mode_change_no_value_goes_to_edit() {
        let range = TextRange {
            start_line: 2,
            start_col: 5,
            end_line: 2,
            end_col: 10,
        };
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            None,
            None,
        );
        let (cursor, mode) = resolve_cursor_and_mode(&q, &range);
        assert_eq!(cursor, Some(CursorPosition { line: 2, col: 5 }));
        assert_eq!(mode, Some(EditorMode::Edit));
    }

    #[test]
    fn cursor_mode_yank_is_none() {
        let range = TextRange {
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 5,
        };
        let q = query(
            Action::Yank,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            None,
            None,
        );
        let (cursor, mode) = resolve_cursor_and_mode(&q, &range);
        assert!(cursor.is_none());
        assert!(mode.is_none());
    }

    // --- Full resolve() with mock LSP ---

    #[test]
    fn function_scope_by_name_resolves_range() {
        let path = "/test/file.rs";
        let lines = &["fn foo() {}", "fn bar(x: i32) {", "    x + 1", "}"];
        let buffer = named_buf(path, lines);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(
            path,
            vec![
                sym("foo", SymbolKind::Function, 0, 0, 0, 11),
                sym("bar", SymbolKind::Function, 1, 0, 3, 1),
            ],
        );

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Function,
            Component::Self_,
            Some("bar"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
        assert_eq!(res.scope_range.end_line, 3);
    }

    #[test]
    fn function_scope_not_found_lists_available_symbols() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 0, 0, 0, 11)]);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Function,
            Component::Self_,
            Some("missing"),
            None,
            None,
        );
        let err = resolve(&q, &buffers, &mut lsp).unwrap_err();
        assert!(format!("{err}").contains("foo"));
    }

    #[test]
    fn function_scope_innermost_at_cursor() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn outer() { fn inner() {} }"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let inner = sym("inner", SymbolKind::Function, 0, 13, 0, 26);
        let outer = sym_with_children(
            "outer",
            SymbolKind::Function,
            0,
            0,
            0,
            28,
            vec![inner.clone()],
        );
        let mut lsp = mock_lsp(path, vec![outer]);

        let q = query(
            Action::Yank,
            Positional::Entire,
            Scope::Function,
            Component::Self_,
            None,
            None,
            Some((0, 18)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_col, 13);
    }

    #[test]
    fn function_scope_without_input_errors() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 0, 0, 0, 11)]);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Function,
            Component::Self_,
            None,
            None,
            None,
        );
        assert!(resolve(&q, &buffers, &mut lsp).is_err());
    }

    #[test]
    fn member_scope_field_by_name() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["struct Foo {", "    x: i32,", "    y: f64,", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let field_x = sym("x", SymbolKind::Field, 1, 4, 1, 10);
        let field_y = sym("y", SymbolKind::Field, 2, 4, 2, 10);
        let struct_sym = sym_with_children(
            "Foo",
            SymbolKind::Struct,
            0,
            0,
            3,
            1,
            vec![field_x, field_y],
        );
        let mut lsp = mock_lsp(path, vec![struct_sym]);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Member,
            Component::Self_,
            Some("y"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 2);
    }

    #[test]
    fn member_scope_ambiguous_without_parent() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["struct A { x: i32 }", "struct B { x: i32 }"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let a_x = sym("x", SymbolKind::Field, 0, 11, 0, 18);
        let b_x = sym("x", SymbolKind::Field, 1, 11, 1, 18);
        let a = sym_with_children("A", SymbolKind::Struct, 0, 0, 0, 19, vec![a_x]);
        let b = sym_with_children("B", SymbolKind::Struct, 1, 0, 1, 19, vec![b_x]);
        let mut lsp = mock_lsp(path, vec![a, b]);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Member,
            Component::Self_,
            Some("x"),
            None,
            None,
        );
        let err = resolve(&q, &buffers, &mut lsp).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("ambiguous"), "expected ambiguous error: {msg}");
    }

    #[test]
    fn member_scope_disambiguates_with_parent() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["struct A { x: i32 }", "struct B { x: i32 }"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let a_x = sym("x", SymbolKind::Field, 0, 11, 0, 18);
        let b_x = sym("x", SymbolKind::Field, 1, 11, 1, 18);
        let a = sym_with_children("A", SymbolKind::Struct, 0, 0, 0, 19, vec![a_x]);
        let b = sym_with_children("B", SymbolKind::Struct, 1, 0, 1, 19, vec![b_x]);
        let mut lsp = mock_lsp(path, vec![a, b]);

        let mut q = query(
            Action::Change,
            Positional::Entire,
            Scope::Member,
            Component::Self_,
            Some("x"),
            None,
            None,
        );
        q.args.parent_name = Some("B".to_string());
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
    }

    #[test]
    fn member_scope_enum_variant_by_name() {
        let path = "/test/file.rs";
        let buffer = named_buf(
            path,
            &["enum Color {", "    Red,", "    Green,", "    Blue,", "}"],
        );
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let red = sym("Red", SymbolKind::Field, 1, 4, 1, 7);
        let green = sym("Green", SymbolKind::Field, 2, 4, 2, 9);
        let blue = sym("Blue", SymbolKind::Field, 3, 4, 3, 8);
        let enum_sym = sym_with_children(
            "Color",
            SymbolKind::Enum,
            0,
            0,
            4,
            1,
            vec![red, green, blue],
        );
        let mut lsp = mock_lsp(path, vec![enum_sym]);

        let q = query(
            Action::Yank,
            Positional::Entire,
            Scope::Member,
            Component::Self_,
            Some("Green"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 2);
    }

    #[test]
    fn member_scope_recurses_into_nested_modules() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["mod inner {", "    struct Foo { x: i32 }", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let field_x = sym("x", SymbolKind::Field, 1, 17, 1, 24);
        let foo = sym_with_children("Foo", SymbolKind::Struct, 1, 4, 1, 27, vec![field_x]);
        let inner = sym_with_children("inner", SymbolKind::Module, 0, 0, 2, 1, vec![foo]);
        let mut lsp = mock_lsp(path, vec![inner]);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Member,
            Component::Self_,
            Some("x"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
    }

    #[test]
    fn arguments_component_finds_call_site() {
        let path = "/test/file.rs";
        let buffer = named_buf(
            path,
            &["fn foo(x: i32) -> i32 { x }", "", "fn main() { foo(42); }"],
        );
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = mock_lsp(
            path,
            vec![
                sym("foo", SymbolKind::Function, 0, 0, 0, 27),
                sym("main", SymbolKind::Function, 2, 0, 2, 22),
            ],
        );

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Function,
            Component::Arguments,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let primary = res.target_ranges.first().unwrap();
        assert_eq!(primary.start_line, 2);
    }

    #[test]
    fn arguments_component_no_call_errors() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 0, 0, 0, 11)]);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Function,
            Component::Arguments,
            Some("foo"),
            None,
            None,
        );
        assert!(resolve(&q, &buffers, &mut lsp).is_err());
    }

    #[test]
    fn next_lsp_scope_finds_following_symbol() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn alpha() {}", "fn beta() {}", "fn gamma() {}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = mock_lsp(
            path,
            vec![
                sym("alpha", SymbolKind::Function, 0, 0, 0, 13),
                sym("beta", SymbolKind::Function, 1, 0, 1, 12),
                sym("gamma", SymbolKind::Function, 2, 0, 2, 13),
            ],
        );

        let q = query(
            Action::Yank,
            Positional::Next,
            Scope::Function,
            Component::Self_,
            None,
            None,
            Some((0, 5)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
    }

    #[test]
    fn previous_lsp_scope_finds_preceding_symbol() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn alpha() {}", "fn beta() {}", "fn gamma() {}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = mock_lsp(
            path,
            vec![
                sym("alpha", SymbolKind::Function, 0, 0, 0, 13),
                sym("beta", SymbolKind::Function, 1, 0, 1, 12),
                sym("gamma", SymbolKind::Function, 2, 0, 2, 13),
            ],
        );

        let q = query(
            Action::Yank,
            Positional::Previous,
            Scope::Function,
            Component::Self_,
            None,
            None,
            Some((2, 5)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
    }

    #[test]
    fn multi_buffer_chord_applies_to_each_independently() {
        let path_a = "/test/a.txt";
        let path_b = "/test/b.txt";
        let buf_a = named_buf(path_a, &["aaa", "bbb", "ccc"]);
        let buf_b = named_buf(path_b, &["xxx", "yyy", "zzz"]);
        let mut buffers = HashMap::new();
        buffers.insert(path_a.to_string(), buf_a);
        buffers.insert(path_b.to_string(), buf_b);

        let mut lsp = no_lsp();
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Line,
            Component::Self_,
            None,
            Some(1),
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        assert_eq!(resolved.resolutions.len(), 2);
        let res_a = resolved.resolutions.get(path_a).unwrap();
        let res_b = resolved.resolutions.get(path_b).unwrap();
        assert_eq!(res_a.scope_range.start_line, 1);
        assert_eq!(res_b.scope_range.start_line, 1);
    }
}
