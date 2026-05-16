use crate::data::lsp::types::{Language, SemanticToken};

#[cfg(test)]
thread_local! {
    pub(crate) static PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

const MAX_PARSE_SIZE: usize = 512 * 1024;

pub fn parse(lang: Language, content: &str) -> Vec<SemanticToken> {
    #[cfg(test)]
    PARSE_COUNT.with(|c| c.set(c.get() + 1));
    if content.len() > MAX_PARSE_SIZE {
        return vec![];
    }
    let result = match lang {
        Language::Rust => parse_with(&tree_sitter_rust::LANGUAGE.into(), content, rust_node_type),
        Language::Go => parse_with(&tree_sitter_go::LANGUAGE.into(), content, go_node_type),
        Language::TypeScript => parse_with(
            &tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            content,
            ts_node_type,
        ),
        Language::Python => parse_with(
            &tree_sitter_python::LANGUAGE.into(),
            content,
            python_node_type,
        ),
        Language::Markdown => Some(parse_markdown(content)),
        Language::Json => Some(parse_json(content)),
        Language::Yaml => Some(parse_yaml(content)),
        Language::Toml => parse_with(
            &tree_sitter_toml_ng::LANGUAGE.into(),
            content,
            toml_node_type,
        ),
        Language::Dockerfile => parse_with(
            &tree_sitter_containerfile::LANGUAGE.into(),
            content,
            dockerfile_node_type,
        ),
        Language::Xml => Some(parse_xml(content)),
    };
    result.unwrap_or_default()
}

fn parse_with(
    language: &tree_sitter::Language,
    content: &str,
    map_fn: fn(&str) -> Option<&'static str>,
) -> Option<Vec<SemanticToken>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(language).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let mut tokens = Vec::new();
    let mut cursor = root.walk();
    walk_tree(&mut cursor, content, map_fn, &mut tokens);
    tokens.sort_by_key(|t| (t.line, t.start_col));
    Some(tokens)
}

fn walk_tree(
    cursor: &mut tree_sitter::TreeCursor,
    content: &str,
    map_fn: fn(&str) -> Option<&'static str>,
    tokens: &mut Vec<SemanticToken>,
) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        if let Some(token_type) = map_fn(kind)
            && (node.child_count() == 0 || is_leaf_like(kind))
        {
            emit_tokens_for_node(&node, content, token_type, tokens);
        }

        if !is_leaf_like(kind) && cursor.goto_first_child() {
            walk_tree(cursor, content, map_fn, tokens);
            cursor.goto_parent();
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn is_leaf_like(kind: &str) -> bool {
    matches!(
        kind,
        "string_literal"
            | "raw_string_literal"
            | "char_literal"
            | "line_comment"
            | "block_comment"
            | "comment"
            | "interpreted_string_literal"
            | "rune_literal"
            | "string"
            | "template_string"
            | "concatenated_string"
            | "atx_heading"
            | "setext_heading"
            | "code_span"
            | "emphasis"
            | "strong_emphasis"
            | "inline_link"
            | "full_reference_link"
            | "collapsed_reference_link"
            | "shortcut_link"
            | "uri_autolink"
            | "email_autolink"
            | "image"
            | "strikethrough"
            | "double_quote_scalar"
            | "single_quote_scalar"
            | "block_scalar"
            | "double_quoted_string"
            | "single_quoted_string"
            | "heredoc_block"
            | "image_tag"
            | "image_digest"
    )
}

fn emit_tokens_for_node(
    node: &tree_sitter::Node,
    content: &str,
    token_type: &'static str,
    tokens: &mut Vec<SemanticToken>,
) {
    let start_line = node.start_position().row;
    let end_line = node.end_position().row;

    if start_line == end_line {
        let start_col = byte_to_char_col(content, start_line, node.start_position().column);
        let end_col = byte_to_char_col(content, end_line, node.end_position().column);
        if end_col > start_col {
            tokens.push(SemanticToken {
                line: start_line,
                start_col,
                length: end_col - start_col,
                token_type: token_type.to_string(),
            });
        }
    } else {
        let lines: Vec<&str> = content.lines().collect();
        for line_num in start_line..=end_line {
            if let Some(line_text) = lines.get(line_num) {
                let char_count = line_text.chars().count();
                let (start_col, end_col) = if line_num == start_line {
                    let sc = byte_to_char_col(content, line_num, node.start_position().column);
                    (sc, char_count)
                } else if line_num == end_line {
                    let ec = byte_to_char_col(content, line_num, node.end_position().column);
                    (0, ec)
                } else {
                    (0, char_count)
                };
                if end_col > start_col {
                    tokens.push(SemanticToken {
                        line: line_num,
                        start_col,
                        length: end_col - start_col,
                        token_type: token_type.to_string(),
                    });
                }
            }
        }
    }
}

fn byte_to_char_col(content: &str, line_num: usize, byte_col: usize) -> usize {
    content
        .lines()
        .nth(line_num)
        .map(|line| {
            let safe_byte = byte_col.min(line.len());
            line[..safe_byte].chars().count()
        })
        .unwrap_or(0)
}

fn parse_markdown(content: &str) -> Vec<SemanticToken> {
    let mut tokens = Vec::new();

    // Phase 1: Block-level parsing (headings, code blocks, quotes, list markers)
    if let Some(block_tokens) = parse_with(&tree_sitter_md::LANGUAGE.into(), content, md_node_type)
    {
        tokens.extend(block_tokens);
    }

    // Phase 2: Inline-level parsing (emphasis, strong, code spans, links)
    if let Some(inline_tokens) = parse_with(
        &tree_sitter_md::INLINE_LANGUAGE.into(),
        content,
        md_inline_node_type,
    ) {
        tokens.extend(inline_tokens);
    }

    tokens.sort_by_key(|t| (t.line, t.start_col));
    tokens
}

// --- Language-specific node type mappings ---

fn rust_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "use" | "let" | "mut" | "const" | "static" | "fn" | "pub" | "mod" | "struct" | "enum"
        | "impl" | "trait" | "type" | "where" | "for" | "in" | "loop" | "while" | "if" | "else"
        | "match" | "return" | "break" | "continue" | "as" | "ref" | "self" | "super" | "crate"
        | "async" | "await" | "move" | "unsafe" | "extern" | "dyn" | "true" | "false" => {
            Some("keyword")
        }
        "type_identifier" | "primitive_type" => Some("type"),
        "identifier" => None,
        "function_item" => None,
        "string_literal" | "raw_string_literal" | "char_literal" => Some("string"),
        "integer_literal" | "float_literal" => Some("number"),
        "line_comment" | "block_comment" => Some("comment"),
        "attribute_item" | "inner_attribute_item" => Some("macro"),
        "macro_invocation" => None,
        "!" => None,
        _ => None,
    }
}

fn go_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "package" | "import" | "func" | "return" | "var" | "const" | "type" | "struct"
        | "interface" | "map" | "chan" | "go" | "defer" | "if" | "else" | "for" | "range"
        | "switch" | "case" | "default" | "select" | "break" | "continue" | "fallthrough"
        | "goto" | "nil" | "true" | "false" => Some("keyword"),
        "type_identifier" => Some("type"),
        "field_identifier" => Some("property"),
        "identifier" => None,
        "interpreted_string_literal" | "raw_string_literal" | "rune_literal" => Some("string"),
        "int_literal" | "float_literal" | "imaginary_literal" => Some("number"),
        "comment" => Some("comment"),
        _ => None,
    }
}

fn ts_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "import" | "export" | "from" | "const" | "let" | "var" | "function" | "return" | "if"
        | "else" | "for" | "while" | "do" | "switch" | "case" | "break" | "continue" | "class"
        | "extends" | "implements" | "new" | "this" | "super" | "typeof" | "instanceof" | "in"
        | "of" | "async" | "await" | "yield" | "throw" | "try" | "catch" | "finally"
        | "default" | "void" | "delete" | "true" | "false" | "null" | "undefined" | "type"
        | "interface" | "enum" | "namespace" | "declare" | "as" | "readonly" | "abstract"
        | "static" | "private" | "protected" | "public" | "keyof" | "infer" | "satisfies" => {
            Some("keyword")
        }
        "type_identifier" | "predefined_type" => Some("type"),
        "property_identifier" => Some("property"),
        "identifier" => None,
        "string" | "template_string" => Some("string"),
        "number" | "regex" => Some("number"),
        "comment" => Some("comment"),
        _ => None,
    }
}

fn python_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "import" | "from" | "def" | "class" | "return" | "if" | "elif" | "else" | "for"
        | "while" | "break" | "continue" | "pass" | "raise" | "try" | "except" | "finally"
        | "with" | "as" | "lambda" | "yield" | "global" | "nonlocal" | "assert" | "del" | "and"
        | "or" | "not" | "is" | "in" | "True" | "False" | "None" | "async" | "await" => {
            Some("keyword")
        }
        "identifier" => None,
        "type" => Some("type"),
        "string" | "concatenated_string" => Some("string"),
        "integer" | "float" => Some("number"),
        "comment" => Some("comment"),
        "decorator" => Some("macro"),
        _ => None,
    }
}

fn md_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "atx_heading" | "setext_heading" | "atx_h1_marker" | "atx_h2_marker" | "atx_h3_marker"
        | "atx_h4_marker" | "atx_h5_marker" | "atx_h6_marker" => Some("heading"),
        "fenced_code_block" | "indented_code_block" | "code_fence_content" | "info_string" => {
            Some("code")
        }
        "block_quote" | "block_quote_marker" => Some("quote"),
        "list_marker_dot"
        | "list_marker_minus"
        | "list_marker_star"
        | "list_marker_plus"
        | "list_marker_parenthesis" => Some("list_marker"),
        "thematic_break" => Some("punctuation"),
        _ => None,
    }
}

fn md_inline_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "code_span" => Some("code"),
        "emphasis" => Some("emphasis"),
        "strong_emphasis" => Some("strong"),
        "inline_link"
        | "full_reference_link"
        | "collapsed_reference_link"
        | "shortcut_link"
        | "uri_autolink"
        | "email_autolink"
        | "image" => Some("link"),
        "strikethrough" => Some("punctuation"),
        _ => None,
    }
}

// --- JSON ---

fn parse_json(content: &str) -> Vec<SemanticToken> {
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_json::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return vec![];
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return vec![],
    };
    let mut tokens = Vec::new();
    let mut cursor = tree.root_node().walk();
    walk_json(&mut cursor, content, &mut tokens);
    tokens.sort_by_key(|t| (t.line, t.start_col));
    tokens
}

fn walk_json(cursor: &mut tree_sitter::TreeCursor, content: &str, tokens: &mut Vec<SemanticToken>) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        match kind {
            "pair" => {
                if cursor.goto_first_child() {
                    // First child is the key — emit it directly as "key"
                    let key_node = cursor.node();
                    if key_node.kind() == "string" {
                        emit_tokens_for_node(&key_node, content, "key", tokens);
                    }
                    // Advance past the ":" separator and recurse into the value subtree
                    while cursor.goto_next_sibling() {
                        if cursor.node().kind() == ":" {
                            continue;
                        }
                        walk_json(cursor, content, tokens);
                        break;
                    }
                    cursor.goto_parent();
                }
            }
            "string" => {
                emit_tokens_for_node(&node, content, "string", tokens);
            }
            "number" => {
                emit_tokens_for_node(&node, content, "number", tokens);
            }
            "true" | "false" | "null" => {
                emit_tokens_for_node(&node, content, "keyword", tokens);
            }
            "comment" => {
                emit_tokens_for_node(&node, content, "comment", tokens);
            }
            _ => {
                if cursor.goto_first_child() {
                    walk_json(cursor, content, tokens);
                    cursor.goto_parent();
                }
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

// --- YAML ---

fn parse_yaml(content: &str) -> Vec<SemanticToken> {
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_yaml::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return vec![];
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return vec![],
    };
    let mut tokens = Vec::new();
    let mut cursor = tree.root_node().walk();
    walk_yaml(&mut cursor, content, &mut tokens, false);
    tokens.sort_by_key(|t| (t.line, t.start_col));
    tokens
}

fn walk_yaml(
    cursor: &mut tree_sitter::TreeCursor,
    content: &str,
    tokens: &mut Vec<SemanticToken>,
    is_key: bool,
) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        match kind {
            "block_mapping_pair" | "flow_pair" => {
                if cursor.goto_first_child() {
                    walk_yaml(cursor, content, tokens, true);
                    cursor.goto_parent();
                }
            }
            "plain_scalar" | "string_scalar" => {
                if is_key {
                    emit_tokens_for_node(&node, content, "key", tokens);
                } else {
                    // The typed child (integer_scalar, boolean_scalar, etc.) is nested
                    // inside plain_scalar — inspect the first child's kind to pick the
                    // correct token type rather than defaulting everything to "string".
                    let token_type = if cursor.goto_first_child() {
                        let child_kind = cursor.node().kind();
                        cursor.goto_parent();
                        match child_kind {
                            "integer_scalar" | "float_scalar" | "timestamp_scalar" => "number",
                            "boolean_scalar" | "null_scalar" => "keyword",
                            _ => "string",
                        }
                    } else {
                        "string"
                    };
                    emit_tokens_for_node(&node, content, token_type, tokens);
                }
            }
            "double_quote_scalar" | "single_quote_scalar" | "block_scalar" => {
                let token_type = if is_key { "key" } else { "string" };
                emit_tokens_for_node(&node, content, token_type, tokens);
            }
            "integer_scalar" | "float_scalar" | "timestamp_scalar" => {
                emit_tokens_for_node(&node, content, "number", tokens);
            }
            "boolean_scalar" | "null_scalar" => {
                emit_tokens_for_node(&node, content, "keyword", tokens);
            }
            "comment" => {
                emit_tokens_for_node(&node, content, "comment", tokens);
            }
            "anchor" | "alias" | "tag" => {
                emit_tokens_for_node(&node, content, "type", tokens);
            }
            ":" => {
                // After the colon, subsequent siblings are values
                if cursor.goto_next_sibling() {
                    walk_yaml(cursor, content, tokens, false);
                }
                break;
            }
            _ => {
                if cursor.goto_first_child() {
                    walk_yaml(cursor, content, tokens, is_key);
                    cursor.goto_parent();
                }
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

// --- TOML ---

fn toml_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "bare_key" | "quoted_key" => Some("key"),
        "table" | "table_array_element" => Some("type"),
        "string" => Some("string"),
        "integer" | "float" | "offset_date_time" | "local_date_time" | "local_date"
        | "local_time" => Some("number"),
        "boolean" => Some("keyword"),
        "comment" => Some("comment"),
        _ => None,
    }
}

// --- Dockerfile ---

fn dockerfile_node_type(kind: &str) -> Option<&'static str> {
    match kind {
        "FROM" | "RUN" | "CMD" | "LABEL" | "MAINTAINER" | "EXPOSE" | "ENV" | "ADD" | "COPY"
        | "ENTRYPOINT" | "VOLUME" | "USER" | "WORKDIR" | "ARG" | "ONBUILD" | "STOPSIGNAL"
        | "HEALTHCHECK" | "SHELL" | "CROSS_BUILD" | "AS" => Some("keyword"),
        "image_name" | "image_alias" => Some("type"),
        "image_tag" | "image_digest" => Some("string"),
        "double_quoted_string" | "single_quoted_string" | "json_string" => Some("string"),
        "comment" => Some("comment"),
        "variable" => Some("variable"),
        _ => None,
    }
}

// --- XML ---

fn parse_xml(content: &str) -> Vec<SemanticToken> {
    let mut parser = tree_sitter::Parser::new();
    let lang: tree_sitter::Language = tree_sitter_xml::LANGUAGE_XML.into();
    if parser.set_language(&lang).is_err() {
        return vec![];
    }
    let tree = match parser.parse(content, None) {
        Some(t) => t,
        None => return vec![],
    };
    let mut tokens = Vec::new();
    let mut cursor = tree.root_node().walk();
    walk_xml(&mut cursor, content, &mut tokens);
    tokens.sort_by_key(|t| (t.line, t.start_col));
    tokens
}

fn walk_xml(cursor: &mut tree_sitter::TreeCursor, content: &str, tokens: &mut Vec<SemanticToken>) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        match kind {
            "Comment" => {
                emit_tokens_for_node(&node, content, "comment", tokens);
            }
            "CDSect" | "CData" => {
                emit_tokens_for_node(&node, content, "string", tokens);
            }
            "PI" => {
                emit_tokens_for_node(&node, content, "keyword", tokens);
            }
            "CharData" => {
                emit_tokens_for_node(&node, content, "variable", tokens);
            }
            "Attribute" => {
                // First child is the Name (attribute name), then =, then AttValue
                if cursor.goto_first_child() {
                    walk_xml_attribute(cursor, content, tokens);
                    cursor.goto_parent();
                }
            }
            "Name" => {
                emit_tokens_for_node(&node, content, "type", tokens);
            }
            "AttValue" => {
                emit_tokens_for_node(&node, content, "string", tokens);
            }
            _ => {
                if cursor.goto_first_child() {
                    walk_xml(cursor, content, tokens);
                    cursor.goto_parent();
                }
            }
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

fn walk_xml_attribute(
    cursor: &mut tree_sitter::TreeCursor,
    content: &str,
    tokens: &mut Vec<SemanticToken>,
) {
    loop {
        let node = cursor.node();
        let kind = node.kind();

        match kind {
            "Name" => {
                emit_tokens_for_node(&node, content, "key", tokens);
            }
            "AttValue" => {
                emit_tokens_for_node(&node, content, "string", tokens);
            }
            _ => {}
        }

        if !cursor.goto_next_sibling() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::lsp::types::Language;

    fn has_type(tokens: &[SemanticToken], ty: &str) -> bool {
        tokens.iter().any(|t| t.token_type == ty)
    }

    fn count_type(tokens: &[SemanticToken], ty: &str) -> usize {
        tokens.iter().filter(|t| t.token_type == ty).count()
    }

    #[test]
    fn json_node_type_mappings() {
        let content = r#"{"key": "value", "count": 42, "active": true, "nothing": null}"#;
        let tokens = parse(Language::Json, content);

        assert!(
            has_type(&tokens, "key"),
            "expected 'key' tokens for object keys"
        );
        assert!(
            has_type(&tokens, "string"),
            "expected 'string' tokens for string values"
        );
        assert!(
            has_type(&tokens, "number"),
            "expected 'number' token for 42"
        );
        assert!(
            has_type(&tokens, "keyword"),
            "expected 'keyword' tokens for true and null"
        );

        assert_eq!(count_type(&tokens, "key"), 4, "four object keys");
        assert_eq!(count_type(&tokens, "string"), 1, "one string value");
    }

    #[test]
    fn yaml_node_type_mappings() {
        let content = "name: hello\ncount: 42\nactive: true\n# a comment\n";
        let tokens = parse(Language::Yaml, content);

        assert!(
            has_type(&tokens, "key"),
            "YAML mapping keys should be 'key'"
        );
        assert!(
            has_type(&tokens, "string"),
            "YAML plain scalars as values should be 'string'"
        );
        assert!(
            has_type(&tokens, "number"),
            "YAML integer should be 'number'"
        );
        assert!(
            has_type(&tokens, "keyword"),
            "YAML boolean should be 'keyword'"
        );
        assert!(
            has_type(&tokens, "comment"),
            "YAML comment should be 'comment'"
        );
    }

    #[test]
    fn toml_node_type_mappings() {
        let content = "[section]\nkey = \"value\"\ncount = 42\nactive = true\n# comment\n";
        let tokens = parse(Language::Toml, content);

        assert!(has_type(&tokens, "key"), "TOML bare keys should be 'key'");
        assert!(
            has_type(&tokens, "string"),
            "TOML strings should be 'string'"
        );
        assert!(
            has_type(&tokens, "number"),
            "TOML integers should be 'number'"
        );
        assert!(
            has_type(&tokens, "keyword"),
            "TOML booleans should be 'keyword'"
        );
        assert!(
            has_type(&tokens, "comment"),
            "TOML comments should be 'comment'"
        );
    }

    #[test]
    fn dockerfile_node_type_mappings() {
        let content = "FROM ubuntu:22.04\nRUN apt-get update\n# comment\n";
        let tokens = parse(Language::Dockerfile, content);

        assert!(
            has_type(&tokens, "keyword"),
            "FROM and RUN instructions should be 'keyword'"
        );
        assert!(
            has_type(&tokens, "type"),
            "image name 'ubuntu' should be 'type'"
        );
        assert!(
            has_type(&tokens, "string"),
            "image tag '22.04' should be 'string'"
        );
        assert!(
            has_type(&tokens, "comment"),
            "# comment should be 'comment'"
        );
    }

    #[test]
    fn xml_node_type_mappings() {
        let content = r#"<root id="1">text</root>"#;
        let tokens = parse(Language::Xml, content);

        assert!(has_type(&tokens, "type"), "tag names should be 'type'");
        assert!(
            has_type(&tokens, "key"),
            "attribute name 'id' should be 'key'"
        );
        assert!(
            has_type(&tokens, "string"),
            "attribute value should be 'string'"
        );
        assert!(
            has_type(&tokens, "variable"),
            "text content should be 'variable'"
        );
    }

    #[test]
    fn yaml_multiline_block_scalar() {
        let content = "text: |\n  line one\n  line two\n  line three\n";
        let tokens = parse(Language::Yaml, content);

        let string_tokens: Vec<_> = tokens.iter().filter(|t| t.token_type == "string").collect();
        // block_scalar spans the content lines; lines 1–3 must each have a string token
        assert!(
            string_tokens.len() >= 3,
            "expected at least 3 string tokens for 3-line block scalar, got {}",
            string_tokens.len()
        );
        let string_lines: std::collections::HashSet<usize> =
            string_tokens.iter().map(|t| t.line).collect();
        assert!(
            string_lines.contains(&1),
            "line 1 should have a string token"
        );
        assert!(
            string_lines.contains(&2),
            "line 2 should have a string token"
        );
        assert!(
            string_lines.contains(&3),
            "line 3 should have a string token"
        );
        for tok in &string_tokens {
            assert_eq!(tok.token_type, "string");
        }
    }

    #[test]
    fn large_file_guard_returns_empty() {
        let huge = "x".repeat(512 * 1024 + 1);
        let tokens = parse(Language::Json, &huge);
        assert!(
            tokens.is_empty(),
            "content over 512 KB should return empty tokens"
        );
    }
}
