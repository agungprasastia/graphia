# M0 Report

## Architecture

Single Rust crate with focused `scan`, `parser`, `model`, `graph`, `storage`, `cli`, and `error` modules. Tree-sitter parses supported source languages; graph construction uses Vec storage and canonical sorting.

## Implemented

- Recursive deterministic scanner.
- `.git`, `.graphia`, generated, cache, vendor, and dependency directory exclusions.
- Rust, Python, and TypeScript extension detection.
- File, module, function, method, class, struct, trait, and interface extraction where grammar syntax exposes them.
- Syntactic imports and direct calls.
- Deterministic node and edge IDs.
- Atomic canonical JSON output.
- `scan`, `build`, `load`, and `stats` commands.
- Focused unit tests for scanner, parser, graph IDs, and serialization.
- Cross-language fixture and integration coverage for ordering, language detection, typed extracted
  facts, concrete locations/callers, containment/import/call edges, root-independent IDs, and
  byte-identical canonical JSON.

## Compatibility

See `docs/graphify-compatibility.md`. Structural code behavior is retained; semantic and advanced Graphify features are intentionally deferred.

## Known Gaps

M0 is complete. M0.1-M0.4 reports cover stable identity, confidence metadata, adjacency indexes, binary storage, and incremental indexing. Initial graph construction and resolver emit symbol-level `Calls` edges only for same-file or uniquely imported targets; unique unimported targets remain unresolved.

## Tests

M0 tests pass with `cargo test --all-targets --all-features`, including scanner, parser, graph,
storage, CLI, and integration coverage. Strict `cargo check`, `cargo clippy --all-targets
--all-features -- -D warnings`, and `cargo fmt -- --check` also pass.

## Recommended Next Work

Plan post-M0.5/M1 work and address remaining documented gaps, including unresolved unique unimported targets and unavailable RSS/Graphify comparisons.
