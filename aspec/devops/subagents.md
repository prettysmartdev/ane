# Subagents for local development

## Subagent 1:
- name: code-review
- description: Reviews pull requests for adherence to the layered architecture, Rust idioms, and aspec conventions
Settings:
- model: claude
- tools: Read, Bash (git, cargo)
- permissions: read-only access to workspace

## Subagent 2:
- name: test-runner
- description: Runs the test suite and reports failures with context
Settings:
- model: claude
- tools: Bash (cargo test, cargo clippy)
- permissions: read-only access to workspace, execute cargo commands
