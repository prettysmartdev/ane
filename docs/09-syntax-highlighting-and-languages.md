# Syntax Highlighting and Language Support

ane uses a two-tier syntax highlighting system that combines **tree-sitter** (fast, structural) and **LSP semantic tokens** (slow, type-aware) to provide immediate visual feedback and richer highlighting once your language server is ready.

This guide explains how syntax highlighting works, which languages are supported, and how ane handles multi-language projects.

---

## How syntax highlighting works

### Two-tier pipeline

When you open a file, ane runs two highlighting passes in parallel:

1. **Tree-sitter (synchronous, ~2ms)**
   - Parses the code structure immediately
   - Highlights keywords, strings, types, comments, etc.
   - Results appear instantly — no waiting for language servers
   - Works the same way every time (structural, not semantic)

2. **LSP semantic tokens (asynchronous, 300ms+)**
   - Runs in the background after a 300ms pause (to avoid constant re-requests while you're typing)
   - Uses type information from your language server for richer colors
   - Overlays on top of tree-sitter results
   - Examples: type aliases, parameters, const variables in different colors than regular identifiers
   - Only available for languages with LSP servers

### Why two tiers?

Tree-sitter is fast and language-agnostic — it works offline and scales instantly to large files. LSP is slower but smarter — it uses your project's type information and compiler knowledge to color code more precisely. By running both, you get the best of both worlds:

- **Immediate visual feedback** via tree-sitter when you open a file
- **Richer semantic coloring** via LSP once the server has time to analyze the code
- **No typing lag** — tree-sitter re-highlights at 2ms per keystroke, LSP is debounced

---

## Supported languages

| Language | Tree-sitter | LSP Server | Detection |
|----------|:-----------:|:----------:|-----------|
| Rust | ✓ | ✓ (rust-analyzer) | `Cargo.toml` |
| Go | ✓ | ✓ (gopls) | `go.mod`, `go.work` |
| TypeScript / JavaScript | ✓ | ✓ (vtsls) | `package.json`, `tsconfig.json` |
| Python | ✓ | ✓ (basedpyright) | `pyproject.toml`, `pyrightconfig.json`, `setup.py` |
| Markdown | ✓ | — | `.md`, `.markdown` files |

All listed languages have tree-sitter support and appear with syntax colors on open. Languages with LSP servers (all except Markdown) gain semantic highlighting once the server initializes.

---

## Language detection and servers

### Auto-detection

ane scans your project for manifest files to determine which languages are in use:

- **Rust** looks for `Cargo.toml` (workspace or member crate)
- **Go** looks for `go.mod` or `go.work`
- **TypeScript** looks for `package.json` or `tsconfig.json`
- **Python** looks for `pyproject.toml`, `pyrightconfig.json`, or `setup.py`
- **Markdown** requires no detection — any `.md` or `.markdown` file gets highlighting

If you open a single file (not a directory), ane detects the language from the file extension and can serve LSP chords immediately without project context.

### Workspace roots

For each detected language, ane determines the appropriate "workspace root" to pass to the language server:

- **Rust** uses the topmost `Cargo.toml` (workspace root)
- **Go** prefers `go.work` (if it exists) then the topmost `go.mod`
- **TypeScript** uses the nearest `tsconfig.json` (to match how tsc resolves projects)
- **Python** uses the nearest `pyproject.toml` or `pyrightconfig.json`

This ensures language servers see the correct project scope.

---

## Server status in multi-language projects

When your project has multiple languages with LSP servers, the status bar shows one compact indicator per language:

```
go:● ts:◌ py:✖
```

| Symbol | Meaning |
|--------|---------|
| `●` (green dot) | Server running, LSP chords available |
| `◌` (empty circle) | Server installing or starting |
| `✖` (red X) | Server failed to install or crashed |
| (omitted) | No server defined for this language |

**Example walkthrough:**
- `go:●` means the Go server (gopls) is running
- `ts:◌` means the TypeScript server (vtsls) is starting or installing
- `py:✖` means the Python server (basedpyright) failed to start

Hover over or check the center message area to see details on failures or progress.

---

## Auto-installation

When ane detects a language with an LSP server, it automatically installs the server if it's not found on your system. Installations happen **one at a time** in the background:

- **Rust**: runs `rustup component add rust-analyzer` (assumes rustup is installed)
- **Go**: runs `go install golang.org/x/tools/gopls@latest` (assumes Go is installed)
- **TypeScript**: runs `npm install -g @vtsls/language-server` (assumes Node.js/npm is installed)
- **Python**: runs `pip install basedpyright` (assumes Python/pip is installed)

**What if the toolchain is missing?**

If you don't have a language's toolchain installed (e.g., no Go on PATH, no Python installed), the server will fail to install and show `✖` in the status bar. ane does not manage toolchain installation — you'll need to install Go, Node.js, or Python separately.

---

## Per-language syntax examples

### Rust

Tree-sitter highlights keywords, types, and strings immediately. Once rust-analyzer starts, you see richer semantic colors:

- Function names colored differently than method calls
- Type aliases in a distinct color from structs
- Lifetime parameters highlighted as type constructs
- Const variables in a different color from regular variables

### Go

Interfaces and exported symbols get semantic treatment once gopls starts. The tree-sitter baseline handles keywords, comments, and string literals right away.

### TypeScript / JavaScript

Tree-sitter catches the structural layer (keywords, strings, JSX tags). vtsls adds type-aware coloring once it analyzes your code — different colors for imports, type definitions, and enum members.

**Note**: `.tsx` files (React) are handled correctly — vtsls receives `"typescriptreact"` as the language ID, not plain `"typescript"`, so completions and go-to-definition work for JSX elements.

### Python

basedpyright detects virtual environments automatically (if configured in `pyrightconfig.json` or `pyproject.toml`). Tree-sitter gives you immediate highlighting; basedpyright adds type-aware colors for function parameters, type annotations, and class definitions.

### Markdown

Markdown is **tree-sitter only** — no LSP server. But it has its own rich syntax highlighting:

| Element | Color | Style |
|---------|-------|-------|
| Headings (`#`, `##`, etc.) | Yellow | Bold |
| Strong emphasis (`**text**`) | White | Bold |
| Emphasis (`*text*`) | White | Italic* |
| Code spans and fenced blocks | Green | — |
| Links and images | Cyan | Underlined |
| Block quotes | Dark gray | — |
| List markers (`-`, `*`, `1.`) | Yellow | — |
| Horizontal rules | Dark gray | — |

*Italic requires terminal support; some terminals fall back to normal style.*

---

## Syntax highlighting in different modes

### TUI mode

As you type and navigate:
- Tree-sitter updates **on every keystroke** (~2ms latency)
- LSP semantic tokens update ~300ms after you **stop typing**
- When you switch files, tree-sitter delivers cached results immediately
- Markdown files render with full syntax highlighting (no LSP needed)

### Exec mode

One-shot edits receive highlighting in the unified diff output based on tree-sitter and any available LSP tokens. No live debouncing — the server is queried once per execution.

---

## Troubleshooting

### Colors look flat / no semantic highlighting

**Common causes:**
1. The language server hasn't started yet — check the status bar (wait for it to show `●`)
2. The server failed to install — status bar shows `✖` — check that you have the required toolchain (Node.js, Go, Python, etc.)
3. The workspace root was detected incorrectly — try opening a directory instead of a single file

### Tree-sitter colors don't match what I expect

Tree-sitter uses a generic set of token types (keyword, type, function, string, etc.) across all languages. If your color scheme uses the same colors for all types, you might not see much variation. Once LSP starts, semantic tokens provide more language-specific coloring.

### No colors at all

Check the file extension and ensure ane recognizes the language:
- `.rs` → Rust
- `.go` → Go
- `.ts`, `.tsx`, `.js`, `.jsx` → TypeScript / JavaScript
- `.py` → Python
- `.md`, `.markdown` → Markdown

If the extension is unrecognized, ane renders the file in plain gray. Edit the file anyway — chords still work (except LSP-scoped ones).

### Go/TypeScript/Python completions or go-to-definition don't work

Wait for the status bar to show `●` for that language. If the status bar shows `◌` (installing), wait a bit longer. If it shows `✖`, the server failed to install — check that you have the required toolchain:

- **Go**: requires Go 1.17+ (`go install golang.org/x/tools/gopls@latest`)
- **TypeScript**: requires Node.js/npm (`npm install -g @vtsls/language-server`)
- **Python**: requires Python 3.8+ (`pip install basedpyright`)

---

## Performance notes

- Tree-sitter parsing is **O(n) in file size** but very fast in practice (~2–5ms for files < 10K lines)
- LSP requests are **debounced 300ms** after the last keystroke to avoid flooding the language server
- Highlighting **never blocks typing** — tree-sitter updates and then LSP updates asynchronously
- Status bar updates are **instant** — no latency when switching between server states

Large files (> 50K lines) may see tree-sitter take longer, but still well under the frame budget. LSP servers are responsible for their own performance — some may be slower than others on complex codebases.

---

[<- Compared to Other Editors](08-compared-to-other-editors.md)
