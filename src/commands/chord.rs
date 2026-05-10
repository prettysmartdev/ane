use std::path::Path;

use anyhow::{bail, Result};

use crate::data::buffer::Buffer;
use crate::data::chord_types::{Action, ChordSpec, Component, Positional, Scope};

#[derive(Debug, Clone)]
pub struct ParsedChord {
    pub spec: ChordSpec,
    pub target: Option<String>,
    pub text: Option<String>,
}

pub struct ChordResult {
    pub original: String,
    pub modified: String,
}

pub fn parse_chord(input: &str) -> Result<ParsedChord> {
    let input = input.trim();

    if let Some(parsed) = try_parse_short_form(input) {
        return Ok(parsed);
    }

    if let Some(parsed) = try_parse_long_form(input) {
        return Ok(parsed);
    }

    bail!("unknown chord: {input}")
}

fn try_parse_short_form(input: &str) -> Option<ParsedChord> {
    let (chord_part, args) = split_chord_and_args(input);

    if chord_part.len() < 4 {
        return None;
    }

    let chars: Vec<&str> = chord_part
        .char_indices()
        .map(|(i, c)| &chord_part[i..i + c.len_utf8()])
        .collect();

    let action = Action::from_short(chars[0])?;
    let positional = Positional::from_short(chars[1])?;
    let scope = Scope::from_short(chars[2])?;
    let component = Component::from_short(chars[3])?;

    let (target, text) = parse_args(action, &args);

    Some(ParsedChord {
        spec: ChordSpec {
            action,
            positional,
            scope,
            component,
            requires_lsp: scope.requires_lsp(),
        },
        target,
        text,
    })
}

fn try_parse_long_form(input: &str) -> Option<ParsedChord> {
    let (chord_part, args) = split_chord_and_args(input);

    let (action, rest) = parse_long_action(chord_part)?;
    let (positional, rest) = parse_long_positional(rest)?;
    let (scope, rest) = parse_long_scope(rest)?;
    let component = parse_long_component(rest)?;

    let (target, text) = parse_args(action, &args);

    Some(ParsedChord {
        spec: ChordSpec {
            action,
            positional,
            scope,
            component,
            requires_lsp: scope.requires_lsp(),
        },
        target,
        text,
    })
}

fn split_chord_and_args(input: &str) -> (&str, Vec<&str>) {
    let mut parts = input.splitn(2, ' ');
    let chord_part = parts.next().unwrap_or("");
    let args: Vec<&str> = parts
        .next()
        .map(|s| s.splitn(2, ' ').collect())
        .unwrap_or_default();
    (chord_part, args)
}

fn parse_args(action: Action, args: &[&str]) -> (Option<String>, Option<String>) {
    match action {
        Action::Read | Action::Select | Action::Yank => {
            let target = args.first().map(|s| s.to_string());
            (target, None)
        }
        Action::Delete => {
            let target = args.first().map(|s| s.to_string());
            (target, None)
        }
        Action::Change | Action::Insert => {
            let target = args.first().map(|s| s.to_string());
            let text = args.get(1).map(|s| s.to_string());
            (target, text)
        }
        Action::Move => {
            let target = args.first().map(|s| s.to_string());
            (target, None)
        }
    }
}

fn parse_long_action(input: &str) -> Option<(Action, &str)> {
    let pairs = [
        ("Change", Action::Change),
        ("Delete", Action::Delete),
        ("Read", Action::Read),
        ("Insert", Action::Insert),
        ("Move", Action::Move),
        ("Select", Action::Select),
        ("Yank", Action::Yank),
    ];
    for (prefix, action) in pairs {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some((action, rest));
        }
    }
    None
}

fn parse_long_positional(input: &str) -> Option<(Positional, &str)> {
    let pairs = [
        ("Around", Positional::Around),
        ("Before", Positional::Before),
        ("After", Positional::After),
        ("In", Positional::In),
        ("At", Positional::At),
    ];
    for (prefix, positional) in pairs {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some((positional, rest));
        }
    }
    None
}

fn parse_long_scope(input: &str) -> Option<(Scope, &str)> {
    let pairs = [
        ("Function", Scope::Function),
        ("Variable", Scope::Variable),
        ("Block", Scope::Block),
        ("Struct", Scope::Struct),
        ("Line", Scope::Line),
        ("File", Scope::File),
        ("Impl", Scope::Impl),
        ("Enum", Scope::Enum),
    ];
    for (prefix, scope) in pairs {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some((scope, rest));
        }
    }
    None
}

fn parse_long_component(input: &str) -> Option<Component> {
    match input {
        "Body" => Some(Component::Body),
        "Name" => Some(Component::Name),
        "Signature" => Some(Component::Signature),
        "Parameters" => Some(Component::Parameters),
        "Type" => Some(Component::Type),
        "Value" => Some(Component::Value),
        "All" => Some(Component::All),
        _ => None,
    }
}

pub fn execute_chord(path: &Path, chord: &ParsedChord) -> Result<ChordResult> {
    let mut buffer = if path.exists() {
        Buffer::from_file(path)?
    } else {
        bail!("file not found: {}", path.display());
    };

    let original = buffer.content();

    match chord.spec.action {
        Action::Read => {}
        Action::Change if chord.spec.scope == Scope::Line => {
            let line: usize = chord.target.as_deref().unwrap_or("0").parse()?;
            let text = chord.text.clone().unwrap_or_default();
            buffer.set_line(line, text);
        }
        Action::Insert if chord.spec.scope == Scope::Line => {
            let line: usize = chord.target.as_deref().unwrap_or("0").parse()?;
            let text = chord.text.clone().unwrap_or_default();
            buffer.insert_line(line, text);
        }
        Action::Delete if chord.spec.scope == Scope::Line => {
            let line: usize = chord.target.as_deref().unwrap_or("0").parse()?;
            buffer.remove_line(line);
        }
        _ => {
            if chord.spec.requires_lsp {
                bail!(
                    "chord {} requires LSP (not yet connected)",
                    chord.spec.short_form()
                );
            }
            bail!(
                "chord {} not yet implemented for exec mode",
                chord.spec.short_form()
            );
        }
    }

    let modified = buffer.content();

    if buffer.dirty {
        buffer.write()?;
    }

    Ok(ChordResult { original, modified })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn temp_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn parse_short_form_cifb() {
        let parsed = parse_chord("cifb my_func replacement text").unwrap();
        assert_eq!(parsed.spec.action, Action::Change);
        assert_eq!(parsed.spec.positional, Positional::In);
        assert_eq!(parsed.spec.scope, Scope::Function);
        assert_eq!(parsed.spec.component, Component::Body);
        assert!(parsed.spec.requires_lsp);
        assert_eq!(parsed.target.as_deref(), Some("my_func"));
        assert_eq!(parsed.text.as_deref(), Some("replacement text"));
    }

    #[test]
    fn parse_long_form() {
        let parsed = parse_chord("ChangeInFunctionBody my_func new body").unwrap();
        assert_eq!(parsed.spec.action, Action::Change);
        assert_eq!(parsed.spec.positional, Positional::In);
        assert_eq!(parsed.spec.scope, Scope::Function);
        assert_eq!(parsed.spec.component, Component::Body);
        assert!(parsed.spec.requires_lsp);
    }

    #[test]
    fn parse_change_at_line_all() {
        let parsed = parse_chord("cala 5 new text here").unwrap();
        assert_eq!(parsed.spec.action, Action::Change);
        assert_eq!(parsed.spec.positional, Positional::At);
        assert_eq!(parsed.spec.scope, Scope::Line);
        assert_eq!(parsed.spec.component, Component::All);
        assert!(!parsed.spec.requires_lsp);
        assert_eq!(parsed.target.as_deref(), Some("5"));
        assert_eq!(parsed.text.as_deref(), Some("new text here"));
    }

    #[test]
    fn parse_read_at_file_all_long() {
        let parsed = parse_chord("ReadAtFileAll").unwrap();
        assert_eq!(parsed.spec.action, Action::Read);
        assert_eq!(parsed.spec.scope, Scope::File);
        assert_eq!(parsed.spec.component, Component::All);
    }

    #[test]
    fn parse_delete_at_line_all() {
        let parsed = parse_chord("dala 3").unwrap();
        assert_eq!(parsed.spec.action, Action::Delete);
        assert_eq!(parsed.spec.positional, Positional::At);
        assert_eq!(parsed.spec.scope, Scope::Line);
        assert_eq!(parsed.spec.component, Component::All);
        assert_eq!(parsed.target.as_deref(), Some("3"));
    }

    #[test]
    fn execute_read_chord() {
        let f = temp_file("hello\nworld");
        let chord = parse_chord("raFa").unwrap();
        let result = execute_chord(f.path(), &chord).unwrap();
        assert_eq!(result.original, result.modified);
    }

    #[test]
    fn execute_change_line() {
        let f = temp_file("aaa\nbbb\nccc");
        let chord = parse_chord("cala 1 xxx").unwrap();
        let result = execute_chord(f.path(), &chord).unwrap();
        assert!(result.modified.contains("xxx"));
        assert!(!result.modified.contains("bbb"));
    }

    #[test]
    fn parse_read_at_file_all_short() {
        let parsed = parse_chord("raFa").unwrap();
        assert_eq!(parsed.spec.action, Action::Read);
        assert_eq!(parsed.spec.positional, Positional::At);
        assert_eq!(parsed.spec.scope, Scope::File);
        assert_eq!(parsed.spec.component, Component::All);
        assert!(!parsed.spec.requires_lsp);
    }

    #[test]
    fn parse_delete_in_struct_body_short() {
        let parsed = parse_chord("disb my_struct").unwrap();
        assert_eq!(parsed.spec.action, Action::Delete);
        assert_eq!(parsed.spec.positional, Positional::In);
        assert_eq!(parsed.spec.scope, Scope::Struct);
        assert_eq!(parsed.spec.component, Component::Body);
        assert!(parsed.spec.requires_lsp);
        assert_eq!(parsed.target.as_deref(), Some("my_struct"));
    }

    #[test]
    fn parse_delete_in_struct_body_long() {
        let parsed = parse_chord("DeleteInStructBody my_struct").unwrap();
        assert_eq!(parsed.spec.action, Action::Delete);
        assert_eq!(parsed.spec.positional, Positional::In);
        assert_eq!(parsed.spec.scope, Scope::Struct);
        assert_eq!(parsed.spec.component, Component::Body);
        assert!(parsed.spec.requires_lsp);
        assert_eq!(parsed.target.as_deref(), Some("my_struct"));
    }
}
