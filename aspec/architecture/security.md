# Security

## API Security

Transport:
- ane is primarily a local-only tool with no network listeners
- LSP integration communicates with language servers via local stdin/stdout pipes (no network sockets)
- LSP server installation may invoke package managers (e.g., `rustup component add rust-analyzer`) which access the network

Authentication:
- N/A — relies on OS-level filesystem permissions

RBAC:
- ane operates with the permissions of the invoking user
- No elevation of privileges
- File operations are bounded to paths explicitly provided by the user
- The exec mode only modifies files explicitly targeted by the chord command
- No arbitrary code execution — chords are a fixed set of predefined operations
- LSP servers are started as child processes with the same user permissions
- Language server installation uses well-known package managers only (e.g., rustup)
