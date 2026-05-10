use std::fmt;

use crate::data::chord_types::{Component, Scope};

#[derive(Debug)]
pub enum ChordError {
    ParseError {
        input: String,
        position: usize,
        message: String,
        suggestion: Option<String>,
    },
    ResolveError {
        buffer_name: String,
        message: String,
        available_symbols: Vec<String>,
    },
    PatchError {
        buffer_name: String,
        message: String,
    },
    LspRequired {
        scope: Scope,
        lsp_state: String,
    },
    InvalidCombination {
        scope: Scope,
        component: Component,
        valid_components: Vec<Component>,
    },
}

impl fmt::Display for ChordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChordError::ParseError {
                input,
                position,
                message,
                suggestion,
            } => {
                write!(f, "chord parse error: {message}\n  input: {input}")?;
                if *position > 0 {
                    write!(f, "\n  position: {position}")?;
                }
                if let Some(sug) = suggestion {
                    write!(f, "\n  did you mean: {sug}?")?;
                }
                Ok(())
            }
            ChordError::ResolveError {
                buffer_name,
                message,
                available_symbols,
            } => {
                write!(f, "chord resolve error in {buffer_name}: {message}")?;
                if !available_symbols.is_empty() {
                    write!(f, "\n  available symbols: {}", available_symbols.join(", "))?;
                }
                Ok(())
            }
            ChordError::PatchError {
                buffer_name,
                message,
            } => {
                write!(f, "chord patch error in {buffer_name}: {message}")
            }
            ChordError::LspRequired { scope, lsp_state } => {
                write!(
                    f,
                    "chord requires LSP for {scope} scope (LSP status: {lsp_state})"
                )
            }
            ChordError::InvalidCombination {
                scope,
                component,
                valid_components,
            } => {
                let valid: Vec<String> = valid_components.iter().map(|c| format!("{c}")).collect();
                write!(
                    f,
                    "invalid component '{component}' for scope '{scope}'\n  {scope} scope supports components: {}",
                    valid.join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ChordError {}

impl ChordError {
    pub fn parse(input: &str, position: usize, message: impl Into<String>) -> Self {
        Self::ParseError {
            input: input.to_string(),
            position,
            message: message.into(),
            suggestion: None,
        }
    }

    pub fn parse_with_suggestion(
        input: &str,
        position: usize,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self::ParseError {
            input: input.to_string(),
            position,
            message: message.into(),
            suggestion: Some(suggestion.into()),
        }
    }

    pub fn resolve(buffer_name: &str, message: impl Into<String>) -> Self {
        Self::ResolveError {
            buffer_name: buffer_name.to_string(),
            message: message.into(),
            available_symbols: Vec::new(),
        }
    }

    pub fn resolve_with_symbols(
        buffer_name: &str,
        message: impl Into<String>,
        symbols: Vec<String>,
    ) -> Self {
        Self::ResolveError {
            buffer_name: buffer_name.to_string(),
            message: message.into(),
            available_symbols: symbols,
        }
    }

    pub fn patch(buffer_name: &str, message: impl Into<String>) -> Self {
        Self::PatchError {
            buffer_name: buffer_name.to_string(),
            message: message.into(),
        }
    }

    pub fn invalid_combination(scope: Scope, component: Component) -> Self {
        let valid = valid_components_for_scope(scope);
        Self::InvalidCombination {
            scope,
            component,
            valid_components: valid,
        }
    }
}

fn valid_components_for_scope(scope: Scope) -> Vec<Component> {
    let all = [
        Component::Beginning,
        Component::End,
        Component::Value,
        Component::Parameters,
        Component::Arguments,
        Component::Name,
        Component::Self_,
    ];
    all.iter()
        .copied()
        .filter(|c| crate::data::chord_types::is_valid_combination(scope, *c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_with_symbols_renders_candidate_list() {
        let err = ChordError::resolve_with_symbols(
            "/tmp/foo.rs",
            "symbol 'fn_x' not found",
            vec!["fn_one".to_string(), "fn_two".to_string()],
        );
        let msg = err.to_string();
        assert!(msg.contains("symbol 'fn_x' not found"), "got: {msg}");
        assert!(
            msg.contains("available symbols: fn_one, fn_two"),
            "got: {msg}"
        );
    }

    #[test]
    fn resolve_with_no_symbols_omits_candidate_line() {
        let err = ChordError::resolve("/tmp/foo.rs", "boom");
        let msg = err.to_string();
        assert_eq!(msg, "chord resolve error in /tmp/foo.rs: boom");
    }
}
