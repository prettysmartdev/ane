# Work Item: Feature

Title: Tree-sitter syntax highlighting for JSON, YAML, TOML, Dockerfile, and XML
Issue: N/A

## Summary

Extend the `SyntaxEngine` and `Language` capability matrix (introduced in work item 0008) with tree-sitter syntax highlighting for JSON, YAML, TOML, Dockerfiles, and XML. All five are config/markup languages with no LSP server — `has_lsp: false` for each. The `SyntaxEngine` pipeline requires no structural changes; this work item adds new `Language` variants, extension/filename detection, tree-sitter grammar crates, and per-language node-type-to-token-type mappings.

---

## User Stories

### User Story 1
As a: developer editing configuration files (JSON, YAML, TOML) in ane

I want to: see syntax highlighting appear immediately when a config file opens

So I can: distinguish keys from values, strings from numbers, and comments from data at a glance without waiting for any external tool

### User Story 2
As a: developer working on a containerized project

I want to: see Dockerfile instructions (FROM, RUN, COPY, etc.) highlighted distinctly from their arguments

So I can: scan build stage structure quickly and spot misconfigured instructions

### User Story 3
As a: developer editing XML or SVG files

I want to: see tag names, attribute keys, attribute values, and comments highlighted separately

So I can: navigate deeply nested markup without losing track of structure

---

## Implementation Details

### 1. Extend `Language` enum — `src/data/lsp/types.rs` (Layer 0)

Add five new variants to the `Language` enum and update `capabilities()`, `from_extension()`, and `name()`:

```rust
pub enum Language {
    // existing variants...
    Json,
    Yaml,
    Toml,
    Dockerfile,
    Xml,
}

impl Language {
    pub fn capabilities(self) -> LanguageCapabilities {
        match self {
            // existing variants...
            Language::Json       => LanguageCapabilities { has_tree_sitter: true, has_lsp: false },
            Language::Yaml       => LanguageCapabilities { has_tree_sitter: true, has_lsp: false },
            Language::Toml       => LanguageCapabilities { has_tree_sitter: true, has_lsp: false },
            Language::Dockerfile => LanguageCapabilities { has_tree_sitter: true, has_lsp: false },
            Language::Xml        => LanguageCapabilities { has_tree_sitter: true, has_lsp: false },
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            // existing variants...
            "json" | "jsonc"             => Some(Self::Json),
            "yaml" | "yml"               => Some(Self::Yaml),
            "toml"                       => Some(Self::Toml),
            "xml" | "xsd" | "xsl"
            | "xslt" | "svg" | "rss"    => Some(Self::Xml),
            _                            => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            // existing variants...
            Self::Json       => "json",
            Self::Yaml       => "yaml",
            Self::Toml       => "toml",
            Self::Dockerfile => "dockerfile",
            Self::Xml        => "xml",
        }
    }
}
```

Dockerfile has no conventional extension — detection requires a separate path-based check. Add a `from_path` method (or extend the existing one) that checks the filename stem after the extension check fails:

```rust
impl Language {
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        // Try extension first
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext == "dockerfile" {
                return Some(Self::Dockerfile);
            }
            if let Some(lang) = Self::from_extension(ext) {
                return Some(lang);
            }
        }
        // Fall back to filename stem for extensionless files
        let stem = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        match stem {
            "Dockerfile" | "dockerfile"                  => Some(Self::Dockerfile),
            "docker-compose.yml" | "docker-compose.yaml" => Some(Self::Yaml),
            _                                            => None,
        }
    }
}
```

`docker-compose.yml` already resolves via the `.yml` → `Yaml` path; the stem check above is a safety net for extensionless compose files. `SyntaxEngine::compute` already calls `Language::from_path` — no changes needed there.

---

### 2. Tree-sitter grammar crates — `Cargo.toml`

```toml
[dependencies]
tree-sitter-json       = "0.21"
tree-sitter-yaml       = "0.6"
tree-sitter-toml       = "0.21"
tree-sitter-dockerfile = "0.2"
tree-sitter-xml        = "0.7"
```

Verify the exact published versions and crate names on crates.io before finalizing — some grammars (YAML in particular) have been republished under different names by different maintainers. All grammar crates compile their C parsers via a `build.rs` build script. Measure release binary size delta after adding all five; LTO + strip is already configured.

---

### 3. Tree-sitter parsing — `src/commands/syntax_engine/tree_sitter.rs` (Layer 1)

Extend `parse()` to dispatch to the five new languages:

```rust
pub fn parse(lang: Language, content: &str) -> Vec<SemanticToken> {
    match lang {
        // existing variants...
        Language::Json       => parse_with(tree_sitter_json::language(),       content, json_node_type),
        Language::Yaml       => parse_with(tree_sitter_yaml::language(),       content, yaml_node_type),
        Language::Toml       => parse_with(tree_sitter_toml::language(),       content, toml_node_type),
        Language::Dockerfile => parse_with(tree_sitter_dockerfile::language(), content, dockerfile_node_type),
        Language::Xml        => parse_with(tree_sitter_xml::language(),        content, xml_node_type),
    }
}
```

**Node type → token type mappings**

All new token types reuse the existing set where possible. Only `"key"` is new and requires a color entry in `token_style`.

| Language    | Node type (tree-sitter)                                  | `token_type` string |
|-------------|----------------------------------------------------------|---------------------|
| JSON        | `string`                                                 | `"string"`          |
| JSON        | `number`                                                 | `"number"`          |
| JSON        | `true`, `false`, `null`                                  | `"keyword"`         |
| JSON        | `pair` → first child (the key string)                   | `"key"`             |
| JSON        | `comment` (JSONC only)                                   | `"comment"`         |
| YAML        | `block_mapping_pair` → key scalar                        | `"key"`             |
| YAML        | `double_quote_scalar`, `single_quote_scalar`, `block_scalar` | `"string"`     |
| YAML        | `integer`, `float`                                       | `"number"`          |
| YAML        | `true`, `false`, `null`                                  | `"keyword"`         |
| YAML        | `comment`                                                | `"comment"`         |
| YAML        | `anchor`, `alias`                                        | `"type"`            |
| YAML        | `tag`                                                    | `"type"`            |
| TOML        | `bare_key`, `quoted_key`                                 | `"key"`             |
| TOML        | `table`, `array_table` (header brackets)                 | `"type"`            |
| TOML        | `basic_string`, `literal_string`, `ml_basic_string`, `ml_literal_string` | `"string"` |
| TOML        | `integer`, `float`                                       | `"number"`          |
| TOML        | `boolean`                                                | `"keyword"`         |
| TOML        | `offset_date_time`, `local_date_time`, `local_date`, `local_time` | `"number"` |
| TOML        | `comment`                                                | `"comment"`         |
| Dockerfile  | `from`, `run`, `copy`, `add`, `env`, `expose`, `cmd`, `entrypoint`, `workdir`, `user`, `volume`, `arg`, `label`, `healthcheck`, `shell`, `onbuild`, `stopsignal` (instruction keywords) | `"keyword"` |
| Dockerfile  | `image_spec` → name component                            | `"type"`            |
| Dockerfile  | `image_tag`                                              | `"string"`          |
| Dockerfile  | `comment`                                                | `"comment"`         |
| Dockerfile  | `double_quoted_string`, `single_quoted_string`           | `"string"`          |
| Dockerfile  | `variable`                                               | `"variable"`        |
| XML         | `tag_name`                                               | `"type"`            |
| XML         | `attribute_name`                                         | `"key"`             |
| XML         | `attribute_value`                                        | `"string"`          |
| XML         | `text`                                                   | `"variable"`        |
| XML         | `comment`                                                | `"comment"`         |
| XML         | `cdata_sect`                                             | `"string"`          |
| XML         | `processing_instruction`                                 | `"keyword"`         |

**New `"key"` token type in `editor_pane.rs`**

Add a case to `token_style` for `"key"` — a distinct color from `"string"` to differentiate config keys from their values:

```rust
"key" => Style::default().fg(Color::Cyan),
```

**`"variable"` token type** is already introduced by the Dockerfile and XML mappings above; confirm it exists in `token_style` (it may already be present from prior language support) or add:

```rust
"variable" => Style::default().fg(Color::LightMagenta),
```

---

### 4. No LSP registry changes

None of the five new languages have an LSP server configured. The `SERVERS` slice in `src/data/lsp/registry.rs` is unchanged. `start_for_context` already filters by `capabilities().has_lsp`, so the new variants are silently skipped there.

---

## Edge Case Considerations

- **Dockerfile with no extension and non-standard casing** (e.g., `DOCKERFILE`, `Dockerfile.dev`): `from_path` checks the stem case-insensitively for the base `Dockerfile` / `dockerfile` names. `Dockerfile.dev` resolves via extension `dev` → no match, then stem `Dockerfile.dev` → no match. Treat as unknown; plain gray rendering is acceptable. Document this in the manual test checklist.

- **JSONC comments**: tree-sitter-json supports `//` and `/* */` comments in JSONC mode. Map `comment` nodes to `"comment"`. Standard JSON files contain no comment nodes — no behavioral difference.

- **YAML multi-document files** (separated by `---`): tree-sitter-yaml handles these natively. Each document is a subtree; the walker processes the full tree, so all documents in a file get highlighted. No special handling needed.

- **TOML dates** (`2024-01-15T10:00:00Z`): TOML dates are a first-class type. Mapped to `"number"` (numeric literal style) since there is no dedicated `"datetime"` token type. If a visual distinction is desired later, a `"datetime"` type can be added to `token_style` without changing the engine.

- **Deeply nested XML**: tree-sitter cursor traversal is depth-first and bounded by the file size, not nesting depth. No stack overflow risk from grammar crate traversal.

- **SVG files** (`.svg` → `Xml`): SVG is valid XML; the XML grammar parses it correctly. Attribute names like `d`, `viewBox`, `fill` are highlighted as `"key"` — adequate for navigation without SVG-specific semantic coloring.

- **XML with DOCTYPE or entity declarations**: tree-sitter-xml covers the core grammar including `<!DOCTYPE`, `<!ENTITY`, and processing instructions. Map `processing_instruction` to `"keyword"` as noted above.

- **Large minified JSON** (single-line, many tokens): tree-sitter parses synchronously in `compute()`. For pathologically large files (>1MB single line), the parse may exceed the 2ms target. Consider adding a file-size guard in `parse_with` that returns `vec![]` for content over a configurable threshold (e.g. 512KB). This prevents a perceptible hitch; plain gray rendering is acceptable for machine-generated files.

- **Dockerfile `ARG`/`ENV` variable interpolation** (`${VAR}`): tree-sitter-dockerfile emits `variable` nodes for interpolated variables inside `RUN` and `COPY` arguments. These are mapped to `"variable"` for a distinct color. Variables outside `${}` syntax (bare `$VAR`) may not be emitted as a separate node type — verify against the grammar and map whatever node type is used.

- **YAML anchors and aliases** (`&anchor`, `*alias`): these are mapped to `"type"` for visual distinction from plain scalars. If the grammar does not emit a dedicated node type, they appear as plain text — acceptable fallback.

- **Multi-line strings in TOML and YAML** (triple-quoted TOML strings, block scalars in YAML): `parse_with` must split multi-line string nodes across lines. The existing multi-line node handling from the Markdown implementation (introduced in 0008) applies here — emit one `SemanticToken` per covered line.

---

## Test Considerations

- **Unit: `Language::from_path` extension detection** — `.json` → `Json`; `.yaml` and `.yml` → `Yaml`; `.toml` → `Toml`; `.xml`, `.svg`, `.xsd` → `Xml`. Unknown extension → `None`.

- **Unit: `Language::from_path` Dockerfile stem detection** — path `/project/Dockerfile` → `Dockerfile`; path `/project/dockerfile` → `Dockerfile`; path `/project/app.dockerfile` → `Dockerfile` (extension match); path `/project/main.go` → `Go` (no regression).

- **Unit: `Language::capabilities` for new variants** — assert all five new variants have `has_tree_sitter: true` and `has_lsp: false`.

- **Unit: `SyntaxEngine::compute` with `has_lsp: false` languages** — compute on `.json`, `.yaml`, `.toml`, `Dockerfile`, `.xml` files; assert `set_semantic_tokens` is called with tree-sitter tokens and no LSP request is sent to the background worker channel.

- **Unit: `json_node_type` mappings** — parse `{"key": "value", "count": 42, "active": true, "nothing": null}`; assert tokens include `"key"` for `key`, `"string"` for `"value"`, `"number"` for `42`, `"keyword"` for `true` and `null`.

- **Unit: `yaml_node_type` mappings** — parse a simple YAML mapping with a string value, integer, boolean, and comment; assert expected token types are emitted for each.

- **Unit: `toml_node_type` mappings** — parse a TOML file with a `[section]` header, bare key, string value, integer, boolean, and inline comment; assert `"type"` for the header, `"key"` for the bare key, `"string"` for the string, `"number"` for the integer, `"keyword"` for the boolean, `"comment"` for the comment.

- **Unit: `dockerfile_node_type` mappings** — parse `FROM ubuntu:22.04\nRUN apt-get update\n# comment`; assert `FROM` and `RUN` produce `"keyword"` tokens, `ubuntu` produces `"type"`, `22.04` produces `"string"`, the comment produces `"comment"`.

- **Unit: `xml_node_type` mappings** — parse `<root id="1">text</root>`; assert `root` produces `"type"`, `id` produces `"key"`, `"1"` produces `"string"`, `text` produces `"variable"`.

- **Unit: `token_style("key")`** — assert returns `Style::default().fg(Color::Cyan)` (or whatever color is chosen); assert it differs from `token_style("string")`.

- **Unit: multi-line YAML block scalar** — parse a YAML literal block scalar spanning 3 lines; assert three `SemanticToken` entries are emitted, one per line, all with the `"string"` type.

- **Unit: large file guard** (if implemented) — provide content over the size threshold; assert `parse_with` returns `vec![]` without panicking.

- **Manual checklist**:
  - Open a `.json` file — object keys are Cyan, string values are colored differently, numbers and `true`/`false`/`null` are visually distinct
  - Open a `.yaml` file — mapping keys are Cyan, scalars and comments are correctly colored, anchors/aliases are highlighted
  - Open a `.toml` file — section headers (`[section]`) appear in type color, keys are Cyan, strings/numbers/booleans are distinct, inline comments are dimmed
  - Open `Dockerfile` — instruction keywords (FROM, RUN, COPY) are highlighted, image names and tags are distinct, `ARG`/`ENV` variable references are colored
  - Open a `.svg` or `.xml` file — tag names, attribute names, and attribute values have distinct colors; `<!-- comments -->` are dimmed
  - Open a file with an unknown extension — no crash, plain gray rendering
  - Open a minified single-line JSON file over 512KB — no perceptible UI lag; graceful fallback to plain rendering if size guard is implemented

---

## Codebase Integration

- **Layer 0** (`src/data/lsp/types.rs`):
  - Add `Json`, `Yaml`, `Toml`, `Dockerfile`, `Xml` to `Language`.
  - Extend `capabilities()`, `name()`, and `from_path()` (or `from_extension()`) for all five variants.
  - No changes to `LspSharedState`, `EditorState`, or `LspServerInfo` — none of the new languages have LSP servers.
  - `detect_languages_from_dir` (introduced in 0008) is unchanged; config languages are detected by file extension at open time, not by directory manifests.

- **Layer 1** (`src/commands/syntax_engine/tree_sitter.rs`):
  - Add five arms to `parse()` dispatching to the new grammar crates.
  - Add five `*_node_type` functions mapping tree-sitter node type strings to token type strings.
  - `parse_with`, `SyntaxEngine`, `merge::merge`, and `SyntaxFrontend` are all unchanged — this work item is purely additive at the parsing layer.

- **Layer 2** (`src/frontend/tui/editor_pane.rs`):
  - Add `"key"` to `token_style`. Confirm `"variable"` is present or add it.
  - No changes to `render`, `styled_line_with_tokens`, or the `token_style` → `Style` refactor (already done in 0008).

- **`Cargo.toml`**: add five grammar crates. Pin to specific versions after verifying crate names; note that YAML and Dockerfile grammar crates have had naming churn — confirm before merging.

- **No changes to**: `app.rs`, `status_bar.rs`, `lsp_engine/`, `lsp/registry.rs`, or any Layer 1 module other than `tree_sitter.rs`. The `SyntaxEngine` compute/debounce/merge pipeline and `TuiSyntaxReceiver` wiring are untouched.
