# Chord Examples

Every valid scope/component combination with worked before/after examples.

Chords have four parts: `<action><positional><scope><component>`. Each example
shows the short form, long form (with parameter names in parentheses), and a
code snippet demonstrating the effect.

---

## Line scope (`l`)

Line scope targets a single line by number or cursor position. No LSP required.

### Line + Self (`ls`)

The entire line.

```
cels(line:1, value:"let y = 20;")
ChangeEntireLineSelf(line:1, value:"let y = 20;")
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
aelb(line:0, value:"// ")
AppendEntireLineBeginning(line:0, value:"// ")
```

```rust
// before                          // after
let x = 10;                       // let x = 10;
```

### Line + End (`le`)

Zero-width point at the end of the line.

```
aele(line:0, value:" // TODO")
AppendEntireLineEnd(line:0, value:" // TODO")
```

```rust
// before                          // after
let x = 10;                       let x = 10; // TODO
```

### Line + Name (`ln`)

For Line scope, Name returns a point at the start of the line (same as
Beginning). Primarily useful with Next/Previous positionals.

```
dals(line:2)
DeleteEntireLineSelf(line:2)
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
cefs(function:greet, value:"fn greet() { println!(\"hey\"); }")
ChangeEntireFunctionSelf(function:greet, value:"fn greet() { println!(\"hey\"); }")
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
cifn(function:process, value:"handle")
ChangeInsideFunctionName(function:process, value:"handle")
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
cifc(function:init, value:"\n    todo!()\n")
ChangeInsideFunctionContents(function:init, value:"\n    todo!()\n")
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
aefe(function:main, value:"\n\nfn helper() {}")
AppendEntireFunctionEnd(function:main, value:"\n\nfn helper() {}")
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
cifp(function:add, value:"a: i32, b: i32")
ChangeInsideFunctionParameters(function:add, value:"a: i32, b: i32")
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
cifa(function:process, value:"new_data, true")
ChangeInsideFunctionArguments(function:process, value:"new_data, true")
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
devs(variable:temp)
DeleteEntireVariableSelf(variable:temp)
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
cevn(variable:old_name, value:"new_name")
ChangeEntireVariableName(variable:old_name, value:"new_name")
```

```rust
// before                          // after
let old_name = compute();          let new_name = compute();
```

### Variable + Value (`vv`)

The right-hand side of the `=` assignment.

```
cevv(variable:config, value:"Config::default()")
ChangeEntireVariableValue(variable:config, value:"Config::default()")
```

```rust
// before                          // after
let config = load_from_file();     let config = Config::default();
```

### Variable + End (`ve`)

Zero-width point at the end of the variable declaration.

```
aeve(variable:items, value:"\nlet count = items.len();")
AppendEntireVariableEnd(variable:items, value:"\nlet count = items.len();")
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
yess(struct:Config)
YankEntireStructSelf(struct:Config)
```

Copies the full `struct Config { ... }` definition. No modification.

### Struct + Name (`sn`)

The struct's identifier.

```
cesn(struct:OldName, value:"NewName")
ChangeEntireStructName(struct:OldName, value:"NewName")
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
cisc(struct:Point, value:"\n    x: f64,\n    y: f64,\n    z: f64,\n")
ChangeInsideStructContents(struct:Point, value:"\n    x: f64,\n    y: f64,\n    z: f64,\n")
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
aese(struct:Color, value:"\n\nimpl Color {\n    fn new() -> Self { todo!() }\n}")
AppendEntireStructEnd(struct:Color, value:"\n\nimpl Color {\n    fn new() -> Self { todo!() }\n}")
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
`parent:<name>` to disambiguate when multiple structs define the same
field name.

### Member + Self (`ms`)

The entire field or variant declaration.

```
dems(member:deprecated_field, parent:Config)
DeleteEntireMemberSelf(member:deprecated_field, parent:Config)
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
cemn(member:x, parent:Point, value:"horizontal")
ChangeEntireMemberName(member:x, parent:Point, value:"horizontal")
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
cemv(member:count, parent:Stats, value:" usize")
ChangeEntireMemberValue(member:count, parent:Stats, value:" usize")
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
aeme(member:name, parent:User, value:"\n    email: String,")
AppendEntireMemberEnd(member:name, parent:User, value:"\n    email: String,")
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
dbfs(function:main)
DeleteBeforeFunctionSelf(function:main)
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
dafs(function:first)
DeleteAfterFunctionSelf(function:first)
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
dofp(function:add)
DeleteOutsideFunctionParameters(function:add)
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

## Scope/component validity matrix

A chord with an invalid scope/component combination is rejected at parse time.

|              | Beginning | Contents | End | Value | Parameters | Arguments | Name | Self |
|--------------|:---------:|:--------:|:---:|:-----:|:----------:|:---------:|:----:|:----:|
| **Line**       | Y         | --       | Y   | --    | --         | --        | Y    | Y    |
| **Buffer**     | Y         | --       | Y   | --    | --         | --        | Y    | Y    |
| **Function**   | --        | Y        | Y   | --    | Y          | Y         | Y    | Y    |
| **Variable**   | --        | --       | Y   | Y     | --         | --        | Y    | Y    |
| **Struct**     | --        | Y        | Y   | --    | --         | --        | Y    | Y    |
| **Member**     | --        | --       | Y   | Y     | --         | --        | Y    | Y    |
| **Delimiter**  | Y         | Y        | Y   | --    | --         | --        | Y    | Y    |
