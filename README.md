# Graphia

[![Rust Version](https://img.shields.io/badge/rust-1.85%2B%20(2024%20edition)-orange.svg)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build Status](https://img.shields.io/badge/build-passing-brightgreen.svg)]()
[![Warnings](https://img.shields.io/badge/warnings-0%20(strict)-brightgreen.svg)]()
[![Languages](https://img.shields.io/badge/languages-16%20supported-blueviolet.svg)]()

**Graphia** is a high-performance, deterministic, native code graph and repository intelligence engine built in Rust. It extracts, indexes, resolves, and analyzes semantic relationships across multi-language codebases in milliseconds—with zero external runtime dependencies (no Python, no SQLite, no cloud requirements, and no LLMs in the core pipeline).

---

## Key Highlights

- **16 First-Class Languages**: Rust, Python, TypeScript, JavaScript, TSX, JSX, Go, C, C++, Java, C#, Kotlin, Zig, PHP, Ruby, and Swift via dedicated Tree-sitter parsers.
- **Deterministic Multi-Stage Resolution**: Scope-aware lexical shadowing, import aliasing, re-export tracking, and receiver method dispatch without speculative false-positive edges.
- **Native Graph Analysis**: Strongly Connected Components (Tarjan), elementary cycle detection, degree & PageRank centrality, afferent/efferent coupling ($C_a, C_e, I$), structural hotspot scoring, and deterministic community detection.
- **Repository Intelligence**: Bounded symbol neighborhood extraction, blast radius & change surface analysis (`graphia impact`), deterministic test discovery, and language-aware entrypoint detection.
- **AI Context Engine**: AST line-range slicing with distance-decay relevance ranking, exact token/byte budgeting, and deduplicated context bundles designed to eliminate context-window waste for coding agents.
- **Model Context Protocol (MCP) Server**: Built-in JSON-RPC 2.0 stdio transport exposing 11 read-only tools with strict `stdout` protocol isolation and sandbox security.
- **Live Daemon**: Low-overhead recursive filesystem watcher with event debouncing/coalescing, bounded incremental update queues, and generation-tracked snapshot isolation.
- **Advanced Static Analysis**: Refined virtual dispatch candidate sets, intra-procedural dataflow/typeflow paths, architecture layer boundary enforcement, git history co-change metrics, structural dead code candidate detection, and graph/API diffing.

---

## Architecture Overview

```text
                                Source Code (16 Languages)
                                            │
                                            ▼
                                  Recursive Native Scanner
                                            │
                                            ▼
                              Tree-sitter Language Analyzers
                                            │
                                            ▼
                              Normalized AST Extractions
                                            │
                                            ▼
                           Scope / Import / Link Resolution
                                            │
                                            ▼
                             Native In-Memory Code Graph
                                            │
              ┌─────────────────────────────┼─────────────────────────────┐
              ▼                             ▼                             ▼
        Query Engine                 Analysis Engine            Intelligence Engine
       (BFS, Path,                   (SCC, Cycles,               (Impact, Neighborhood,
        Exact Lookup)                 Centrality,                 Test Discovery,
              │                       Communities)                Architecture Overview)
              │                             │                             │
              └─────────────────────────────┼─────────────────────────────┘
                                            ▼
                                     Context Engine
                              (Token-Budgeted AST Slicing)
                                            │
                    ┌───────────────────────┴───────────────────────┐
                    ▼                                               ▼
             CLI Subcommands                                 MCP Stdio Server
      (Human & Versioned JSON)                              (11 Read-Only Tools)
                    │                                               │
                    └───────────────────────┬───────────────────────┘
                                            ▼
                                    Live Daemon Engine
                              (Debounced Incremental Sync)
```

---

## Supported Languages

| Language | Extension(s) | Key Entities Extracted | Directives Handled |
|---|---|---|---|
| **Rust** | `.rs` | Structs, Enums, Traits, Functions, Impls, Modules | `use`, `mod`, `pub use` |
| **Python** | `.py` | Classes, Functions, Methods, Modules | `import`, `from ... import` |
| **TypeScript** | `.ts`, `.mts`, `.cts` | Interfaces, Classes, Functions, Methods, TypeAliases, Variables | `import`, `require`, `export` |
| **JavaScript** | `.js`, `.mjs`, `.cjs` | Classes, Functions, Methods, Variables | `import`, `require`, `export` |
| **TSX** | `.tsx` | React Components, Interfaces, Classes, Functions | `import`, `export` |
| **JSX** | `.jsx` | React Components, Classes, Functions | `import`, `export` |
| **Go** | `.go` | Packages, Structs, Interfaces, Receiver Methods, TypeAliases | `import` declarations |
| **C** | `.c`, `.h` | Structs, Enums, Functions, TypeAliases, Headers | `#include` directives |
| **C++** | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hxx`, `.hh`, `.h` | Namespaces, Classes, Structs, TypeAliases, Methods, Constructors, Destructors | `#include` directives |
| **Java** | `.java` | Packages, Classes, Interfaces, Structs/Records, Enums, Constructors, Methods | `import` statements, `package` |
| **C#** | `.cs` | Namespaces, Classes, Structs, Enums, Interfaces, Properties, Constructors | `using` directives, `namespace` |
| **Kotlin** | `.kt`, `.kts` | Packages, Classes, Objects, Interfaces, Structs/DataClasses, Constructors, Functions | `import`, `package` |
| **Zig** | `.zig` | Structs, Enums, Functions, Constants | `@import(...)` |
| **PHP** | `.php`, `.phtml`, `.php3..7`, `.phps` | Namespaces, Classes, Traits, Interfaces, Enums, Functions, Methods | `use`, `namespace` |
| **Ruby** | `.rb`, `.erb` | Modules, Classes, Methods, Singleton Methods | `require`, `require_relative` |
| **Swift** | `.swift` | Protocols, Classes, Structs, Enums, Initializers (`init`), Functions | `import` statements |

---

## Installation & Requirements

- **Rust Toolchain**: Rust 1.85+ (2024 Edition).
- **Zero Runtime Dependencies**: No Python runtime, SQLite, or background services required.

```bash
# Clone and build
git clone https://github.com/agungprasastia/graphia.git
cd graphia
cargo build --release

# The binary will be available at target/release/graphia
```

---

## CLI Usage Guide

### 1. Repository Scanning & Graph Building

```bash
# Scan supported source files
graphia scan .

# Build canonical graph index (writes graph.json and .graphia/index.bin)
graphia build .

# Perform a clean full rebuild
graphia build . --clean

# Display graph statistics
graphia stats .

# Export binary index to JSON
graphia export . --format json
```

### 2. Graph Traversal & Structural Queries

```bash
# Query exact or partial symbol definition
graphia query . UserService

# Find shortest structural path between two symbols
graphia path . "AuthController::login" "Database::query"

# Explain relationships connected to a symbol
graphia explain . UserService
```

### 3. Structural Graph Analysis

```bash
# General graph analysis overview (Symbol, File, or Module level)
graphia analyze . --level module --format json

# Detect dependency cycles (supports edge filtering)
graphia cycles . --level file --edge imports

# Identify structural coupling hotspots
graphia hotspots . --limit 10

# Detect architectural communities (Label Propagation)
graphia communities . --level module
```

### 4. Repository Intelligence

```bash
# Structural search with multi-signal ranking
graphia search "login" --limit 10

# Extract bounded structural neighborhood
graphia neighborhood UserService --depth 2

# Compute blast radius and change surface
graphia impact UserService --files --explain

# Deterministic test discovery
graphia tests UserService

# Detect application entrypoints
graphia entrypoints

# Generate architectural structural overview
graphia architecture
```

### 5. AI Context Slicing

```bash
# Generate minimal sufficient context bundle for a symbol
graphia context --symbol UserService --token-budget 8000

# Slicing based on query text
graphia context --query "user authentication" --budget-type approx_tokens --format json

# Context for recently changed files
graphia context --changed
```

### 6. Model Context Protocol (MCP) Server

Graphia includes a native, read-only MCP server over standard input/output (`stdio`):

```bash
# Start MCP server (errors if repository is not indexed yet)
graphia mcp --repo .

# Start MCP server with explicit auto-indexing enabled
graphia mcp --repo . --auto-index
```

#### MCP Tool Roster:
1. `graphia_search_symbol`: Multi-signal ranked symbol search.
2. `graphia_get_symbol`: Detailed definition, location, and structural relationships.
3. `graphia_find_callers`: Inbound caller tracing.
4. `graphia_find_callees`: Outbound invocation analysis.
5. `graphia_find_references`: References categorized by calls, types, and imports.
6. `graphia_dependency_path`: Shortest structural dependency path.
7. `graphia_neighborhood`: Structural neighborhood extraction.
8. `graphia_impact`: Blast radius and change surface estimation.
9. `graphia_find_tests`: Associated test discovery.
10. `graphia_architecture`: Structural repository overview.
11. `graphia_context`: Token-budgeted AST-sliced context bundle.

### 7. Live Daemon

Run Graphia as a live synchronization daemon that updates the native graph incrementally in real time:

```bash
# Start foreground synchronization daemon
graphia daemon --repo . --debounce-ms 100 --foreground

# Check daemon health and tracked generation
graphia daemon status --repo . --format json
```

### 8. Advanced Static Analysis

```bash
# Intra-procedural and bounded source-to-sink dataflow paths
graphia flow --source "req.body" --sink "db.execute"

# Architectural boundary validation and drift detection
graphia architecture check --config architecture.toml

# Git history churn and historical contributors
graphia history --max-commits 100

# File co-change matrix and change coupling
graphia cochange --min-support 0.1

# Identify structural dead code candidates
graphia deadcode

# Structural graph diffing between two index states
graphia diff old_index.bin new_index.bin

# Public API surface diffing
graphia api diff old_index.bin new_index.bin
```

---

## Benchmark & Performance

Measurements taken on representative multi-language repository fixtures (AMD Ryzen 9 / Apple Silicon baseline):

| Benchmark Stage | Small (10 files, 40 nodes) | Medium (50 files, 200 nodes) | Large (150 files, 600 nodes) |
|---|---|---|---|
| **Scan Latency** | 2.40 ms | 0.91 ms | 0.93 ms |
| **Parse & Extract (16 Langs)** | 25.16 ms | 28.56 ms | 87.30 ms |
| **Graph Resolution** | 6.19 ms | 17.21 ms | 31.73 ms |
| **Exact Query** | 5.30 µs | 1.54 µs | 1.20 µs |
| **BFS Path Traversal** | 12.64 µs | 10.34 µs | 7.80 µs |
| **Structural Search** | 198.11 µs | 1.02 ms | 3.17 ms |
| **Impact Traversal** | 288.25 µs | 1.20 ms | 3.84 ms |
| **AI Context Generation** | 398.24 µs | 1.35 ms | 3.88 ms |
| **MCP Tool Invocation** | 236.00 µs | 1.11 ms | 3.74 ms |
| **Live Snapshot Publish** | 72.89 µs | 484.30 µs | 1.14 ms |

---

## Quality & Verification Policy

Graphia enforces a strict zero-warning policy:
- `compiler warnings = 0`
- `clippy warnings = 0` (`cargo clippy --all-targets --all-features -- -D warnings`)
- Format compliance: `cargo fmt --check`
- 100% test pass rate across all 21 integration and unit test suites (160+ total automated tests).

```bash
# Run all tests
cargo test --all-targets --all-features

# Run Clippy in strict mode
cargo clippy --all-targets --all-features -- -D warnings

# Run performance benchmarks
cargo bench --bench performance
```

---

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE) for details.
