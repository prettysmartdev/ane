# Operations

## Installing and running
Installation:
- Download pre-built binary from GitHub Releases, or `cargo install ane`
- Place binary in PATH (e.g. /usr/local/bin/ane)

Setup and run:
- No setup required — run `ane` to open the current directory, or `ane <path>` for a specific file/directory
- For agent use: `ane exec --chord "<chord>" <path>`

Environment variables:
- None required for basic operation
- Future: `ANE_CONFIG` to override config file location

Secrets:
- N/A — ane has no network features or credentials

## Ongoing operations

Version upgrades/downgrades:
- Replace the binary with the desired version
- No migration steps needed — ane has no persistent server-side state

Database migrations:
- N/A — ane has no database
