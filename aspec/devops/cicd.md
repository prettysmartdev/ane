# Continuous Integration and Deployment

Platform: github

## Pipelines:

Build:
- Build on Linux x86_64 and aarch64
- Build on macOS x86_64 and aarch64
- Use `cargo build --release` with LTO and strip enabled

Test:
- `cargo test` on all supported platforms
- `cargo clippy -- -D warnings`
- `cargo fmt --check`

Releases:
- Tag-based releases (v0.1.0, v0.2.0, etc.)
- GitHub Actions workflow triggers on version tags

Versioning:
- Semantic versioning (semver)
- Version tracked in Cargo.toml

Publishing:
- GitHub Releases with pre-built static binaries for all target platforms
- Future: publish to crates.io

Deployment:
- N/A — ane is a client-side binary, not a deployed service
