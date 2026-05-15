use crate::data::lsp::types::{Language, SemanticToken};

#[cfg(test)]
thread_local! {
    pub(crate) static PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub fn parse(lang: Language, content: &str) -> Vec<SemanticToken> {
    #[cfg(test)]
    PARSE_COUNT.with(|c| c.set(c.get() + 1));
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
