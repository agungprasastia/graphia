# M2 Repository Intelligence Engine Report

## 1. Overview
The M2 Repository Intelligence Engine provides deterministic repository-level insights, semantic context search, change surface calculations, automated test discovery, multi-language entrypoint detection, bounded structural neighborhood queries, and structural architecture summaries.

## 2. Capabilities Implemented

### 2.1 Bounded Structural Neighborhood (`src/intelligence/neighborhood.rs`)
- Extracts complete bounded context for target symbol:
  - Container / Parent Module (`Contains` edges)
  - Children symbols
  - Callers & Callees (configurable depth and limit)
  - Imports & Exports
  - Referenced types (`Struct`, `Class`, `Trait`, `Interface`)
  - Trait & Interface implementations (`Inherits`, `Implements`)
  - Deterministically mapped test cases

### 2.2 Change Surface & Blast Radius Calculation (`src/intelligence/impact.rs`)
- Computes upstream impact graph categorized into:
  - `DirectImpact`: Immediate callers and direct importers.
  - `TransitiveImpact`: Multi-hop callers and importers.
  - `PossibleImpact`: Indirect / structural relations.
- Generates human-readable explanatory traces (`because: X -> calls -> Y`).
- Aggregates affected files and relevant test suites.

### 2.3 Deterministic Test Discovery (`src/intelligence/tests.rs`)
- Multi-strategy test linking:
  - Reference / call graph mapping (`TestFunction` -> `calls` -> `SourceFunction`).
  - Naming conventions (`test_*`, `*_test`, `*.spec.*`, `*.test.*`).
  - Directory structure matching (`tests/`, `test/`).

### 2.4 Language-Aware Entrypoint Detection (`src/intelligence/entrypoints.rs`)
- Identifies program entrypoints across supported languages:
  - Rust / Go / C / C++ `main` functions.
  - JVM / CLR `main` static methods (Java, Kotlin, C#).
  - Python `main` entrypoints & script guards.
  - CLI subcommand / command handlers.

### 2.5 Structural Architecture Overview (`src/intelligence/architecture.rs`)
- Produces an end-to-end repository architecture report:
  - Node, edge, symbol, file, and module counts.
  - Primary dependency flow directions with edge weights.
  - High-centrality architectural modules.
  - Cycle detection & count.
  - Detected modular communities.

### 2.6 Relevance Scoring & Search (`src/intelligence/search.rs`, `src/intelligence/relevance.rs`)
- Multi-signal ranking algorithm combining:
  - Exact qualified match (+100.0)
  - Exact name match (+80.0)
  - Prefix name match (+50.0)
  - Substring name match (+30.0)
  - Path match (+15.0)
  - Centrality boost (PageRank * 10.0)
- Deterministic tie-breaking by score, qualified name, and node ID.

### 2.7 CLI Subcommands & Formats (`src/cli/mod.rs`)
- `graphia search <query> [--kind <kind>] [--file <file>] [--limit <limit>] [--format human|json]`
- `graphia neighborhood <target> [--depth <depth>] [--limit <limit>] [--format human|json]`
- `graphia impact <target> [--depth <depth>] [--files] [--format human|json]`
- `graphia tests [--target <target>] [--format human|json]`
- `graphia entrypoints [--format human|json]`
- `graphia architecture [--format human|json]`

## 3. Verification & Testing
- Integration tests in `tests/repository_intelligence.rs`:
  - `test_search_relevance_ranking_and_filters`
  - `test_bounded_neighborhood_extraction`
  - `test_impact_analysis_and_explanations`
  - `test_deterministic_test_discovery`
  - `test_multi_language_entrypoints_detection`
  - `test_architecture_overview_template`
  - `test_cli_intelligence_subcommands_e2e`
  - `test_negative_cases_and_missing_targets`
- Zero compiler warnings, zero clippy warnings (`cargo clippy --all-targets --all-features -- -D warnings`), clean formatting (`cargo fmt --check`), and all unit and integration tests passing.

## 4. Benchmark Performance Metrics
Benchmarked via `benches/performance.rs` (`cargo bench --bench performance`):

| Dataset | Files | Nodes | Edges | Intel Search (ns) | Intel Neighborhood (ns) | Intel Impact (ns) |
|---|---|---|---|---|---|---|
| small | 3 | 12 | 9 | ~15,000 | ~16,000 | ~11,000 |
| medium | 12 | 48 | 36 | ~53,000 | ~37,000 | ~48,000 |
| large | 48 | 192 | 144 | ~125,000 | ~137,000 | ~150,000 |

