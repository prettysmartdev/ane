# Infrastructure

Deployment platform: N/A (client-side binary)
Cloud platform: N/A
Automation: github-actions

## Architecture:

Best practices:
- CI/CD via GitHub Actions for build, test, and release automation
- Cross-compilation targets for Linux and macOS (x86_64 and aarch64)
- Static linking for zero-dependency binaries

Security and RBAC:
- GitHub Actions uses minimal permissions (contents: read, packages: write for releases)
- No cloud infrastructure to secure — ane is entirely client-side
