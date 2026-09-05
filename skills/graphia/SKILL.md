---
name: graphia
description: Use Graphia to navigate and analyze a codebase with minimal token usage. Activate when locating symbols, tracing callers or dependencies, estimating change impact, finding relevant tests, understanding architecture, or gathering bounded source context in a repository where Graphia is installed or `.graphia` exists. Do not use for unrelated prose or non-code tasks.
license: MIT
metadata:
  author: graphia
  purpose: token-efficient-code-navigation
  compatibility: codex-claude-copilot-opencode-agent-skills
---

# Graphia

Use Graphia as a code index, not as a substitute for verifying code you will change.

## Start

1. Check whether `.graphia/index.bin` exists and `graphia --version` works.
2. If index exists, query it immediately. Do not scan or read repository broadly first.
3. If index is missing, run `graphia build .` only when building an index is within task scope.
4. After ordinary source edits, run `graphia update .`. Reserve `graphia build . --clean` for corrupt or explicitly rebuilt indexes.

## Default Query

For a named symbol, start with one bounded call:

```bash
graphia explore <symbol> --depth 2 --format json
```

This usually replaces separate definition, source, callers, callees, impact, and test searches. Read source files only when Graphia output leaves a concrete question unanswered or before editing exact code.

When Graphia MCP tools are available, prefer `graphia_explore` with equivalent arguments. MCP avoids shell formatting noise and returns structured data directly.

## Choose Narrowly

- Unknown exact symbol: `graphia search . <query> --limit 10 --format json`, then explore one result.
- Callers/callees and nearby structure: `graphia neighborhood . <symbol> --depth 2 --limit 20 --format json`.
- Change risk: `graphia impact . <symbol> --depth 3 --files --format json`.
- Tests to run: `graphia tests . --target <symbol> --format json`.
- Minimal source bundle: `graphia context . --symbol <symbol> --token-budget <budget> --format json`.
- Repository shape: `graphia architecture . --format json`.
- Dependency route: `graphia path . <from> <to>`.

Use the smallest depth, limit, and token budget that answers the request. Never dump `graph.json`, `.graphia/index.bin`, or an unbounded repository listing into model context.

## Reliability

- Treat ambiguous search results as candidates. Select by qualified name and file before acting.
- Verify exact source and current working tree before editing.
- If results look stale after edits, run `graphia update .` and repeat the same query.
- If a command option is uncertain, run `graphia <command> --help`; do not guess flags.
- Do not start `graphia daemon`, launch `graphia ui`, rebuild indexes, or modify code unless allowed by the user's task.
- Keep Graphia stdout machine-readable when consuming it programmatically; use `--format json` where supported.

For less-common commands and MCP mappings, read [references/commands.md](references/commands.md) only as needed.
