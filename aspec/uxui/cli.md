# CLI Design

Binary name: ane
Install path: /usr/local/bin
Storage location: $HOME/.ane/

## Design principles:

### Command structure
Top level command groups:
- (default): Open the TUI editor (`ane .` or `ane path/to/file`)
- exec: Execute a chord in headless mode (`ane exec --chord "cifb my_func new body" path/to/file`)

### Flag structure
Flag guidance:
- Use long flags with short aliases for common options (e.g. `--chord` / `-c`)
- Flags should be self-documenting via clap's derive macros
- Positional arguments for file/directory paths

### Chord syntax
Two accepted formats:
- Short form: `cifb my_func new body` (4-character chord code + arguments)
- Long form: `ChangeInFunctionBody my_func new body` (PascalCase + arguments)

### Inputs and outputs
I/O Guidance:
- stdin: reserved for future piped input (e.g. reading chord sequences from stdin)
- stdout: in exec mode, unified diff output; in TUI mode, not used (terminal is the output)
- stderr: error messages and status messages (e.g. "no changes")

### Configuration
Global config:
- Future: `$HOME/.ane/config.toml` for editor preferences (theme, keybindings, defaults)
- Not yet implemented in the bootstrap — the config system will be added as features grow
