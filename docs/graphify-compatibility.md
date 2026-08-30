# Graphify Compatibility Audit

## Scope

Graphia M0 preserves deterministic structural code-graph behavior. Semantic extraction, LLM processing, document ingestion, MCP, and advanced graph analytics remain outside M0.

## Pipeline

Graphify detects supported files, extracts code structure with Tree-sitter, creates nodes and relationships, merges structural results, then serializes graph JSON. Graphia follows the same useful code path natively: scan, parse, extract, construct, canonicalize, serialize.

## Graph Shape

Graphia uses file and symbol nodes. Supported symbol kinds are `File`, `Module`, `Function`, `Method`, `Class`, `Struct`, `Trait`, and `Interface`. Supported structural relationships are `Contains`, `Imports`, `Calls`, `Inherits`, and `Implements`.

## Identity and Ordering

Node and edge IDs are fixed-width values derived from SHA-256 identity seeds, with deterministic collision resolution. Canonical sorting keeps output deterministic for identical input and avoids hash-map iteration ordering in persistent output.

## Language and Parsing

Initial languages are Rust, Python, and TypeScript. Tree-sitter grammar nodes provide source locations, declarations, imports, and syntactic calls. Resolution is intentionally conservative: ambiguous call targets are omitted rather than guessed.

## Serialization and CLI

`graph.json` contains canonical `nodes` and `edges` arrays. `graphia scan`, `graphia build`, and `graphia stats` expose the initial CLI behavior. Writes use temporary files followed by replacement.

## M0 Baseline Coverage

`tests/fixtures/` contains minimal Rust, Python, and TypeScript sources plus imported helper modules.
`tests/foundation.rs` locks scanner ordering and language detection, parser symbols/imports/calls
and source locations, graph file/containment/import/call edges and repeated-build IDs, and
byte-identical canonical JSON.
The fixture tree uses relative paths so checks remain independent of checkout location.

## Compatibility Gaps

M0 is complete. M0.1-M0.4 milestone acceptance covers inferred semantic edges, community detection, heterogeneous document ingestion, rich provenance metadata, query/explain/path commands, binary index, and incremental index; those capabilities are outside this M0 compatibility claim, not declared absent from the codebase.
