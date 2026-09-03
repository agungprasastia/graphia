# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-09-04

### Added
- **Unified AI Exploration (`graphia explore` & `graphia_explore`)**:
  - 1-call code exploration returning symbol definition, source code slice, container, callers, callees, blast radius, and related tests.
  - Registered as the 12th tool `graphia_explore` in MCP server.
  - CLI command `graphia explore <symbol> [--depth <n>] [--format human|json]`.
- **Zero-Config Agent Setup (`graphia init` & `.graphia/.gitignore`)**:
  - Automatic initialization for `.graphia/`, `.gitignore` rules, and initial code graph index priming.
  - Self-contained internal `.graphia/.gitignore` (`*` and `!.gitignore`) auto-generated on any index write to prevent local graph cache and daemon files from ever showing up in git.
  - Auto-configures MCP server configurations for Claude Code (`.claude/mcp.json`), Cursor (`.cursor/mcp.json`), VS Code (`.vscode/mcp.json`), and Claude Desktop.
  - CLI command `graphia init [--yes]`.
- **Executive Architectural Report (`graphia report` / `GRAPH_REPORT.md`)**:
  - Comprehensive architectural audit covering repository metrics, God nodes & hotspots, circular dependency cycles, community clusters, entrypoints, and AI agent safety guidelines.
  - CLI command `graphia report [--repo <path>] [--output <path>] [--format human|json]`.
- **Interactive Browser Explorer (`graphia ui`)**:
  - Parity with `codegraph ui` serving at `http://127.0.0.1:4747` with zero external runtime dependencies.
  - 3-column layout: inbound callers and references (left), symbol definition and syntax source slice (center), outbound callees and blast radius (right).
  - Interactive HTML5 canvas graph for visual radial neighborhood navigation with zoom, pan, and click-to-explore.
  - Live symbol search with keyboard shortcut (`Ctrl+K` or `/`), and drawer inspector for God nodes & circular cycles.
  - CLI command `graphia ui [--repo <path>] [--port <port>] [--no-open]`.
- **Multi-Format Graph Exporters (`graphia export`)**:
  - Parity with Graphify and CodeGraph visual export formats.
  - **Obsidian Vault (`--format obsidian`)**: Interactive Markdown knowledge vault with YAML frontmatter, `[[wikilinks]]` between symbols and files, and preconfigured `.obsidian/graph.json` color groups.
  - **Mermaid Flowchart (`--format mermaid`)**: Markdown flowchart syntax with subgraphs ready for GitHub READMEs, PRs, and Notion embeds.
  - **Graphviz DOT (`--format dot`)**: Directed graph syntax with shaped nodes, color-coded edges, and cluster subgraphs.
  - **GraphML & GEXF (`--format graphml`, `--format gexf`)**: Standard XML graph formats for Gephi, Cytoscape, and yEd network visualizers.
  - **Cytoscape JSON (`--format cytoscape`)**: Direct elements JSON schema for Cytoscape.js web graphs.
  - Added `--output / -o` flag for custom destination files or vault folders.
- **One-Liner Installers**:
  - Added `install.sh` for instant Linux and macOS installation via `curl -fsSL ... | sh`.
  - Added `install.ps1` for instant Windows PowerShell installation via `irm ... | iex`.
- **Integration Test Suite**:
  - Added `tests/agent_dx.rs` testing explore, init, and report workflows end-to-end.
  - Added `tests/ui_server.rs` verifying embedded HTTP endpoints and lifecycle.
  - Added `tests/export_formats.rs` verifying all graph export formats and CLI subcommands.

## [0.1.0] - 2026-09-04

### Added
- **Multi-Language AST Parsing**: Full Tree-sitter parser integration across 16 languages:
  - Systems: Rust, Go, C, C++, Zig, Swift.
  - Managed: Java, C#, Kotlin.
  - Web & Scripting: TypeScript, JavaScript, TSX, JSX, Python, PHP, Ruby.
- **Deterministic Multi-Stage Resolution**:
  - Scope-aware lexical shadowing and container hierarchy.
  - Import aliasing and multi-hop re-export resolution.
  - Receiver method dispatch without speculative false-positive edges.
- **Selective Incremental Resolution**:
  - In-memory code graph with pending and reverse-resolution indexing.
  - Re-resolves affected consumers on incremental update without ordinary full graph rebuilds.
  - Clean-build equivalence across resolved, ambiguous, and unresolved candidate transitions.
- **Native Graph Analysis Engine**:
  - Strongly Connected Components (Tarjan algorithm).
  - Elementary cycle detection and reporting.
  - PageRank and degree centrality calculation.
  - Afferent and efferent coupling metrics ($C_a$, $C_e$, Instability $I$).
  - Structural hotspot scoring and modularity-based community detection.
- **Repository Intelligence**:
  - Bounded structural neighborhood extraction (`graphia neighborhood`).
  - Blast radius and change surface analysis (`graphia impact`).
  - Language-aware entrypoint detection (`graphia entrypoints`).
  - Deterministic test discovery and source-to-test mapping (`graphia tests`).
  - Structural symbol search and shortest dependency path querying (`graphia path`).
- **AI Context Engine**:
  - Token-, byte-, and character-budgeted AST slicing with distance-decay relevance scoring.
  - Deduplicated context bundle extraction (`graphia context`).
- **Model Context Protocol (MCP) Server**:
  - Stdio JSON-RPC 2.0 server (`graphia mcp`) exposing 11 read-only tools.
  - Strict `stdout` protocol isolation and sandboxed path traversal checks.
  - In-flight request cancellation and bounded worker concurrency.
- **Live Daemon**:
  - Background recursive filesystem watcher via `notify` (`graphia daemon`).
  - Configurable event debouncing and bounded update queue.
  - Periodic persistence to `.graphia/index.bin`.
- **Advanced Static Analysis & Tooling**:
  - Intra-procedural dataflow and typeflow path tracing (`graphia flow`).
  - Architectural boundary rules and drift enforcement (`graphia architecture check`).
  - Git history churn and co-change coupling matrix (`graphia history`, `graphia cochange`).
  - Structural dead code candidate detection (`graphia deadcode`).
  - Graph index diffing and public API surface diffing (`graphia diff`, `graphia api diff`).
- **CLI Commands**:
  - Comprehensive subcommands: `scan`, `build`, `load`, `stats`, `query`, `path`, `update`, `export`, `explain`, `analyze`, `cycles`, `hotspots`, `communities`, `search`, `neighborhood`, `impact`, `tests`, `entrypoints`, `architecture`, `context`, `mcp`, `daemon`, `daemon-status`, `flow`, `architecture-check`, `history`, `cochange`, `deadcode`, `diff`, `api-diff`.
