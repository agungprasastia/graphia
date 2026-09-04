# Hybrid Skill Init Implementation Plan

1. Add `src/cli/skill.rs` with embedded canonical skill files, user/project target resolution, status comparison, and idempotent installation.
   Verify with isolated unit tests for current, missing, stale, and project-scoped installs.
2. Extend CLI with `skill status|install|update` and `init --no-skill --skill-scope`.
   Verify Clap parsing and mutually exclusive arguments.
3. Make `graphia init` select prompt, automatic, skip, or project behavior without blocking non-interactive runs; add OpenCode MCP configuration using its native schema.
   Verify init integration tests with temporary repository and home paths.
4. Update README, command reference, and changelog.
5. Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, full tests, installer syntax checks, and Graphia skill validation.
