# Listing, Words, and Definition Signatures

This guide covers four new chord grammar elements that enhance navigation and exploration:

- **List action** (`l`) — collect and browse matching items interactively
- **Word component** (`w`) — work with whitespace-delimited words
- **Definition component** (`d`) — target declaration signatures separately from bodies
- **First and Last positionals** (`f`, `l`) — jump to the first or last item in a scope

---

## List Action: Exploring Code Structure

The `List` action is an exploratory tool. Instead of modifying code, it gathers all matching items and presents them for navigation.

### Discovering functions with `lefn`

When you want to understand a file's structure at a glance, use `lefn` (List Entire Function Name):

**TUI behavior:**
- Opens a scrollable dialog with all function names and their line numbers
- Use **Up/Down arrows** to browse
- Press **Enter** to jump the cursor to the selected function
- Press **Escape** to close without navigating

**CLI behavior:**
```bash
$ ane exec src/lib.rs lefn
1:1   init
15:1  fetch_data
32:1  process
48:1  cleanup
```

This is useful in scripts: pipe to `wc -l` to count functions, parse line numbers for further targeted edits, or integrate with other Unix tools.

### Scoped listing with positional filters

List respects positional filters, allowing you to narrow results:

```
lafn(cursor:"20,0")   # list functions after line 20
lbfn(cursor:"20,0")   # list functions before line 20
lffn               # list first function in buffer
llfn               # list last function in buffer
lisn(cursor:"15,0")   # list struct names inside current scope
```

This is powerful for navigation in large files:
- `lafn` shows "what comes next?" when jumping around
- `lbfn` shows "what came before?"  
- `lffn` / `llfn` quickly land on entry points or wrap-up code

### Listing definitions for API overview

`lefd` (List Entire Function Definition) shows signatures without bodies:

```rust
// file: data_processor.rs
pub fn init(config: &Config) -> Result<()> {
    // 50 lines of setup
}

async fn fetch_remote(url: &str) -> Result<Vec<u8>> {
    // 30 lines of HTTP logic
}

fn parse_response(data: &[u8]) -> Item {
    // 40 lines of parsing
}
```

Running `lefd` lists:
```
1:1   pub fn init(config: &Config) -> Result<()>
52:1  async fn fetch_remote(url: &str) -> Result<Vec<u8>>
83:1  fn parse_response(data: &[u8]) -> Item
```

Now you understand the API and parameter contracts without reading implementations. Pair this with `jlfn` to jump directly to any function.

### Member and Variable listing

List works with any scope/component combo:

```
lemn(target:Point)    # list all member names in struct Point
levn              # list all variable names in buffer
```

Use cases:
- Enumerate struct fields before refactoring
- Find all module-level constants
- Explore what variables exist at a scope before modifying

---

## Word Component: Line-level Precision

The `Word` component targets whitespace-delimited words. No LSP required — it's pure text-based.

### Practical word operations

Rename a variable on the current line without specifying its name:

```rust
// line: let count = 42;
//            │
//        cursor here

celw(value:"total")
```

Result: `let total = 42;`

Navigate within a line:

```rust
// line: config.set_timeout(5000).start();
//       │                         │
//      first                     last

jflw  # cursor to "config"
jllw  # cursor to "start"
jnlw  # cursor to next word from current position
```

### Word vs. Name component

- **Name** — the identifier of a symbol (requires LSP, works across scopes)
- **Word** — any whitespace run (text-based, confined to Line or Buffer scope)

Example: In `foo_bar`, Name and Word behave differently:

```rust
let foo_bar = 42;
    │
    Name: "foo_bar" (entire identifier)
    Word: "foo_bar" (same, since no whitespace within)

foo_bar.method()
        │
    Name: "method" (the symbol)
    Word: "method" (same)

fn foo_bar() {}
   │       │
   Name: "foo_bar"
   Word: "foo" (if cursor at start), then "bar"
```

### Editing with words

Change the second word on a line:

```rust
// line: "let config = Config::default();"
//            │                 │
//         second              fifth

jnlw           # jump to second word
celw(value:"settings")  # change it
```

Result: `let settings = Config::default();`

---

## Definition Component: Signatures Without Bodies

The Definition component separates a scope's declaration (signature) from its implementation (body). This is useful for high-level refactoring and documentation.

### Function definitions

A function's Definition includes everything up to (not including) the opening brace:

```rust
pub fn process(input: &str) -> Result<Output> {
└─────────────────────────────────────────┘
         This is the Definition

    // 100 lines of implementation
}
```

Use `cefd` to change a function's signature while preserving its body:

```rust
// before
fn old_name(x: i32) -> String {
    // lots of code
}

// chord: cefd(target:old_name, value:"fn new_name(y: u32) -> i32")
// after
fn new_name(y: u32) -> i32 {
    // lots of code
}
```

### Variable definitions

For variables, Definition captures the declaration but not the assignment:

```rust
let count: i32 = 0;
└──────────────┘
    Definition (excludes "= 0;")

let config = Config::default();
└──────────┘
    Definition (excludes "= Config::default();")
```

Yank a variable's type annotation without its value:

```rust
let threshold: i32 = 1000;

// yevd(target:threshold) yanks: "let threshold: i32"
```

Then paste it to declare a similar variable elsewhere:

```rust
// ... later in code
let min_threshold: i32 = 100;  // can reuse the type
```

### Struct/enum definitions

A struct or enum Definition includes generics but excludes the field list:

```rust
pub struct Cache<K, V> {
│          ─────────────
│          Definition
│
└────────────────────────┘
    Entire scope (Definition + Contents)

    field1: K,
    field2: V,
}
```

Rename a struct and update its generics while keeping fields:

```rust
// before
struct OldCache<T> {
    data: T,
    size: usize,
}

// chord: cesd(target:OldCache, value:"struct NewCache<T, E>")
// after
struct NewCache<T, E> {
    data: T,
    size: usize,
}
```

Use `lefd` to view all struct signatures and their generic parameters:

```
1:5   pub struct Config<T>
12:3  struct Inner
25:1  pub struct Cache<K, V, H>
```

This gives you the "table of contents" for your data structures.

---

## First and Last Positionals: Boundary Jumps

The `First` and `Last` positionals let you jump to the first or last occurrence of a component without scanning.

### Jump to first/last function

```rust
fn entry_point() { }
fn utility_1() { }
fn utility_2() { }
fn cleanup() { }

jffn   # jump to "entry_point" (first function)
jlfn   # jump to "cleanup" (last function)
```

Useful patterns:
- `jffn` to go to the main entry point
- `jlfn` to jump to utility or cleanup code
- Faster than scrolling in large files

### First/last word on a line

```rust
// line: "let config = Config::new().build().finalize();"
//        │                                              │
//      first                                           last

jflw   # jump to "let"
jllw   # jump to "finalize"
```

### Scoped first/last

```
jlfn(target:MyStruct)   # last function defined inside MyStruct
jffn(target:module)     # first function in module scope
```

---

## Combining Features: Practical Workflows

### Workflow 1: Refactor an exported API

You're updating a public struct's interface.

1. `lefd(target:PublicStruct)` → List its definition to see current signature and generics
2. `cesd(target:PublicStruct, value:"struct NewName<Updated>")` → Change signature
3. `jlfn` → Jump to the last function (helpers/factories) and verify they match the new interface
4. `lafn(cursor:"...,0")` → List functions after the struct to see dependents

### Workflow 2: Understand a legacy file

You inherit a 2000-line Rust file.

1. `lefn` → Open the function list dialog, read through all function names
2. `lefd` → See function signatures to understand the API
3. `jlfn` → Jump to the end (likely `main()` or a run function)
4. `jffn` → Jump to the start (likely setup/init)
5. Use List with First/Last to navigate between key sections

### Workflow 3: Extract and refactor a signature

You want to move a function's signature to a trait.

1. `jlfn(target:target_scope)` → Jump to the function you want to extract
2. `yevd(target:func)` → Yank its full definition (signature)
3. Navigate to trait definition
4. Paste; edit as needed

Or use `cefd` on the original to change its signature while tests keep the body working.

### Workflow 4: Bulk rename with word precision

Rename a variable name that appears multiple times on one line.

```rust
let x = x + x * 2;
    │   │   │

celw(cursor:"0,10", value:"y")  # change one "x" to "y"
```

Then repeat for other lines without worrying about LSP symbol precision.

---

## Implementation Notes

### Word boundaries

- **Whitespace detection**: uses Unicode `char::is_whitespace()`, not ASCII-only
- **Tab handling**: tabs are whitespace; column offsets account for tab width
- **Empty lines**: word operations on empty lines return an error
- **Cursor on whitespace**: defaults to next word right; if none, falls back to previous

### Definition extraction

- **Attributes and comments**: NOT included (start at first keyword)
- **Generics and where clauses**: included
- **Line wrapping**: definitions can span multiple lines; extracted as single contiguous range
- **Trait methods**: end at `;`, not `{`
- **Unit structs**: defined as `struct Name;` → Definition is `struct Name` (semicolon excluded)

### List and positional filtering

- **Empty results**: TUI shows "No results" dialog; CLI prints nothing (exit 0)
- **Outside positional**: invalid with List (no clear filtering semantics)
- **Cursor-dependent filters**: `After`, `Before`, `Next`, `Previous`, `Inside`, `Until`, `To` all require `cursor` argument

---

## See Also

- [Chord System](01-chord-system.md) — full grammar reference
- [Chord Examples](02-chord-examples.md) — exhaustive before/after examples
- [Using the TUI](03-using-the-tui.md) — keybindings and list dialog behavior
