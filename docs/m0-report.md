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

## Compatibility

See `docs/graphify-compatibility.md`. Structural code behavior is retained; semantic and advanced Graphify features are intentionally deferred.

## Known Gaps

Stable identity across graph changes, confidence metadata, cross-file symbol resolution, adjacency indexes, binary storage, incremental indexing, and benchmark hardening remain for later milestones.

## Tests

`cargo test --lib` passes after aligning `tree-sitter-rust` with the Tree-sitter runtime ABI.

## Recommended Next Work

M0.1: introduce explicit identity and confidence semantics, graph invariants, and conservative cross-file resolution.