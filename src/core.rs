pub use crate::commands::chord::{execute_chord, parse_chord, ChordResult, FrontendCapabilities};
pub use crate::commands::chord_engine::errors::ChordError;
pub use crate::commands::chord_engine::types::{
    ChordAction, ChordArgs, ChordQuery, ResolvedChord, TextRange,
};
pub use crate::commands::chord_engine::ChordEngine;
pub use crate::commands::diff::unified_diff;
pub use crate::commands::lsp_engine::{LspEngine, LspEngineConfig};
pub use crate::data::buffer::Buffer;
pub use crate::data::chord_types::{Action, Component, Positional, Scope};
pub use crate::data::skill::SKILL_CONTENT;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

const TOOL_DESCRIPTION: &str = "\
Execute a structured chord edit on a file using ane's chord grammar.\n\
\n\
Chords are 4 characters: Action + Positional + Scope + Component.\n\
\n\
Actions:    c=Change d=Delete r=Replace y=Yank a=Append p=Prepend i=Insert\n\
Positional: i=Inside e=Entire a=After b=Before n=Next p=Previous u=Until o=Outside t=To\n\
Scope:      l=Line b=Buffer f=Function v=Variable s=Struct m=Member\n\
Component:  b=Beginning c=Contents e=End v=Value p=Parameters n=Name s=Self\n\
\n\
Args in parens: chord(target:fn_name, line:N)\n\
Use the value parameter (not inline) for replacement text.\n\
\n\
Examples:\n\
  cels(line:3) + value -> change line 3\n\
  dels(line:5) -> delete line 5\n\
  cifn(function:getData) + value -> rename function\n\
  aale(line:10) + value -> append after line 10\n\
  yefc(function:main) -> yank function body\n\
  rifc(function:handler) + value -> replace function contents";

pub fn tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: "ane".to_string(),
        description: TOOL_DESCRIPTION.to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "chord": {
                    "type": "string",
                    "description": "Chord expression, e.g. \"cels(line:3)\" or \"cifn(function:getData)\""
                },
                "value": {
                    "type": "string",
                    "description": "Text for Change/Replace/Append/Insert actions. Preferred over inline value arg for multiline content."
                }
            },
            "required": ["file_path", "chord"]
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_parse_round_trip() {
        let query = parse_chord("cifn").unwrap();
        assert_eq!(query.action, Action::Change);
    }

    #[test]
    fn skill_content_accessible_via_core() {
        assert!(!SKILL_CONTENT.is_empty());
    }

    #[test]
    fn tool_definition_has_correct_name() {
        assert_eq!(tool_definition().name, "ane");
    }

    #[test]
    fn tool_definition_has_non_empty_description() {
        assert!(!tool_definition().description.is_empty());
    }

    #[test]
    fn tool_definition_schema_has_required_fields() {
        let def = tool_definition();
        let required = def.input_schema["required"]
            .as_array()
            .expect("required should be an array");
        let fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            fields.contains(&"file_path"),
            "missing file_path in required"
        );
        assert!(fields.contains(&"chord"), "missing chord in required");
    }

    #[test]
    fn tool_definition_serializes_to_valid_json() {
        let value = serde_json::to_value(tool_definition()).unwrap();
        assert!(value.get("name").is_some());
        assert!(value.get("description").is_some());
        assert!(value.get("input_schema").is_some());
    }

    #[test]
    fn tool_description_under_250_words() {
        let word_count = TOOL_DESCRIPTION.split_whitespace().count();
        assert!(
            word_count <= 250,
            "TOOL_DESCRIPTION has {word_count} words, expected <= 250"
        );
    }
}
