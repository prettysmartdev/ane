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
    let mut target_ranges =
        apply_positional(query, buffer, &scope_range, &component_range, buffer_name)?;

    if query.action == Action::Jump && query.positional == Positional::Outside {
        target_ranges = match query.component {
            Component::Beginning => vec![position_before_scope(buffer, &scope_range)],
            Component::End => vec![position_after_scope(buffer, &scope_range)],
            _ => target_ranges,
        };
    }

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
        Scope::Variable => resolve_variable_scope(query, buffer_name, buffer, lsp),
        Scope::Struct => resolve_lsp_scope(
            query,
            buffer_name,
            buffer,
            lsp,
            &[SymbolKind::Struct, SymbolKind::Enum],
        ),
        Scope::Member => resolve_member_scope(query, buffer_name, buffer, lsp),
        Scope::Delimiter => resolve_delimiter_scope(query, buffer, buffer_name),
    }
}

fn resolve_line_scope(query: &ChordQuery, buffer: &Buffer, buffer_name: &str) -> Result<TextRange> {
    let base_line = match query
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

    let line = match query.positional {
        Positional::Next => {
            let next = base_line + 1;
            if next >= buffer.line_count() {
                return Err(ChordError::resolve(
                    buffer_name,
                    format!(
                        "no next line: cursor is on line {base_line} (file has {} lines)",
                        buffer.line_count()
                    ),
                )
                .into());
            }
            next
        }
        Positional::Previous => {
            if base_line == 0 {
                return Err(ChordError::resolve(
                    buffer_name,
                    "no previous line: cursor is already on line 0",
                )
                .into());
            }
            base_line - 1
        }
        _ => base_line,
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
        if matches!(query.scope, Scope::Variable) {
            if let Some(sym) = find_symbol_on_line_by_kind(&symbols, line, target_kinds) {
                return Ok(symbol_to_range(&sym.range));
            }
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

fn resolve_variable_scope(
    query: &ChordQuery,
    buffer_name: &str,
    buffer: &Buffer,
    lsp: &mut LspEngine,
) -> Result<TextRange> {
    let target_kinds = &[SymbolKind::Variable, SymbolKind::Const];
    let path = Path::new(buffer_name);

    if query.args.target_name.is_some() {
        return resolve_lsp_scope(query, buffer_name, buffer, lsp, target_kinds);
    }

    let (line, col) = query.args.cursor_pos.ok_or_else(|| {
        ChordError::resolve(
            buffer_name,
            "variable scope requires a cursor position or target name",
        )
    })?;

    if let Ok(symbols) = lsp.document_symbols(path) {
        if matches!(query.positional, Positional::Next | Positional::Previous) {
            if let Some(sym) =
                find_neighbor_symbol(&symbols, line, col, target_kinds, query.positional)
            {
                return Ok(symbol_to_range(&sym.range));
            }
            return Err(ChordError::resolve(
                buffer_name,
                format!(
                    "no {} variable found from cursor ({line}, {col})",
                    if query.positional == Positional::Next {
                        "next"
                    } else {
                        "previous"
                    }
                ),
            )
            .into());
        }

        let var_pos = find_symbol_at_position_by_kind(&symbols, line, col, target_kinds)
            .or_else(|| find_symbol_on_line_by_kind(&symbols, line, target_kinds))
            .map(|sym| (sym.range.start_line, sym.range.start_col));

        if let Some((var_line, var_col)) = var_pos {
            if let Ok(sel) = lsp.selection_range(path, var_line, var_col) {
                if let Some(range) = find_enclosing_declaration(&sel) {
                    return Ok(range);
                }
            }
        }
    }

    resolve_variable_scope_via_selection_range(query, buffer_name, buffer, lsp)
}

fn find_enclosing_declaration(sel: &crate::data::lsp::types::SelectionRange) -> Option<TextRange> {
    // The innermost range is typically a zero-width point at the cursor.
    // Skip it so we compare against the identifier/name range instead.
    let reference =
        if sel.range.start_line == sel.range.end_line && sel.range.start_col == sel.range.end_col {
            sel.parent.as_ref()?
        } else {
            sel
        };
    let inner = &reference.range;
    let mut current = reference;
    while let Some(ref parent) = current.parent {
        let r = &parent.range;
        let wider = (r.start_line < inner.start_line
            || (r.start_line == inner.start_line && r.start_col < inner.start_col))
            || (r.end_line > inner.end_line
                || (r.end_line == inner.end_line && r.end_col > inner.end_col));
        if wider {
            return Some(symbol_to_range(r));
        }
        current = parent;
    }
    None
}

fn resolve_variable_scope_via_selection_range(
    query: &ChordQuery,
    buffer_name: &str,
    buffer: &Buffer,
    lsp: &mut LspEngine,
) -> Result<TextRange> {
    let (line, col) = query.args.cursor_pos.ok_or_else(|| {
        ChordError::resolve(
            buffer_name,
            "variable scope requires a cursor position or target name",
        )
    })?;

    let path = Path::new(buffer_name);
    let sel = lsp
        .selection_range(path, line, col)
        .map_err(|e| ChordError::resolve(buffer_name, format!("LSP selectionRange failed: {e}")))?;

    // Walk the hierarchy and find the smallest range with an interior `=`
    // (i.e. an assignment — language-agnostic signal for variable declarations).
    let mut current = &sel;
    loop {
        let range = symbol_to_range(&current.range);
        if has_interior_assignment(buffer, &range) {
            return Ok(range);
        }
        match current.parent {
            Some(ref parent) => current = parent,
            None => break,
        }
    }

    Err(ChordError::resolve(
        buffer_name,
        format!("no enclosing variable declaration found at cursor ({line}, {col})"),
    )
    .into())
}

fn has_interior_assignment(buffer: &Buffer, range: &TextRange) -> bool {
    let text = extract_range_text(buffer, range);
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if c == '=' && i > 0 && i < chars.len() - 1 {
            let prev = chars[i - 1];
            let next = chars[i + 1];
            let is_compound = matches!(
                prev,
                '!' | '<' | '>' | '=' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^'
            ) || next == '='
                || next == '>';
            if !is_compound {
                return true;
            }
        }
    }
    false
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
        Component::Contents => resolve_contents_component(query, buffer, scope_range, buffer_name),
        Component::End => Ok(TextRange::point(scope_range.end_line, scope_range.end_col)),
        Component::Self_ => Ok(*scope_range),
        Component::Name => resolve_name_component(query, buffer, buffer_name, lsp, scope_range),
        Component::Value => resolve_value_component(query, buffer, scope_range, buffer_name),
        Component::Parameters => resolve_parameters_component(buffer, scope_range, buffer_name),
        Component::Arguments => {
            resolve_arguments_component(query, buffer, scope_range, buffer_name)
        }
    }
}

fn resolve_name_component(
    query: &ChordQuery,
    buffer: &Buffer,
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

    if query.scope == Scope::Delimiter {
        return Ok(TextRange {
            start_line: scope_range.start_line,
            start_col: scope_range.start_col,
            end_line: scope_range.start_line,
            end_col: scope_range.start_col + 1,
        });
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
            return Ok(symbol_name_range(sym));
        }
        return Err(ChordError::resolve(
            buffer_name,
            format!("symbol '{name}' not found for Name component"),
        )
        .into());
    }

    if let Some((line, col)) = query.args.cursor_pos {
        let target_kinds = scope_to_symbol_kinds(query.scope);
        if !target_kinds.is_empty() {
            if let Some(sym) = find_symbol_in_range(&symbols, scope_range, &target_kinds) {
                return Ok(symbol_name_range(sym));
            }
        }

        if let Some(sym) = find_innermost_symbol(&symbols, line, col) {
            if target_kinds.is_empty() || matches_kind(&sym.kind, &target_kinds) {
                return Ok(symbol_name_range(sym));
            }
        }

        if query.scope == Scope::Variable {
            if let Some(range) = extract_variable_name_from_text(buffer, scope_range) {
                return Ok(range);
            }
        }

        return Err(ChordError::resolve(
            buffer_name,
            format!("no matching symbol at cursor ({line}, {col}) for Name component"),
        )
        .into());
    }

    Err(ChordError::resolve(
        buffer_name,
        "Name component requires either a target name or cursor position",
    )
    .into())
}

fn scope_to_symbol_kinds(scope: Scope) -> Vec<SymbolKind> {
    match scope {
        Scope::Function => vec![SymbolKind::Function, SymbolKind::Method],
        Scope::Variable => vec![SymbolKind::Variable, SymbolKind::Const],
        Scope::Struct => vec![SymbolKind::Struct, SymbolKind::Enum],
        Scope::Member | Scope::Line | Scope::Buffer | Scope::Delimiter => vec![],
    }
}

fn find_symbol_in_range<'a>(
    symbols: &'a [DocumentSymbol],
    range: &TextRange,
    kinds: &[SymbolKind],
) -> Option<&'a DocumentSymbol> {
    for sym in symbols {
        if matches_kind(&sym.kind, kinds) && symbol_within_range(&sym.range, range) {
            return Some(sym);
        }
        if let Some(found) = find_symbol_in_range(&sym.children, range, kinds) {
            return Some(found);
        }
    }
    None
}

fn symbol_within_range(
    sym_range: &crate::data::lsp::types::SymbolRange,
    outer: &TextRange,
) -> bool {
    let after_start = sym_range.start_line > outer.start_line
        || (sym_range.start_line == outer.start_line && sym_range.start_col >= outer.start_col);
    let before_end = sym_range.end_line < outer.end_line
        || (sym_range.end_line == outer.end_line && sym_range.end_col <= outer.end_col);
    after_start && before_end
}

fn extract_variable_name_from_text(buffer: &Buffer, scope_range: &TextRange) -> Option<TextRange> {
    let text = extract_range_text(buffer, scope_range);
    let keywords = ["let", "const", "static", "mut"];
    let mut pos = 0;
    let chars: Vec<char> = text.chars().collect();
    loop {
        while pos < chars.len() && !chars[pos].is_alphanumeric() && chars[pos] != '_' {
            pos += 1;
        }
        if pos >= chars.len() {
            return None;
        }
        let start = pos;
        while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') {
            pos += 1;
        }
        let word: String = chars[start..pos].iter().collect();
        if !keywords.contains(&word.as_str()) {
            let abs_col = if scope_range.start_line == scope_range.end_line {
                scope_range.start_col + start
            } else {
                let newlines_before = text
                    [..chars[..start].iter().map(|c| c.len_utf8()).sum::<usize>()]
                    .matches('\n')
                    .count();
                if newlines_before == 0 {
                    scope_range.start_col + start
                } else {
                    start
                        - text[..chars[..start].iter().map(|c| c.len_utf8()).sum::<usize>()]
                            .rfind('\n')
                            .map(|i| i + 1)
                            .unwrap_or(0)
                }
            };
            return Some(TextRange {
                start_line: scope_range.start_line,
                start_col: abs_col,
                end_line: scope_range.start_line,
                end_col: abs_col + (pos - start),
            });
        }
    }
}

fn resolve_value_component(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    match query.scope {
        Scope::Variable => find_assignment_rhs(buffer, scope_range, buffer_name),
        Scope::Member => find_member_value(buffer, scope_range, buffer_name),
        _ => Err(ChordError::resolve(
            buffer_name,
            format!("Value component is not valid for {} scope", query.scope),
        )
        .into()),
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
        Positional::After => {
            let (end_line, end_col) = if query.component == Component::Self_ {
                let last = buffer.line_count().saturating_sub(1);
                (
                    last,
                    buffer
                        .lines
                        .get(last)
                        .map(|l| line_char_count(l))
                        .unwrap_or(0),
                )
            } else {
                (scope_range.end_line, scope_range.end_col)
            };
            Ok(vec![TextRange {
                start_line: component_range.end_line,
                start_col: component_range.end_col,
                end_line,
                end_col,
            }])
        }
        Positional::Before => {
            let (start_line, start_col) = if query.component == Component::Self_ {
                (0, 0)
            } else {
                (scope_range.start_line, scope_range.start_col)
            };
            Ok(vec![TextRange {
                start_line,
                start_col,
                end_line: component_range.start_line,
                end_col: component_range.start_col,
            }])
        }
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
        Positional::To => {
            let cursor = query.args.cursor_pos.ok_or_else(|| {
                ChordError::resolve(buffer_name, "'To' positional requires a cursor position")
            })?;
            Ok(vec![TextRange {
                start_line: cursor.0,
                start_col: cursor.1,
                end_line: component_range.end_line,
                end_col: component_range.end_col,
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

/// Position just before the scope: end of the previous line, or (0, 0) if the
/// scope already starts at the top of the buffer. Used by Jump+Outside+Beginning.
fn position_before_scope(buffer: &Buffer, scope: &TextRange) -> TextRange {
    if scope.start_line == 0 {
        return TextRange::point(0, 0);
    }
    let prev = scope.start_line - 1;
    let col = buffer
        .lines
        .get(prev)
        .map(|l| line_char_count(l))
        .unwrap_or(0);
    TextRange::point(prev, col)
}

/// Position just after the scope: start of the next line, or end-of-scope if
/// the scope already ends on the buffer's last line. Used by Jump+Outside+End.
fn position_after_scope(buffer: &Buffer, scope: &TextRange) -> TextRange {
    let last = buffer.line_count().saturating_sub(1);
    if scope.end_line >= last {
        return TextRange::point(scope.end_line, scope.end_col);
    }
    TextRange::point(scope.end_line + 1, 0)
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
        Action::Jump => {
            let cursor = match query.positional {
                Positional::To | Positional::Until | Positional::Before => CursorPosition {
                    line: target_range.end_line,
                    col: target_range.end_col,
                },
                _ => CursorPosition {
                    line: target_range.start_line,
                    col: target_range.start_col,
                },
            };
            (Some(cursor), Some(EditorMode::Chord))
        }
    }
}

// --- Helper functions ---

fn resolve_contents_component(
    query: &ChordQuery,
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    match query.scope {
        Scope::Function | Scope::Struct => find_brace_range(buffer, scope_range, buffer_name),
        Scope::Delimiter => {
            // TextRange end_col is exclusive: scope_range.end_col points one
            // past the closing delimiter, so end_col - 1 is the position of
            // the closing delimiter and thus the exclusive end of "contents".
            let end_col = if scope_range.end_col > 0 {
                scope_range.end_col - 1
            } else {
                0
            };
            Ok(TextRange {
                start_line: scope_range.start_line,
                start_col: scope_range.start_col + 1,
                end_line: scope_range.end_line,
                end_col,
            })
        }
        _ => Err(ChordError::resolve(
            buffer_name,
            format!("Contents component is not valid for {} scope", query.scope),
        )
        .into()),
    }
}

fn symbol_name_range(sym: &DocumentSymbol) -> TextRange {
    if let Some(ref sr) = sym.selection_range {
        symbol_to_range(sr)
    } else {
        symbol_to_range(&sym.range)
    }
}

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

fn find_symbol_on_line_by_kind<'a>(
    symbols: &'a [DocumentSymbol],
    line: usize,
    kinds: &[SymbolKind],
) -> Option<&'a DocumentSymbol> {
    for sym in symbols {
        if matches_kind(&sym.kind, kinds) && sym.range.start_line == line {
            return Some(sym);
        }
        if let Some(found) = find_symbol_on_line_by_kind(&sym.children, line, kinds) {
            return Some(found);
        }
    }
    None
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
fn find_assignment_in_text(
    text: &str,
    base_line: usize,
    base_col: usize,
    end_line: usize,
    end_col: usize,
) -> Option<TextRange> {
    let chars: Vec<char> = text.chars().collect();
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
                let lines_before = text[..byte_offset].matches('\n').count();
                let line_start = text[..byte_offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
                let col_in_line = text[line_start..byte_offset].chars().count();
                let abs_line = base_line + lines_before;
                let abs_col = if lines_before == 0 {
                    base_col + col_in_line
                } else {
                    col_in_line
                };
                return Some(TextRange {
                    start_line: abs_line,
                    start_col: abs_col + 1,
                    end_line,
                    end_col,
                });
            }
        }
        byte_offset += c.len_utf8();
        char_idx += 1;
    }
    None
}

fn find_assignment_rhs(
    buffer: &Buffer,
    scope_range: &TextRange,
    buffer_name: &str,
) -> Result<TextRange> {
    let content = extract_range_text(buffer, scope_range);
    if let Some(range) = find_assignment_in_text(
        &content,
        scope_range.start_line,
        scope_range.start_col,
        scope_range.end_line,
        scope_range.end_col,
    ) {
        return Ok(range);
    }

    // The scope range may only cover the variable name (e.g. from documentSymbol).
    // Expand to the full line and retry.
    if scope_range.start_line == scope_range.end_line {
        let line_idx = scope_range.start_line;
        if let Some(line) = buffer.lines.get(line_idx) {
            let line_end = line_char_count(line);
            if let Some(mut range) = find_assignment_in_text(line, line_idx, 0, line_idx, line_end)
            {
                range.end_line = line_idx;
                range.end_col = line_end;
                return Ok(range);
            }
        }
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

fn resolve_delimiter_scope(
    query: &ChordQuery,
    buffer: &Buffer,
    buffer_name: &str,
) -> Result<TextRange> {
    let (line, col) = query.args.cursor_pos.ok_or_else(|| {
        ChordError::resolve(buffer_name, "Delimiter scope requires a cursor position")
    })?;
    find_innermost_delimiter(buffer, line, col, buffer_name)
}

fn find_innermost_delimiter(
    buffer: &Buffer,
    cursor_line: usize,
    cursor_col: usize,
    buffer_name: &str,
) -> Result<TextRange> {
    let paired = [('(', ')'), ('{', '}'), ('[', ']')];
    let self_paired = ['"', '\'', '`'];

    let mut candidates: Vec<TextRange> = Vec::new();

    for &(open, close) in &paired {
        if let Some(range) = find_paired_delimiter(buffer, cursor_line, cursor_col, open, close) {
            candidates.push(range);
        }
    }

    for &delim in &self_paired {
        if let Some(range) = find_self_paired_delimiter(buffer, cursor_line, cursor_col, delim) {
            candidates.push(range);
        }
    }

    candidates
        .into_iter()
        .min_by_key(delimiter_span_size)
        .ok_or_else(|| {
            ChordError::resolve(
                buffer_name,
                "no enclosing delimiter found at cursor position",
            )
            .into()
        })
}

fn delimiter_span_size(range: &TextRange) -> (usize, usize) {
    if range.start_line == range.end_line {
        (0, range.end_col - range.start_col)
    } else {
        (range.end_line - range.start_line, range.end_col)
    }
}

fn find_paired_delimiter(
    buffer: &Buffer,
    cursor_line: usize,
    cursor_col: usize,
    open: char,
    close: char,
) -> Option<TextRange> {
    let mut best: Option<TextRange> = None;

    let mut depth: i32 = 0;
    let mut candidates: Vec<(usize, usize)> = Vec::new();

    let lines = &buffer.lines;

    for line_idx in (0..=cursor_line.min(lines.len().saturating_sub(1))).rev() {
        let line_chars: Vec<char> = lines[line_idx].chars().collect();
        let start_col = if line_idx == cursor_line {
            cursor_col.min(line_chars.len())
        } else {
            line_chars.len()
        };

        for col in (0..start_col).rev() {
            let ch = line_chars[col];
            if ch == close {
                depth += 1;
            } else if ch == open {
                if depth > 0 {
                    depth -= 1;
                } else {
                    candidates.push((line_idx, col));
                }
            }
        }
    }

    for (open_line, open_col) in candidates {
        let mut d: i32 = 0;
        let mut found = false;
        'outer: for (line_idx, line) in lines.iter().enumerate().skip(open_line) {
            let line_chars: Vec<char> = line.chars().collect();
            let from = if line_idx == open_line { open_col } else { 0 };
            for (col, &ch) in line_chars.iter().enumerate().skip(from) {
                if ch == open {
                    d += 1;
                } else if ch == close {
                    d -= 1;
                    if d == 0 {
                        let encloses = (line_idx > cursor_line)
                            || (line_idx == cursor_line && col >= cursor_col);
                        if encloses {
                            let range = TextRange {
                                start_line: open_line,
                                start_col: open_col,
                                end_line: line_idx,
                                end_col: col + 1,
                            };
                            if best.as_ref().map_or(true, |b| {
                                delimiter_span_size(&range) < delimiter_span_size(b)
                            }) {
                                best = Some(range);
                            }
                        }
                        found = true;
                        break 'outer;
                    }
                }
            }
        }
        if !found {
            continue;
        }
        if best.is_some() {
            break;
        }
    }

    best
}

/// A character at `col` is escaped iff the number of consecutive backslashes
/// immediately preceding it is odd. (Two `\\` consume each other.)
fn is_escaped_at(line_chars: &[char], col: usize) -> bool {
    let mut count = 0usize;
    let mut i = col;
    while i > 0 && line_chars[i - 1] == '\\' {
        count += 1;
        i -= 1;
    }
    count % 2 == 1
}

fn find_self_paired_delimiter(
    buffer: &Buffer,
    cursor_line: usize,
    cursor_col: usize,
    delim: char,
) -> Option<TextRange> {
    let lines = &buffer.lines;

    let mut count = 0usize;

    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(cursor_line.min(lines.len().saturating_sub(1)) + 1)
    {
        let line_chars: Vec<char> = line.chars().collect();
        let end = if line_idx == cursor_line {
            cursor_col.min(line_chars.len())
        } else {
            line_chars.len()
        };
        let mut last_was_backslash = false;
        for &ch in line_chars.iter().take(end) {
            if ch == delim && !last_was_backslash {
                count += 1;
            }
            last_was_backslash = ch == '\\' && !last_was_backslash;
        }
    }

    if count % 2 == 0 {
        return None;
    }

    let mut open_pos: Option<(usize, usize)> = None;
    'backward: for line_idx in (0..=cursor_line.min(lines.len().saturating_sub(1))).rev() {
        let line_chars: Vec<char> = lines[line_idx].chars().collect();
        let start = if line_idx == cursor_line {
            cursor_col.min(line_chars.len())
        } else {
            line_chars.len()
        };
        for col in (0..start).rev() {
            let ch = line_chars[col];
            if ch == delim && !is_escaped_at(&line_chars, col) {
                open_pos = Some((line_idx, col));
                break 'backward;
            }
        }
    }

    let (open_line, open_col) = open_pos?;

    let mut close_pos: Option<(usize, usize)> = None;
    'forward: for (line_idx, line) in lines.iter().enumerate().skip(cursor_line) {
        let line_chars: Vec<char> = line.chars().collect();
        let from = if line_idx == cursor_line {
            cursor_col.min(line_chars.len())
        } else {
            0
        };
        let mut prev_backslash = if from > 0 {
            line_chars[from - 1] == '\\'
        } else {
            false
        };
        for (col, &ch) in line_chars.iter().enumerate().skip(from) {
            if ch == delim && !prev_backslash {
                close_pos = Some((line_idx, col));
                break 'forward;
            }
            prev_backslash = ch == '\\' && !prev_backslash;
        }
    }

    let (close_line, close_col) = close_pos?;

    Some(TextRange {
        start_line: open_line,
        start_col: open_col,
        end_line: close_line,
        end_col: close_col + 1,
    })
}

fn shrink_range(buffer: &Buffer, range: &TextRange) -> TextRange {
    let content = extract_range_text(buffer, range);

    let first_char = content.chars().next();
    let last_char = content.chars().last();

    let is_delimited = matches!(
        (first_char, last_char),
        (Some('('), Some(')'))
            | (Some('{'), Some('}'))
            | (Some('['), Some(']'))
            | (Some('"'), Some('"'))
            | (Some('\''), Some('\''))
            | (Some('`'), Some('`'))
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
    use crate::data::lsp::types::{DocumentSymbol, SelectionRange, SymbolKind, SymbolRange};

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
            selection_range: None,
            children: vec![],
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn sym_sel(
        name: &str,
        kind: SymbolKind,
        sl: usize,
        sc: usize,
        el: usize,
        ec: usize,
        sel_sl: usize,
        sel_sc: usize,
        sel_el: usize,
        sel_ec: usize,
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
            selection_range: Some(SymbolRange {
                start_line: sel_sl,
                start_col: sel_sc,
                end_line: sel_el,
                end_col: sel_ec,
            }),
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
            selection_range: None,
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

    #[test]
    fn line_scope_next_positional_resolves_next_line() {
        let buffer = buf(&["first", "second", "third"]);
        let q = query(
            Action::Jump,
            Positional::Next,
            Scope::Line,
            Component::End,
            None,
            None,
            Some((0, 0)),
        );
        let range = resolve_line_scope(&q, &buffer, "/buf").unwrap();
        assert_eq!(range.start_line, 1);
        assert_eq!(range.end_line, 1);
        assert_eq!(range.end_col, 6);
    }

    #[test]
    fn line_scope_previous_positional_resolves_previous_line() {
        let buffer = buf(&["first", "second", "third"]);
        let q = query(
            Action::Jump,
            Positional::Previous,
            Scope::Line,
            Component::Beginning,
            None,
            None,
            Some((2, 3)),
        );
        let range = resolve_line_scope(&q, &buffer, "/buf").unwrap();
        assert_eq!(range.start_line, 1);
        assert_eq!(range.end_line, 1);
    }

    #[test]
    fn line_scope_next_at_last_line_errors() {
        let buffer = buf(&["first", "second"]);
        let q = query(
            Action::Jump,
            Positional::Next,
            Scope::Line,
            Component::End,
            None,
            None,
            Some((1, 0)),
        );
        assert!(resolve_line_scope(&q, &buffer, "/buf").is_err());
    }

    #[test]
    fn line_scope_previous_at_first_line_errors() {
        let buffer = buf(&["first", "second"]);
        let q = query(
            Action::Jump,
            Positional::Previous,
            Scope::Line,
            Component::Beginning,
            None,
            None,
            Some((0, 0)),
        );
        assert!(resolve_line_scope(&q, &buffer, "/buf").is_err());
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

    #[test]
    fn find_assignment_rhs_expands_to_line_when_scope_is_name_only() {
        let buffer = buf(&["    let asdf = dude();"]);
        // Scope covers only the variable name "asdf" (cols 8-12)
        let scope = TextRange {
            start_line: 0,
            start_col: 8,
            end_line: 0,
            end_col: 12,
        };
        let range = find_assignment_rhs(&buffer, &scope, "/buf").unwrap();
        assert_eq!(range.start_line, 0);
        assert_eq!(range.start_col, 14); // after the `=` at col 13
        assert_eq!(range.end_col, 22); // end of line
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

    // --- cifn: ChangeInFunctionName ---

    #[test]
    fn cifn_with_selection_range_targets_identifier() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() { 42 }"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        // selection_range covers just "foo" (cols 3..6)
        let mut lsp = mock_lsp(
            path,
            vec![sym_sel(
                "foo",
                SymbolKind::Function,
                0,
                0,
                0,
                15,
                0,
                3,
                0,
                6,
            )],
        );

        let q = query(
            Action::Change,
            Positional::Inside,
            Scope::Function,
            Component::Name,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let target = res.target_ranges.first().unwrap();
        assert_eq!(target.start_line, 0);
        assert_eq!(target.start_col, 3);
        assert_eq!(target.end_col, 6);
    }

    // --- cifc: ChangeInsideFunctionContents ---

    #[test]
    fn cifc_resolves_to_brace_contents() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() { 42 }"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 0, 0, 0, 15)]);

        let q = query(
            Action::Change,
            Positional::Inside,
            Scope::Function,
            Component::Contents,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let target = res.target_ranges.first().unwrap();
        // Contents for Function finds brace block {..}, Inside shrinks past braces
        // "fn foo() { 42 }"  braces at 9..15, inside = 10..14
        assert_eq!(target.start_line, 0);
        assert_eq!(target.start_col, 10);
        assert_eq!(target.end_col, 14);
    }

    #[test]
    fn cifc_multiline_resolves_contents() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {", "    42", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 0, 0, 2, 1)]);

        let q = query(
            Action::Change,
            Positional::Inside,
            Scope::Function,
            Component::Contents,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let target = res.target_ranges.first().unwrap();
        // brace block: (0,9)..(2,1), inside shrinks to (0,10)..(2,0)
        assert_eq!(target.start_line, 0);
        assert_eq!(target.start_col, 10);
        assert_eq!(target.end_line, 2);
        assert_eq!(target.end_col, 0);
    }

    // --- cbfs: ChangeBeforeFunctionSelf ---

    #[test]
    fn cbfs_targets_text_before_function() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["use std::io;", "", "fn foo() { 42 }"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 2, 0, 2, 15)]);

        let q = query(
            Action::Change,
            Positional::Before,
            Scope::Function,
            Component::Self_,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let target = res.target_ranges.first().unwrap();
        // Before Self_ uses buffer start (0,0) to function start (2,0)
        assert_eq!(target.start_line, 0);
        assert_eq!(target.start_col, 0);
        assert_eq!(target.end_line, 2);
        assert_eq!(target.end_col, 0);
    }

    #[test]
    fn after_function_self_targets_text_after_function() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {}", "", "fn bar() {}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(
            path,
            vec![
                sym("foo", SymbolKind::Function, 0, 0, 0, 11),
                sym("bar", SymbolKind::Function, 2, 0, 2, 11),
            ],
        );

        let q = query(
            Action::Change,
            Positional::After,
            Scope::Function,
            Component::Self_,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let target = res.target_ranges.first().unwrap();
        // After Self_ uses function end (0,11) to buffer end (2,11)
        assert_eq!(target.start_line, 0);
        assert_eq!(target.start_col, 11);
        assert_eq!(target.end_line, 2);
        assert_eq!(target.end_col, 11);
    }

    // --- variable scope via selectionRange fallback ---

    fn sel_range(
        sl: usize,
        sc: usize,
        el: usize,
        ec: usize,
        parent: Option<SelectionRange>,
    ) -> SelectionRange {
        SelectionRange {
            range: SymbolRange {
                start_line: sl,
                start_col: sc,
                end_line: el,
                end_col: ec,
            },
            parent: parent.map(Box::new),
        }
    }

    #[test]
    fn variable_scope_selection_range_simple_let() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let x = 42;", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("main", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        // selectionRange hierarchy: x(1,8-9) → let x = 42;(1,4-15) → block(0,10-2,1)
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 15, Some(block));
        let ident = sel_range(1, 8, 1, 9, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 8, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Self_,
            None,
            None,
            Some((1, 8)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_line, 1);
        assert_eq!(res.scope_range.end_col, 15);
    }

    #[test]
    fn variable_scope_selection_range_value_component() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let name = \"hello\";", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("main", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 22, Some(block));
        let ident = sel_range(1, 8, 1, 12, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 8, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Value,
            None,
            None,
            Some((1, 8)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.component_range.start_col, 14);
    }

    #[test]
    fn variable_scope_selection_range_const() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["const MAX: usize = 100;"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = mock_lsp(path, vec![]);
        // selectionRange: MAX(0,6-9) → const MAX: usize = 100;(0,0-23) → file
        let file = sel_range(0, 0, 0, 23, None);
        let const_stmt = sel_range(0, 0, 0, 23, Some(file));
        let ident = sel_range(0, 6, 0, 9, Some(const_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 0, 6, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Self_,
            None,
            None,
            Some((0, 6)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 0);
        assert_eq!(res.scope_range.start_col, 0);
        assert_eq!(res.scope_range.end_col, 23);
    }

    #[test]
    fn variable_scope_selection_range_multiline() {
        let path = "/test/file.rs";
        let buffer = named_buf(
            path,
            &[
                "fn main() {",
                "    let v = vec![",
                "        1, 2, 3,",
                "    ];",
                "}",
            ],
        );
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("main", SymbolKind::Function, 0, 0, 4, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        // selectionRange: v(1,8-9) → let v = vec![...];(1,4-3,6) → block
        let block = sel_range(0, 10, 4, 1, None);
        let let_stmt = sel_range(1, 4, 3, 6, Some(block));
        let ident = sel_range(1, 8, 1, 9, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 8, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Self_,
            None,
            None,
            Some((1, 8)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_line, 3);
        assert_eq!(res.scope_range.end_col, 6);
    }

    #[test]
    fn variable_scope_selection_range_cursor_on_keyword() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let params = some_value;", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("main", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        // Cursor on 'let' keyword: let(1,4-7) → let params = some_value;(1,4-27) → block
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 27, Some(block));
        let keyword = sel_range(1, 4, 1, 7, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 5, keyword);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Self_,
            None,
            None,
            Some((1, 5)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_line, 1);
        assert_eq!(res.scope_range.end_col, 27);
    }

    #[test]
    fn variable_scope_prefers_selection_range_over_narrow_document_symbol() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let asdf = dude();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        // documentSymbol returns variable with name-only range (rust-analyzer behavior)
        let var_sym = sym("asdf", SymbolKind::Variable, 1, 8, 1, 12);
        let main_fn = sym_with_children("main", SymbolKind::Function, 0, 0, 2, 1, vec![var_sym]);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        // selectionRange gives full declaration
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 21, Some(block));
        let ident = sel_range(1, 8, 1, 12, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 9, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Value,
            None,
            None,
            Some((1, 9)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_col, 21);
        assert_eq!(res.component_range.start_col, 14);
    }

    #[test]
    fn variable_scope_selection_range_cursor_on_value() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let x = 42;", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("main", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        // Cursor on '42': 42(1,12-14) → let x = 42;(1,4-15) → block
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 15, Some(block));
        let literal = sel_range(1, 12, 1, 14, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 12, literal);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Value,
            None,
            None,
            Some((1, 12)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_line, 1);
        assert_eq!(res.scope_range.end_col, 15);
    }

    #[test]
    fn variable_name_component_finds_variable_not_function() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let asdf = dude();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        // documentSymbol returns variable as child of function (rust-analyzer behavior)
        let var_sym = sym("asdf", SymbolKind::Variable, 1, 8, 1, 12);
        let main_fn = sym_with_children("main", SymbolKind::Function, 0, 0, 2, 1, vec![var_sym]);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 21, Some(block));
        let ident = sel_range(1, 8, 1, 12, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 9, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Name,
            None,
            None,
            Some((1, 9)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.component_range.start_line, 1);
        assert_eq!(res.component_range.start_col, 8);
        assert_eq!(res.component_range.end_col, 12);
    }

    #[test]
    fn variable_name_component_cursor_on_value_side() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let asdf = dude();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        // documentSymbol has variable with name-only range
        let var_sym = sym("asdf", SymbolKind::Variable, 1, 8, 1, 12);
        let main_fn = sym_with_children("main", SymbolKind::Function, 0, 0, 2, 1, vec![var_sym]);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        // cursor on 'dude' — selectionRange for scope resolution
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 21, Some(block));
        let call = sel_range(1, 15, 1, 21, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 15, call);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Name,
            None,
            None,
            Some((1, 15)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // Scope should be the full let statement, not dude()
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_col, 21);
        // Name should be "asdf", not "main" or "dude"
        assert_eq!(res.component_range.start_col, 8);
        assert_eq!(res.component_range.end_col, 12);
    }

    #[test]
    fn variable_name_text_fallback_when_no_symbol() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn main() {", "    let asdf = dude();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        // documentSymbol has NO variable symbol — only function
        let main_fn = sym("main", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        let block = sel_range(0, 10, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 21, Some(block));
        let ident = sel_range(1, 8, 1, 12, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 9, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Name,
            None,
            None,
            Some((1, 9)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.component_range.start_col, 8);
        assert_eq!(res.component_range.end_col, 12);
    }

    #[test]
    fn variable_name_text_fallback_const() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["const MAX: usize = 100;"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![]);
        let file = sel_range(0, 0, 0, 23, None);
        let const_stmt = sel_range(0, 0, 0, 23, Some(file));
        let ident = sel_range(0, 6, 0, 9, Some(const_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 0, 7, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Name,
            None,
            None,
            Some((0, 7)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.component_range.start_col, 6);
        assert_eq!(res.component_range.end_col, 9);
    }

    #[test]
    fn variable_name_text_fallback_let_mut() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn f() {", "    let mut count = 0;", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("f", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        let block = sel_range(0, 7, 2, 1, None);
        let let_stmt = sel_range(1, 4, 1, 21, Some(block));
        let ident = sel_range(1, 12, 1, 17, Some(let_stmt));
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 13, ident);

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Name,
            None,
            None,
            Some((1, 13)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // "count" is at cols 12-17
        assert_eq!(res.component_range.start_col, 12);
        assert_eq!(res.component_range.end_col, 17);
    }

    // --- Real rust-analyzer selectionRange hierarchy tests ---
    // These mirror the actual hierarchy from rust-analyzer (verified empirically):
    //   cursor_point → ident/keyword → [intermediate exprs] → let-statement → block → fn → file

    fn real_sel_hierarchy_at_let(line: usize) -> SelectionRange {
        // cursor on 'let' at (line,4):
        // (l,4)..(l,4) → (l,4)..(l,7) [let kw] → (l,4)..(l,23) [stmt] → block → fn → file
        let file = sel_range(0, 0, 3, 0, None);
        let func = sel_range(0, 0, 2, 1, Some(file));
        let block = sel_range(0, 12, 2, 1, Some(func));
        let stmt = sel_range(line, 4, line, 23, Some(block));
        let keyword = sel_range(line, 4, line, 7, Some(stmt));
        sel_range(line, 4, line, 4, Some(keyword))
    }

    fn real_sel_hierarchy_at_name(line: usize) -> SelectionRange {
        // cursor on 'cmon' at (line,12):
        // (l,8)..(l,8) → (l,8)..(l,12) [ident] → (l,4)..(l,23) [stmt] → block → fn → file
        let file = sel_range(0, 0, 3, 0, None);
        let func = sel_range(0, 0, 2, 1, Some(file));
        let block = sel_range(0, 12, 2, 1, Some(func));
        let stmt = sel_range(line, 4, line, 23, Some(block));
        let ident = sel_range(line, 8, line, 12, Some(stmt));
        sel_range(line, 8, line, 8, Some(ident))
    }

    fn real_sel_hierarchy_at_rhs(line: usize) -> SelectionRange {
        // cursor on 'hello' at (line,15):
        // (l,15)..(l,15) → (l,15)..(l,20) [hello ident] → (l,15)..(l,22) [hello() call]
        //   → (l,4)..(l,23) [stmt] → block → fn → file
        let file = sel_range(0, 0, 3, 0, None);
        let func = sel_range(0, 0, 2, 1, Some(file));
        let block = sel_range(0, 12, 2, 1, Some(func));
        let stmt = sel_range(line, 4, line, 23, Some(block));
        let call = sel_range(line, 15, line, 22, Some(stmt));
        let ident = sel_range(line, 15, line, 20, Some(call));
        sel_range(line, 15, line, 15, Some(ident))
    }

    #[test]
    fn real_lsp_variable_scope_cursor_on_let() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn on_stderr() {", "    let cmon = hello();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("on_stderr", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 4, real_sel_hierarchy_at_let(1));

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Self_,
            None,
            None,
            Some((1, 4)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_line, 1);
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_col, 23);
    }

    #[test]
    fn real_lsp_variable_scope_cursor_on_name() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn on_stderr() {", "    let cmon = hello();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("on_stderr", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 8, real_sel_hierarchy_at_name(1));

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Self_,
            None,
            None,
            Some((1, 8)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_col, 23);
    }

    #[test]
    fn real_lsp_variable_scope_cursor_on_rhs() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn on_stderr() {", "    let cmon = hello();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("on_stderr", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 15, real_sel_hierarchy_at_rhs(1));

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Self_,
            None,
            None,
            Some((1, 15)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_col, 4);
        assert_eq!(res.scope_range.end_col, 23);
    }

    #[test]
    fn real_lsp_variable_name_cursor_on_let() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn on_stderr() {", "    let cmon = hello();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("on_stderr", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 4, real_sel_hierarchy_at_let(1));

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Name,
            None,
            None,
            Some((1, 4)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // Name should be "cmon" at cols 8-12
        assert_eq!(res.component_range.start_col, 8);
        assert_eq!(res.component_range.end_col, 12);
    }

    #[test]
    fn real_lsp_variable_name_cursor_on_rhs() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn on_stderr() {", "    let cmon = hello();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("on_stderr", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 15, real_sel_hierarchy_at_rhs(1));

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Name,
            None,
            None,
            Some((1, 15)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // Name should be "cmon" at cols 8-12, not "hello"
        assert_eq!(res.component_range.start_col, 8);
        assert_eq!(res.component_range.end_col, 12);
    }

    #[test]
    fn real_lsp_variable_value_cursor_on_let() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn on_stderr() {", "    let cmon = hello();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("on_stderr", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 4, real_sel_hierarchy_at_let(1));

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Value,
            None,
            None,
            Some((1, 4)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // Value starts after '=' at col 14
        assert_eq!(res.component_range.start_col, 14);
        assert_eq!(res.component_range.end_col, 23);
    }

    #[test]
    fn real_lsp_variable_value_cursor_on_rhs() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn on_stderr() {", "    let cmon = hello();", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let main_fn = sym("on_stderr", SymbolKind::Function, 0, 0, 2, 1);
        let mut lsp = mock_lsp(path, vec![main_fn]);
        lsp.inject_test_selection_range(PathBuf::from(path), 1, 15, real_sel_hierarchy_at_rhs(1));

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Variable,
            Component::Value,
            None,
            None,
            Some((1, 15)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.component_range.start_col, 14);
        assert_eq!(res.component_range.end_col, 23);
    }

    // --- work item 0005: Jump / To / Delimiter ---

    // find_innermost_delimiter

    #[test]
    fn find_innermost_delimiter_parens_basic() {
        // "foo(bar, baz)" cursor at col 4 — '(' at 3, ')' at 12, end_col=13
        let buffer = buf(&["foo(bar, baz)"]);
        let result = find_innermost_delimiter(&buffer, 0, 4, "/buf").unwrap();
        assert_eq!(result.start_line, 0);
        assert_eq!(result.start_col, 3);
        assert_eq!(result.end_line, 0);
        assert_eq!(result.end_col, 13);
    }

    #[test]
    fn find_innermost_delimiter_braces() {
        // "if true { x + 1 }" cursor at col 10 — '{' at 8, '}' at 16, end_col=17
        let buffer = buf(&["if true { x + 1 }"]);
        let result = find_innermost_delimiter(&buffer, 0, 10, "/buf").unwrap();
        assert_eq!(result.start_col, 8);
        assert_eq!(result.end_col, 17);
    }

    #[test]
    fn find_innermost_delimiter_double_quotes() {
        // let s = "hello"; cursor at col 10 — '"' at 8, '"' at 14, end_col=15
        let buffer = buf(&[r#"let s = "hello";"#]);
        let result = find_innermost_delimiter(&buffer, 0, 10, "/buf").unwrap();
        assert_eq!(result.start_col, 8);
        assert_eq!(result.end_col, 15);
    }

    #[test]
    fn find_innermost_delimiter_nested_picks_innermost() {
        // "foo({ bar })" cursor at col 6 — inner '{' at 4, '}' at 10, end_col=11
        let buffer = buf(&["foo({ bar })"]);
        let result = find_innermost_delimiter(&buffer, 0, 6, "/buf").unwrap();
        assert_eq!(result.start_col, 4);
        assert_eq!(result.end_col, 11);
    }

    #[test]
    fn find_innermost_delimiter_empty_parens() {
        // "f()" cursor at col 2 — '(' at 1, ')' at 2, end_col=3
        let buffer = buf(&["f()"]);
        let result = find_innermost_delimiter(&buffer, 0, 2, "/buf").unwrap();
        assert_eq!(result.start_col, 1);
        assert_eq!(result.end_col, 3);
    }

    #[test]
    fn find_innermost_delimiter_multi_line_braces() {
        // ["fn f() {", "    x", "}"] cursor at (1,4) — '{' at (0,7), '}' at (2,0), end_col=1
        let buffer = buf(&["fn f() {", "    x", "}"]);
        let result = find_innermost_delimiter(&buffer, 1, 4, "/buf").unwrap();
        assert_eq!(result.start_line, 0);
        assert_eq!(result.start_col, 7);
        assert_eq!(result.end_line, 2);
        assert_eq!(result.end_col, 1);
    }

    #[test]
    fn find_innermost_delimiter_no_delimiter_errors() {
        let buffer = buf(&["abc"]);
        let result = find_innermost_delimiter(&buffer, 0, 1, "/buf");
        assert!(result.is_err());
    }

    #[test]
    fn find_innermost_delimiter_double_backslash_does_not_escape_quote() {
        // Buffer: `"a\\"b"` — chars: " a \ \ " b "  (indices 0..=6)
        // The pair `\\` is an escaped backslash; the next `"` at col 4 is NOT
        // escaped and closes the first quote pair. With cursor at col 2 (on 'a'),
        // the algorithm must pick the [0, 4] pair (5 chars including both `"`s).
        let buffer = buf(&[r#""a\\"b""#]);
        let result = find_innermost_delimiter(&buffer, 0, 2, "/buf").unwrap();
        assert_eq!(result.start_col, 0);
        assert_eq!(result.end_col, 5);
    }

    #[test]
    fn find_innermost_delimiter_cursor_on_close_paren() {
        // "foo(bar)" cursor at col 7 (the ')') — '(' at 3, ')' at 7, end_col=8
        let buffer = buf(&["foo(bar)"]);
        let result = find_innermost_delimiter(&buffer, 0, 7, "/buf").unwrap();
        assert_eq!(result.start_col, 3);
        assert_eq!(result.end_col, 8);
    }

    // Delimiter scope full resolve — Self_ and Contents components

    #[test]
    fn delimiter_scope_self_returns_full_delimiter_range() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["foo(bar, baz)"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = no_lsp();

        // ceds = Change Entire Delimiter Self — cursor inside parens
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Delimiter,
            Component::Self_,
            None,
            None,
            Some((0, 4)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(res.scope_range.start_col, 3);
        assert_eq!(res.scope_range.end_col, 13);
        assert_eq!(res.component_range.start_col, 3);
        assert_eq!(res.component_range.end_col, 13);
    }

    #[test]
    fn delimiter_scope_contents_shrinks_past_open_delimiter() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["foo(bar, baz)"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = no_lsp();

        // cedc = Change Entire Delimiter Contents — contents starts after '(' and ends before ')'
        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Delimiter,
            Component::Contents,
            None,
            None,
            Some((0, 4)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // Contents excludes both delimiters: [4, 12) covers "bar, baz"
        assert_eq!(res.component_range.start_col, 4);
        assert_eq!(res.component_range.end_col, 12);
    }

    #[test]
    fn delimiter_inside_self_on_quotes_strips_both_quote_chars() {
        // `cids` on a string literal must shrink past both `"` chars.
        let path = "/test/file.rs";
        let buffer = named_buf(path, &[r#"let s = "hello";"#]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = no_lsp();

        let q = query(
            Action::Change,
            Positional::Inside,
            Scope::Delimiter,
            Component::Self_,
            None,
            None,
            Some((0, 11)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // `"hello"` lives at [8, 15); stripping both quotes -> [9, 14) covers "hello"
        assert_eq!(res.target_ranges[0].start_col, 9);
        assert_eq!(res.target_ranges[0].end_col, 14);
    }

    #[test]
    fn delimiter_entire_contents_on_braces_excludes_both_braces() {
        // `cedc` on a brace block: Contents covers strictly between `{` and `}`.
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["{ block }"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = no_lsp();

        let q = query(
            Action::Change,
            Positional::Entire,
            Scope::Delimiter,
            Component::Contents,
            None,
            None,
            Some((0, 4)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // "{ block }" -> scope [0, 9). Contents [1, 8) covers " block ".
        assert_eq!(res.component_range.start_col, 1);
        assert_eq!(res.component_range.end_col, 8);
    }

    // Positional::To

    #[test]
    fn positional_to_from_cursor_to_component_end() {
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
            Positional::To,
            Scope::Function,
            Component::Parameters,
            None,
            None,
            Some((0, 2)),
        );
        let result = apply_positional(&q, &buffer, &scope, &comp, "/buf").unwrap();
        assert_eq!(result[0].start_line, 0);
        assert_eq!(result[0].start_col, 2);
        assert_eq!(result[0].end_col, 14);
    }

    #[test]
    fn positional_to_requires_cursor_pos() {
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
            Positional::To,
            Scope::Function,
            Component::Parameters,
            None,
            None,
            None,
        );
        assert!(apply_positional(&q, &buffer, &scope, &comp, "/buf").is_err());
    }

    // Jump action — resolve_cursor_and_mode

    #[test]
    fn cursor_mode_jump_goes_to_edit_mode() {
        let range = TextRange {
            start_line: 3,
            start_col: 7,
            end_line: 5,
            end_col: 0,
        };
        let q = query(
            Action::Jump,
            Positional::Entire,
            Scope::Function,
            Component::Self_,
            None,
            None,
            None,
        );
        let (cursor, mode) = resolve_cursor_and_mode(&q, &range);
        assert_eq!(cursor, Some(CursorPosition { line: 3, col: 7 }));
        assert_eq!(mode, Some(EditorMode::Chord));
    }

    #[test]
    fn jump_entire_function_name_sets_cursor_destination() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {", "    x", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(
            path,
            vec![sym_sel("foo", SymbolKind::Function, 0, 0, 2, 1, 0, 3, 0, 6)],
        );

        let q = query(
            Action::Jump,
            Positional::Entire,
            Scope::Function,
            Component::Name,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert_eq!(
            res.cursor_destination,
            Some(CursorPosition { line: 0, col: 3 })
        );
        assert_eq!(res.mode_after, Some(EditorMode::Chord));
    }

    #[test]
    fn jump_does_not_set_replacement() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 0, 0, 1, 1)]);

        let q = query(
            Action::Jump,
            Positional::Entire,
            Scope::Function,
            Component::Self_,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        assert!(res.replacement.is_none());
        assert!(res.cursor_destination.is_some());
    }

    #[test]
    fn jump_outside_function_beginning_lands_on_previous_line() {
        // Buffer with a function starting on line 1. jofb must jump to end of line 0.
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["// preamble line", "fn foo() {", "    x", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 1, 0, 3, 1)]);

        let q = query(
            Action::Jump,
            Positional::Outside,
            Scope::Function,
            Component::Beginning,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let dest = res.cursor_destination.unwrap();
        assert_eq!(dest.line, 0);
        assert_eq!(dest.col, "// preamble line".chars().count());
        assert_eq!(res.mode_after, Some(EditorMode::Chord));
    }

    #[test]
    fn jump_outside_function_end_lands_on_next_line() {
        // Function ends on line 3; jofe must jump to (4, 0).
        let path = "/test/file.rs";
        let buffer = named_buf(
            path,
            &[
                "// preamble",
                "fn foo() {",
                "    x",
                "}",
                "// trailing line",
            ],
        );
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 1, 0, 3, 1)]);

        let q = query(
            Action::Jump,
            Positional::Outside,
            Scope::Function,
            Component::End,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let dest = res.cursor_destination.unwrap();
        assert_eq!(dest.line, 4);
        assert_eq!(dest.col, 0);
        assert_eq!(res.mode_after, Some(EditorMode::Chord));
    }

    #[test]
    fn jump_outside_function_beginning_at_buffer_start_clamps_to_origin() {
        // Function starts on line 0; no line before it. jofb lands at (0, 0).
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["fn foo() {", "    x", "}"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);

        let mut lsp = mock_lsp(path, vec![sym("foo", SymbolKind::Function, 0, 0, 2, 1)]);

        let q = query(
            Action::Jump,
            Positional::Outside,
            Scope::Function,
            Component::Beginning,
            Some("foo"),
            None,
            None,
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        let dest = res.cursor_destination.unwrap();
        assert_eq!(dest.line, 0);
        assert_eq!(dest.col, 0);
    }

    #[test]
    fn jump_delimiter_scope_sets_cursor_to_open_delimiter() {
        let path = "/test/file.rs";
        let buffer = named_buf(path, &["foo(bar, baz)"]);
        let mut buffers = HashMap::new();
        buffers.insert(path.to_string(), buffer);
        let mut lsp = no_lsp();

        // jeds = Jump Entire Delimiter Self — cursor inside parens
        let q = query(
            Action::Jump,
            Positional::Entire,
            Scope::Delimiter,
            Component::Self_,
            None,
            None,
            Some((0, 4)),
        );
        let resolved = resolve(&q, &buffers, &mut lsp).unwrap();
        let res = resolved.resolutions.get(path).unwrap();
        // Jump cursor goes to start of target range (the '(' at col 3)
        assert_eq!(
            res.cursor_destination,
            Some(CursorPosition { line: 0, col: 3 })
        );
        assert_eq!(res.mode_after, Some(EditorMode::Chord));
    }
}
