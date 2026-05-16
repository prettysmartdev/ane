# Chord Examples

Every valid scope/component combination with worked before/after examples.

Chords have four parts: `<action><positional><scope><component>`. See
[Chord System](01-chord-system.md) for the full grammar reference. Each example
below shows the short form, long form (with parameter names in parentheses),
and a code snippet demonstrating the effect.

---

## Line scope (`l`)

Line scope targets a single line by number or cursor position. No LSP required.

### Line + Self (`ls`)

The entire line.

```
cels(target:1, value:"let y = 20;")
ChangeEntireLineSelf(target:1, value:"let y = 20;")
```

```rust
// before                          // after
let x = 10;                       let x = 10;
let old = 42;                     let y = 20;
let z = 30;                       let z = 30;
```

### Line + Beginning (`lb`)

Zero-width point at the start of the line.

```
aelb(target:0, value:"// ")
AppendEntireLineBeginning(target:0, value:"// ")
```

```rust
// before                          // after
let x = 10;                       // let x = 10;
```

### Line + End (`le`)

Zero-width point at the end of the line.

```
aele(target:0, value:" // TODO")
AppendEntireLineEnd(target:0, value:" // TODO")
```

```rust
// before                          // after
let x = 10;                       let x = 10; // TODO
```

### Line + Name (`ln`)

For Line scope, Name returns a point at the start of the line (same as
Beginning). Primarily useful with Next/Previous positionals.

```
dals(target:2)
DeleteEntireLineSelf(target:2)
```

```rust
// before                          // after
fn main() {                        fn main() {
    let x = 10;                        let x = 10;
    let old = 42;                  }
}
```

---

## Buffer scope (`b`)

Buffer scope targets the entire file. No LSP required.

### Buffer + Self (`bs`)

The full file contents.

```
yebs
YankEntireBufferSelf
```

Copies the entire file to the yank register. No modification.

### Buffer + Beginning (`bb`)

Zero-width point at the start of the file.

```
aebb(value:"use std::io;\n\n")
AppendEntireBufferBeginning(value:"use std::io;\n\n")
```

```rust
// before                          // after
fn main() {}                       use std::io;

                                   fn main() {}
```

### Buffer + End (`be`)

Zero-width point at the end of the file.

```
aebe(value:"\n\nfn helper() {}")
AppendEntireBufferEnd(value:"\n\nfn helper() {}")
```

```rust
// before                          // after
fn main() {}                       fn main() {}

                                   fn helper() {}
```

### Buffer + Name (`bn`)

For Buffer scope, Name returns a point at position (0, 0). Equivalent to
Beginning.

---

## Function scope (`f`)

Function scope targets a function or method by name or cursor position. Requires LSP.

### Function + Self (`fs`)

The entire function definition.

```
cefs(target:greet, value:"fn greet() { println!(\"hey\"); }")
ChangeEntireFunctionSelf(target:greet, value:"fn greet() { println!(\"hey\"); }")
```

```rust
// before                          // after
fn greet(name: &str) {             fn greet() { println!("hey"); }
    println!("Hello, {name}!");
}
```

### Function + Name (`fn`)

The function's identifier (name only, via LSP selectionRange).

```
cifn(target:process, value:"handle")
ChangeInsideFunctionName(target:process, value:"handle")
```

```rust
// before                          // after
fn process(data: &[u8]) {          fn handle(data: &[u8]) {
    // ...                             // ...
}                                  }
```

### Function + Contents (`fc`)

The brace-delimited body `{ ... }`.

```
cifc(target:init, value:"\n    todo!()\n")
ChangeInsideFunctionContents(target:init, value:"\n    todo!()\n")
```

```rust
// before                          // after
fn init() {                        fn init() {
    setup();                           todo!()
    connect();                     }
}
```

### Function + End (`fe`)

Zero-width point at the end of the function.

```
aefe(target:main, value:"\n\nfn helper() {}")
AppendEntireFunctionEnd(target:main, value:"\n\nfn helper() {}")
```

```rust
// before                          // after
fn main() {                        fn main() {
    run();                             run();
}                                  }

                                   fn helper() {}
```

### Function + Parameters (`fp`)

The parenthesized parameter list `(...)`.

```
cifp(target:add, value:"a: i32, b: i32")
ChangeInsideFunctionParameters(target:add, value:"a: i32, b: i32")
```

```rust
// before                          // after
fn add(x: f64, y: f64) -> f64 {   fn add(a: i32, b: i32) -> f64 {
    x + y                              x + y
}                                  }
```

### Function + Arguments (`fa`)

The parenthesized argument list at a **call site** of the named function,
searched outside the function's own definition.

```
cifa(target:process, value:"new_data, true")
ChangeInsideFunctionArguments(target:process, value:"new_data, true")
```

```rust
// before                          // after
fn process(data: &[u8]) { ... }   fn process(data: &[u8]) { ... }

fn main() {                        fn main() {
    process(old_data);                 process(new_data, true);
}                                  }
```

---

## Variable scope (`v`)

Variable scope targets a variable or constant declaration. Requires LSP.
The cursor can be anywhere within the declaration line (on the keyword, name,
or value) and the scope resolves to the full statement.

### Variable + Self (`vs`)

The entire variable declaration.

```
devs(target:temp)
DeleteEntireVariableSelf(target:temp)
```

```rust
// before                          // after
let count = 0;                     let count = 0;
let temp = calculate();
let result = finish();             let result = finish();
```

### Variable + Name (`vn`)

The variable's identifier. Cursor position does not matter -- even if the
cursor is on the value side, the name is correctly identified via LSP
documentSymbol or text-based fallback.

```
cevn(target:old_name, value:"new_name")
ChangeEntireVariableName(target:old_name, value:"new_name")
```

```rust
// before                          // after
let old_name = compute();          let new_name = compute();
```

### Variable + Value (`vv`)

The right-hand side of the `=` assignment.

```
cevv(target:config, value:"Config::default()")
ChangeEntireVariableValue(target:config, value:"Config::default()")
```

```rust
// before                          // after
let config = load_from_file();     let config = Config::default();
```

### Variable + End (`ve`)

Zero-width point at the end of the variable declaration.

```
aeve(target:items, value:"\nlet count = items.len();")
AppendEntireVariableEnd(target:items, value:"\nlet count = items.len();")
```

```rust
// before                          // after
let items = vec![1, 2, 3];        let items = vec![1, 2, 3];
                                   let count = items.len();
```

---

## Struct scope (`s`)

Struct scope targets a struct or enum definition. Requires LSP.

### Struct + Self (`ss`)

The entire struct/enum definition.

```
yess(target:Config)
YankEntireStructSelf(target:Config)
```

Copies the full `struct Config { ... }` definition. No modification.

### Struct + Name (`sn`)

The struct's identifier.

```
cesn(target:OldName, value:"NewName")
ChangeEntireStructName(target:OldName, value:"NewName")
```

```rust
// before                          // after
struct OldName {                   struct NewName {
    field: i32,                        field: i32,
}                                  }
```

### Struct + Contents (`sc`)

The brace-delimited body `{ ... }`.

```
cisc(target:Point, value:"\n    x: f64,\n    y: f64,\n    z: f64,\n")
ChangeInsideStructContents(target:Point, value:"\n    x: f64,\n    y: f64,\n    z: f64,\n")
```

```rust
// before                          // after
struct Point {                     struct Point {
    x: f64,                            x: f64,
    y: f64,                            y: f64,
}                                      z: f64,
                                   }
```

### Struct + End (`se`)

Zero-width point at the end of the struct definition.

```
aese(target:Color, value:"\n\nimpl Color {\n    fn new() -> Self { todo!() }\n}")
AppendEntireStructEnd(target:Color, value:"\n\nimpl Color {\n    fn new() -> Self { todo!() }\n}")
```

```rust
// before                          // after
struct Color {                     struct Color {
    r: u8, g: u8, b: u8,              r: u8, g: u8, b: u8,
}                                  }

                                   impl Color {
                                       fn new() -> Self { todo!() }
                                   }
```

---

## Member scope (`m`)

Member scope targets a struct field or enum variant. Requires LSP. Use
`parent:<name>` to disambiguate when the same field name appears in
multiple structs.

### Member + Self (`ms`)

The entire field or variant declaration.

```
dems(target:deprecated_field, parent:Config)
DeleteEntireMemberSelf(target:deprecated_field, parent:Config)
```

```rust
// before                          // after
struct Config {                    struct Config {
    name: String,                      name: String,
    deprecated_field: bool,        }
}
```

### Member + Name (`mn`)

The field or variant identifier.

```
cemn(target:x, parent:Point, value:"horizontal")
ChangeEntireMemberName(target:x, parent:Point, value:"horizontal")
```

```rust
// before                          // after
struct Point {                     struct Point {
    x: f64,                            horizontal: f64,
    y: f64,                            y: f64,
}                                  }
```

### Member + Value (`mv`)

The type annotation (for struct fields) or the variant payload (for enum
variants).

```
cemv(target:count, parent:Stats, value:" usize")
ChangeEntireMemberValue(target:count, parent:Stats, value:" usize")
```

```rust
// before                          // after
struct Stats {                     struct Stats {
    count: i32,                        count: usize,
}                                  }
```

### Member + End (`me`)

Zero-width point at the end of the field/variant declaration.

```
aeme(target:name, parent:User, value:"\n    email: String,")
AppendEntireMemberEnd(target:name, parent:User, value:"\n    email: String,")
```

```rust
// before                          // after
struct User {                      struct User {
    name: String,                      name: String,
}                                      email: String,
                                   }
```

---

## Delimiter scope (`d`)

Delimiter scope targets the innermost matching delimiter pair surrounding the
cursor: parentheses `()`, braces `{}`, brackets `[]`, double quotes `""`,
single quotes `''`, or backticks ` `` `. No LSP required. Purely text-based
scanning.

### Delimiter + Self (`ds`)

The full delimiter pair including the delimiters themselves.

```
ceds(value:"[1, 2, 3]")
ChangeEntireDelimiterSelf(value:"[1, 2, 3]")
```

```rust
// before (cursor on "old")        // after
let x = (old, data);               let x = [1, 2, 3];
```

### Delimiter + Contents (`dc`)

The text between the delimiters, exclusive of the delimiters.

```
cidc(value:"x, y, z")
ChangeInsideDelimiterContents(value:"x, y, z")
```

```rust
// before (cursor on "a")          // after
call(a, b, c)                      call(x, y, z)
```

### Delimiter + Beginning (`db`)

Zero-width point at the opening delimiter.

```
jtdb
JumpToDelimiterBeginning
```

Moves the cursor to the position of the opening delimiter. TUI-only.

### Delimiter + End (`de`)

Zero-width point at the closing delimiter.

```
jtde
JumpToDelimiterEnd
```

Moves the cursor to the position of the closing delimiter. TUI-only.

### Delimiter + Name (`dn`)

The opening delimiter character itself. Useful for identifying or changing
which delimiter type is in play.

```
cedn(value:"{")
ChangeEntireDelimiterName(value:"{")
```

```rust
// before (cursor inside parens)   // after
let v = (1, 2, 3);                 let v = {1, 2, 3);
```

Note: changing a single delimiter character without its pair is rarely
useful on its own -- this component exists primarily for inspection (Yank)
or as part of a multi-chord workflow.

---

## Positional variations

The examples above primarily use `e` (Entire) and `i` (Inside). Here are the
other positionals demonstrated with various scope/component pairs.

### Before (`b`) -- text before the component

```
dbfs(target:main)
DeleteBeforeFunctionSelf(target:main)
```

```rust
// before                          // after
use std::io;                       fn main() {
                                       run();
fn main() {                        }
    run();
}
```

### After (`a`) -- text after the component

```
dafs(target:first)
DeleteAfterFunctionSelf(target:first)
```

```rust
// before                          // after
fn first() {}                      fn first() {}
fn second() {}
fn third() {}
```

### Until (`u`) -- from cursor to the start of the component

Requires a cursor position. Range ends *before* the target.

```
cufb(cursor:"2,0")
ChangeUntilFunctionBeginning(cursor:"2,0")
```

Deletes from cursor position (2, 0) up to (but not including) the start of
the function's beginning point.

### To (`t`) -- from cursor through the end of the component

Like Until but with inclusive endpoint semantics.

```
ctfe(cursor:"1,4")
ChangeToFunctionEnd(cursor:"1,4")
```

```rust
// before (cursor at line 1, col 4)   // after
fn example() {                         fn example() {
    first_call();                      }
    second_call();
}
```

### Outside (`o`) -- everything except the component

Returns the scope minus the component. May produce two disjoint ranges (head
and tail).

```
dofp(target:add)
DeleteOutsideFunctionParameters(target:add)
```

```rust
// before                          // after
fn add(x: i32, y: i32) -> i32 {   (x: i32, y: i32)
    x + y
}
```

### Next (`n`) -- advance to the next symbol

```
jnfn
JumpNextFunctionName
```

Jumps the cursor to the name of the next function after the current cursor
position. TUI-only.

```
ynfs
YankNextFunctionSelf
```

Yanks the entire next function after the cursor.

### Previous (`p`) -- go to the previous symbol

```
jpfn
JumpPreviousFunctionName
```

Jumps the cursor to the name of the previous function before the current
cursor position. TUI-only.

---

## Jump action (`j`)

Jump moves the cursor without modifying the buffer. TUI-only -- the CLI
rejects Jump chords before any file I/O. Jump always transitions to Edit
mode after landing.

```
jtfc    JumpToFunctionContents      -- jump to the opening brace of the current function
jnfn    JumpNextFunctionName        -- jump to the name of the next function
jpfn    JumpPreviousFunctionName    -- jump to the name of the previous function
jtbe    JumpToBufferEnd             -- jump to end of file
jofb    JumpOutsideFunctionBeginning -- jump to the line before the function
jofe    JumpOutsideFunctionEnd      -- jump to the line after the function
jtdb    JumpToDelimiterBeginning    -- jump to the opening delimiter
jtde    JumpToDelimiterEnd          -- jump to the closing delimiter
```

### Jump + Outside

Jump with the Outside positional requires Beginning or End to specify
direction. `jofb` jumps to before the function, `jofe` jumps to after it.
Other components with Outside are invalid.

---

## List Action

The `List` action collects matching items and displays them in an interactive overlay (TUI) or prints them to stdout (CLI). Unlike modification actions, List produces no diff.

### List + Function Name

Lists all function names in the buffer or a specific scope.

```
lefn                        ListEntireFunctionName
lafn(cursor:"5,0")          ListAfterFunctionName
llfn                        ListLastFunctionName
lffn                        ListFirstFunctionName
```

In TUI, this opens a scrollable dialog:

```
┌─────────────────────────────────────┐
│ List Results                        │
├─────────────────────────────────────┤
│ setup                    (line 1)   │
│ initialize               (line 8)   │
│ process_data             (line 15)  │
│ cleanup                  (line 22)  │
└─────────────────────────────────────┘
```

Use arrow keys to navigate, press Enter to jump, Escape to close.

In CLI, this prints to stdout:

```
$ ane exec file.rs lefn
1:1   setup
8:1   initialize
15:1  process_data
22:1  cleanup
```

### List + Function Definition

Lists function signatures (declarations without bodies).

```
lefd                        ListEntireFunctionDefinition
```

```rust
// buffer with three functions
pub fn setup() {
    // body
}

fn initialize(config: Config) -> Result<()> {
    // body
}

async fn process(&mut self) -> Vec<Item> {
    // body
}
```

With `lefd`, the dialog shows:

```
┌─────────────────────────────────────────────┐
│ List Results                                │
├─────────────────────────────────────────────┤
│ pub fn setup()                  (line 1)    │
│ fn initialize(config: Config..  (line 5)    │
│ async fn process(&mut self)..   (line 10)   │
└─────────────────────────────────────────────┘
```

Each item displays the full function signature up to (but not including) the opening brace.

### List + Struct Name

Lists struct (and enum) names, respecting the positional filter.

```
lesn                        ListEntireStructName
lisn(cursor:"12,0")         ListInsideStructName
```

---

## Word Component

A word is a maximal run of non-whitespace characters. Word is text-based and requires no LSP.

### Line + Word

#### Jump to word

```
jnlw                        JumpNextLineWord
jplw                        JumpPreviousLineWord
jflw                        JumpFirstLineWord
jllw                        JumpLastLineWord
jElw(cursor:"0,8")          JumpEntireLineWord
```

```rust
// before: cursor on first word
let config = Config::default();
│
│ jflw: jumps to "let" (first word)
│ jnlw: jumps to "config" (next word)
│ jllw: jumps to "default" (last word)
```

#### Change a word

```
celw(cursor:"0,4", value:"mut x")          ChangeEntireLineWord
```

```rust
// before
let value = 42;
    │
    cursor is on "value"

// after
let mut x = 42;
```

### Buffer + Word

Lists or operates on words across the entire file.

```
lelw                        ListEntireLineWord
lewb(target:"config")       YankEntireBufferWord
```

---

## Definition Component

The Definition component targets the declaration part of a scope, excluding the body.

### Function Definition

The definition includes visibility, keyword, name, parameters, and return type, but excludes the brace block.

```
lefd                        ListEntireFunctionDefinition
cefd(target:process)        ChangeEntireFunctionDefinition
yevd(target:init)           YankEntireFunctionDefinition (same as lefd, but only one item)
defd(target:helper)         DeleteEntireFunctionDefinition
```

```rust
// before: changing the signature of process()
pub fn process(old_arg: String) -> bool {
    println!("working");
    true
}

// chord: cefd(target:process, value:"fn process(new_arg: &str) -> Result<()>")
// after
fn process(new_arg: &str) -> Result<()> {
    println!("working");
    true
}
```

The body remains untouched; only the signature is replaced.

For trait methods and extern functions (no brace body), Definition is the full scope excluding the trailing semicolon:

```rust
// trait method
fn required_fn(&self) -> String;
          │
          Definition component includes everything except the semicolon

// after cefd: entire line is replaced with the new signature
```

### Variable Definition

For variables, Definition includes the keyword, name, and type annotation, but excludes the assignment (RHS).

```
yevd(target:config)         YankEntireVariableDefinition
```

```rust
// buffer
let config: Config = Config::default();

// yevd(target:config) yanks: "let config: Config"
// (excludes the "= Config::default()" part)

let my_count: i32 = 0;

// yevd(target:my_count) yanks: "let my_count: i32"
```

### Struct Definition

For structs and enums, Definition includes visibility, keyword, name, and generic parameters, but excludes the brace block.

```
lefd                        ListEntireStructDefinition
cesd(target:Config)         ChangeEntireStructDefinition
```

```rust
// before: changing a struct's signature
pub struct Config<T> {
    field: T,
    count: usize,
}

// chord: cesd(target:Config, value:"struct SimpleConfig")
// after
struct SimpleConfig {
    field: T,
    count: usize,
}
```

---

## Last and First Positionals

These select the last (or first) occurrence of a component within a scope.

### Last Positional

```
jlfn                        JumpLastFunctionName
jlfn(target:outer)          JumpLastFunctionName (inside outer function)
celmn                       ChangeEntireLastMemberName
```

```rust
// buffer with multiple functions
fn outer() {
    fn inner_one() { }
    let x = 10;
    fn inner_two() { }
}

// with cursor inside outer:
jlfn     → jumps to inner_two (last function in the buffer)
jlfn(target:outer) → jumps to inner_two (last function inside outer)
```

### First Positional

```
jffn                        JumpFirstFunctionName
jffn(target:outer)          JumpFirstFunctionName (inside outer function)
```

```rust
// buffer with multiple functions
fn outer() {
    fn inner_one() { }
    let x = 10;
    fn inner_two() { }
}

// with cursor inside outer:
jffn     → jumps to inner_one (first function in the buffer)
jffn(target:outer) → jumps to inner_one (first function inside outer)
```

### First and Last with Word

```
jflw                        JumpFirstLineWord
jllw                        JumpLastLineWord
```

```rust
// line: "  let mut config = Config::new();"
//         │                                  │
//       first                              last

jflw  →  cursor jumps to "let"
jllw  →  cursor jumps to "new"
```

---

## Scope/component validity matrix

A chord with an invalid scope/component combination is rejected at parse time.

|              | Beginning | Contents | End | Value | Parameters | Arguments | Name | Self | Word | Definition |
|--------------|:---------:|:--------:|:---:|:-----:|:----------:|:---------:|:----:|:----:|:----:|:----------:|
| **Line**       | Y         | --       | Y   | --    | --         | --        | Y    | Y    | Y    | --         |
| **Buffer**     | Y         | --       | Y   | --    | --         | --        | Y    | Y    | Y    | --         |
| **Function**   | --        | Y        | Y   | --    | Y          | Y         | Y    | Y    | --   | Y          |
| **Variable**   | --        | --       | Y   | Y     | --         | --        | Y    | Y    | --   | Y          |
| **Struct**     | --        | Y        | Y   | --    | --         | --        | Y    | Y    | --   | Y          |
| **Member**     | --        | --       | Y   | Y     | --         | --        | Y    | Y    | --   | --         |
| **Delimiter**  | Y         | Y        | Y   | --    | --         | --        | Y    | Y    | --   | --         |

---

[<- Chord System](01-chord-system.md) | [Next: Using the TUI ->](03-using-the-tui.md)
