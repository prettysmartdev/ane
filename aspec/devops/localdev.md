# Local Development

Development: docker
Build tools: cargo

## Workflows:

Developer Loop:
- Use `Dockerfile.dev` to build a development container with the full Rust toolchain
- `docker build -f Dockerfile.dev -t ane-dev .`
- `docker run -it -v $(pwd):/workspace ane-dev`
- Inside the container: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt --check`

Local testing:
- `cargo test` runs all unit and integration tests
- `cargo test --lib` for unit tests only
- `cargo clippy -- -D warnings` for lint checks
- `cargo fmt --check` to verify formatting

Version control:
- Git-based workflow on GitHub
- Feature branches merged via pull request
- All PRs must pass CI (build, test, clippy, fmt)

Documentation:
- Inline rustdoc comments for public APIs only when the behavior is non-obvious
- aspec/ directory contains all project specifications
- README.md for user-facing documentation
