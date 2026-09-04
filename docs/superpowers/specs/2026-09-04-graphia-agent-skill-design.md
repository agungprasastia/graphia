# Graphia Agent Skill Design

## Goal

Ship one portable Graphia skill that teaches compatible coding agents to query a repository's code graph accurately and with minimal context usage. Install it automatically with Graphia while preserving existing user configuration and supporting explicit manual installation.

## Canonical Skill

Store the maintained source at `skills/graphia/SKILL.md`. It uses portable Agent Skills frontmatter and contains only routing, invariants, and a token-efficient workflow. Detailed command and MCP mappings live in `skills/graphia/references/commands.md` and are loaded only when needed.

The skill activates for repository structure, symbol lookup, caller/callee tracing, impact analysis, architecture, code-flow, test discovery, and context gathering when Graphia is available. It must not activate for unrelated coding tasks.

## Token-Efficient Workflow

- Detect `.graphia` and Graphia availability before use.
- Prefer one `graphia explore <symbol> --format json` call for definition, source, relationships, impact, and related tests.
- Use the narrowest specialized command only when `explore` cannot answer the request.
- Prefer JSON and bounded limits for machine consumption; avoid dumping complete indexes or whole repositories.
- Use `graphia update` after ordinary edits. Use `graphia build --clean` only for missing, corrupt, or explicitly rebuilt indexes.
- Query before reading source files; read only exact files or spans still needed after Graphia output.
- Treat Graphia results as navigation evidence, then verify code before mutation.
- Do not infer permission to edit code, start daemons, or alter external configuration.

## Supported Agents and Locations

Install the same canonical skill content into supported global locations:

- Codex: `~/.codex/skills/graphia`
- Claude Code and compatible consumers: `~/.claude/skills/graphia`
- Shared Agent Skills consumers: `~/.agents/skills/graphia`
- GitHub Copilot CLI: `~/.copilot/skills/graphia`
- OpenCode: `~/.config/opencode/skills/graphia`

OpenCode also discovers `.claude/skills` and `.agents/skills`; duplicate installation must remain byte-identical and harmless. Cursor receives a generated project rule at `.cursor/rules/graphia.mdc` because Cursor uses its own MDC rule format.

## Installation

- Release archives include the `skills/graphia` directory.
- `install.ps1` and `install.sh` install the binary first, then copy the skill to supported global locations.
- Skill installation is idempotent and replaces only Graphia-owned skill directories/files.
- Failure to install one optional agent adapter emits a warning but does not remove a successfully installed binary.
- Installers support an environment override for a temporary destination root so installation behavior can be tested without touching a real home directory.
- `graphia init` installs or updates the project Cursor rule while keeping unrelated rules intact.

## Compatibility and Safety

- Existing Graphia CLI and MCP configuration behavior remains compatible.
- No broad `AGENTS.md`, `CLAUDE.md`, or Copilot instruction file is overwritten.
- No shell tool is pre-approved in skill metadata.
- Paths are quoted and resolved safely on Windows, macOS, and Linux.
- Reinstalling Graphia updates the Graphia skill without duplicating content.

## Verification

- Validate `skills/graphia` with the bundled skill validator.
- Check skill name, description, links, absence of placeholders, and progressive disclosure.
- Test installer copies in isolated temporary directories.
- Test `graphia init` creates an idempotent Cursor MDC rule without changing unrelated files.
- Run PowerShell syntax validation and `bash -n`.
- Run Rust format, Clippy with warnings denied, and all targets tests.
- Add the new skill, supported agents, and automatic installation to `CHANGELOG.md` and user documentation.

## Out of Scope

- Vendor-specific agents without documented skill or rule discovery.
- Automatically granting command execution permissions.
- Installing or configuring the AI applications themselves.
