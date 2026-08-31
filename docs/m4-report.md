# M4 Advanced Static Analysis Report

> Historical M4 scope. All capabilities listed here are closed under M4.1.2 with precision fixes and known limitations documented in [`m4.1.2-final-closure-report.md`](m4.1.2-final-closure-report.md).

## Overview
M4 introduces modular, on-demand advanced static analysis capabilities to the Graphia engine, enabling deep structural reasoning across codebases without heavy runtime overhead.

## Key Capabilities

1. **Refined Call Graph & Dynamic Dispatch (`src/analysis/advanced/callgraph.rs`)**:
   - Trait/interface implementation resolution with confidence scoring (`Extracted`, `Inferred`, `Possible`).
   - Dispatches method invocations through interface bounds and trait implementations.

2. **Intra-Procedural Type Flow & Source-to-Sink Dataflow (`src/analysis/advanced/typeflow.rs`, `dataflow.rs`)**:
   - Intra-procedural assignment tracking and return path discovery.
   - BFS-based path synthesis between arbitrary source and sink symbols with hop-by-hop confidence calculation.

3. **Architecture Boundary Enforcement (`src/analysis/advanced/boundaries.rs`)**:
   - Verifies customizable layer architecture definitions against project dependency edges.
   - Detects architectural drift and forbidden cross-layer imports.

4. **Git History & Temporal Co-Change Coupling (`src/analysis/advanced/history.rs`, `change_coupling.rs`)**:
   - Computes commit churn, active contributors per file, and association rule metrics:
     $$C(A, B) = \frac{\text{commits}(A \cap B)}{\text{commits}(A)}$$
   - Configurable minimum support and confidence thresholds.

5. **Structural Dead Code Detection (`src/analysis/advanced/dead_code.rs`)**:
   - Discovers non-entrypoint, unreferenced symbols across the codebase.

6. **Graph & Public API Diffing (`src/analysis/advanced/diff.rs`)**:
   - Generational graph diffing across index versions (+ / - / ~ nodes and edges).
   - Public API surface comparison highlighting added/removed exported symbols.

7. **CLI Integration**:
   - `graphia flow --source <sym> --sink <sym> [--limit <n>]`
   - `graphia architecture check [--config <path>]`
   - `graphia history [--max-commits <n>]`
   - `graphia cochange [--min-support <f32>]`
   - `graphia deadcode`
   - `graphia diff <old-index> <new-index>`
   - `graphia api diff <old-index> <new-index>`

## Verification Matrix & Benchmark Summary
- Test Suite: All unit and integration tests passing (`cargo test --all-targets --all-features`).
- Quality Gates: Clippy clean with zero warnings (`-D warnings`) and formatted (`cargo fmt --check`).
- Advanced Analysis Latency (`benches/performance.rs`):
  - Flow query: ~42 µs (small), ~230 µs (medium), ~1.9 ms (large).
  - Boundary check: ~37 µs (small), ~122 µs (medium), ~1.79 ms (large).
  - Graph diff: ~24 µs (small), ~123 µs (medium), ~739 µs (large).
  - Change coupling: ~5 µs (small), ~14 µs (medium), ~14 µs (large).
