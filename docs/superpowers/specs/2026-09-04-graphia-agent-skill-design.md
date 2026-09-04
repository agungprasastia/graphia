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

## Hybrid Init Lifecycle

Machine installation owns the user-global skill. Repository initialization owns the index and agent connections. `graphia init` joins both lifecycles without copying a skill into every repository by default:

1. Initialize or update the repository index.
2. Detect supported agent configuration already present in the repository or user environment.
3. Check whether the global Graphia skill is installed and current.
4. If missing or stale, ask before installing it. The prompt defaults to yes; `--yes` accepts it without prompting, `--no-skill` skips it, and a non-interactive run without either flag skips it with a warning instead of blocking.
5. Configure supported MCP integrations and install or update the project Cursor rule while preserving unrelated configuration.
6. Print a concise summary for index, MCP, rules, and skill status.

`graphia skill status`, `graphia skill install`, and `graphia skill update` provide explicit repair and inspection paths. Status is current only when every installed Graphia-owned file matches the embedded canonical content. Skill installation uses content embedded in the Graphia binary so Cargo-installed and standalone binaries do not depend on a nearby release archive.

Project-scoped installation is opt-in through `graphia init --skill-scope project`. It writes the canonical skill to `.agents/skills/graphia`, which is shared by OpenCode, GitHub Copilot, and other Agent Skills consumers. It is mutually exclusive with `--no-skill`. Vendor-specific project adapters may still be generated when their documented format differs. Global scope remains the default to avoid Git noise, stale per-repository copies, and project copies unexpectedly shadowing updates.

## Compatibility and Safety

- Existing Graphia CLI and MCP configuration behavior remains compatible.
- Existing non-interactive use remains deterministic: `--yes` installs a missing or stale global skill, while `--no-skill` performs no skill writes.
- No broad `AGENTS.md`, `CLAUDE.md`, or Copilot instruction file is overwritten.
- No shell tool is pre-approved in skill metadata.
- Paths are quoted and resolved safely on Windows, macOS, and Linux.
- Reinstalling Graphia updates the Graphia skill without duplicating content.

## Verification

- Validate `skills/graphia` with the bundled skill validator.
- Check skill name, description, links, absence of placeholders, and progressive disclosure.
- Test installer copies in isolated temporary directories.
- Test `graphia init` skill detection, prompt-free flags, global default, project opt-in, summary, and idempotent Cursor MDC rule without changing unrelated files.
- Test embedded skill output matches `skills/graphia` and repeated installation stays byte-identical.
- Run PowerShell syntax validation and `bash -n`.
- Run Rust format, Clippy with warnings denied, and all targets tests.
- Add the new skill, supported agents, and automatic installation to `CHANGELOG.md` and user documentation.

## Out of Scope

- Vendor-specific agents without documented skill or rule discovery.
- Automatically granting command execution permissions.
- Installing or configuring the AI applications themselves.
