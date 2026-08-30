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
- `scan`, `build`, and `stats` commands.
- Focused unit tests for scanner, parser, graph IDs, and serialization.
- Cross-language fixture and integration coverage for ordering, language detection, typed extracted
  facts, concrete locations/callers, containment/import/call edges, root-independent IDs, and
  byte-identical canonical JSON.

## Compatibility

See `docs/graphify-compatibility.md`. Structural code behavior is retained; semantic and advanced Graphify features are intentionally deferred.

## Known Gaps

M0 is complete. M0.1-M0.4 milestone acceptance covers stable identity across graph changes, confidence metadata, adjacency indexes, binary storage, incremental indexing, and benchmark hardening; those capabilities are not accepted in this M0 report. M0 uses conservative helper-file resolution for direct fixture imports and calls.

## Tests

M0 tests pass with `cargo test --all-targets --all-features`, including scanner, parser, graph,
storage, CLI, and integration coverage. Strict `cargo check`, `cargo clippy --all-targets
--all-features -- -D warnings`, and `cargo fmt -- --check` also pass.

## Recommended Next Work

M0.1: introduce explicit identity and confidence semantics, graph invariants, and conservative cross-file resolution.
