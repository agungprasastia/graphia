# Audit Bugfixes Design

## Goal

Remove all confirmed defects from the 2026-09-04 audit without changing public commands or valid persisted data. Every fix must have a focused regression test and preserve existing behavior for valid inputs.

## Transport and Server Safety

- Replace recursive blank-line handling in MCP stdio with iteration.
- Reject an MCP line once it exceeds a documented finite byte limit. Return a parse/protocol error instead of exhausting memory.
- Track concurrent MCP responses by request occurrence, not only `RequestId`, so duplicate IDs cannot overwrite completion state or block EOF shutdown.
- Bound UI connection concurrency. Excess connections receive a service-unavailable response or are closed; accepted connections keep the current timeout behavior.

## Configuration and Filesystem Integrity

- `graphia init` must preserve an existing valid object configuration and add only Graphia's MCP entry.
- Invalid JSON or a non-object configuration must return an error and leave the original file unchanged.
- Source extraction with a repository root must reject absolute paths and paths whose canonical target escapes the canonical root. Missing in-root files retain the current empty-slice behavior.

## Graph Analysis Correctness

- Symbol projection IDs must preserve source-node identity when qualified names collide. Human-readable names remain unchanged and projected edges target the correct symbol.
- Community propagation must converge deterministically rather than oscillate; connected two-node input must form one stable community independent of iteration parity.
- Weighted PageRank must divide contributions by total outgoing weight and multiply each contribution by its edge weight.
- Incremental summaries must report actual added and removed component nodes and edges, not combine component sizes with whole-graph deltas.
- Source slicing must apply one-based line/column spans safely, including UTF-8 text and single-line symbols.

## Encoding, Storage, and History

- URL query decoding must decode percent escapes as bytes and then validate UTF-8. `+` remains a space. Malformed escapes remain non-panicking and deterministic.
- Binary index loading must reject unknown visibility discriminants as corrupt input.
- Git numstat parsing must preserve filenames exactly, including repeated spaces and tabs supported by Git output. Rename syntax continues to use Git's emitted path representation unless explicitly normalized elsewhere.

## Compatibility

- No CLI command, option, JSON response field, or public Rust function is removed.
- Existing valid indexes remain readable.
- Invalid inputs that were silently accepted may now return errors.
- When collision-free, projected symbol IDs remain the existing qualified names. Collision disambiguation is applied only when required.

## Verification

- Add regression tests for every defect above.
- Run `cargo fmt --all -- --check`.
- Run `cargo clippy --all-targets --all-features -- -D warnings`.
- Run `cargo test --all-targets`.
- Review the complete diff for regressions and unrelated changes.
- Add concise entries under `Unreleased` in `CHANGELOG.md`.

## Out of Scope

- New user-facing features.
- Broad architectural refactoring.
- Dependency upgrades unrelated to these fixes.
- Claims that testing proves absence of every possible future bug; completion means all audited defects are fixed and the full current verification suite passes.
