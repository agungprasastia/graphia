# Graphify Compatibility Audit

## Scope

Graphia M0 preserves deterministic structural code-graph behavior. Semantic extraction, LLM processing, document ingestion, MCP, and advanced graph analytics remain outside M0.

## Pipeline

Graphify detects supported files, extracts code structure with Tree-sitter, creates nodes and relationships, merges structural results, then serializes graph JSON. Graphia follows the same useful code path natively: scan, parse, extract, construct, canonicalize, serialize.

## Graph Shape

Graphia uses file and symbol nodes. Supported symbol kinds are `File`, `Module`, `Function`, `Method`, `Class`, `Struct`, `Trait`, and `Interface`. Supported structural relationships are `Contains`, `Imports`, `Calls`, `Inherits`, and `Implements`.

## Identity and Ordering

Node IDs are compact sequential integers assigned after paths and symbols are sorted. Edge IDs are reassigned after canonical edge sorting. This keeps output deterministic for identical input and avoids hash-map iteration ordering in persistent output.

## Language and Parsing

Initial languages are Rust, Python, and TypeScript. Tree-sitter grammar nodes provide source locations, declarations, imports, and syntactic calls. Resolution is intentionally conservative: ambiguous call targets are omitted rather than guessed.

## Serialization and CLI

`graph.json` contains canonical `nodes` and `edges` arrays. `graphia scan`, `graphia build`, and `graphia stats` expose the initial CLI behavior. Writes use temporary files followed by replacement.

## Compatibility Gaps

Graphia does not yet reproduce Graphify's inferred semantic edges, community detection, heterogeneous document ingestion, rich provenance metadata, query/explain/path commands, binary index, or incremental index. Those belong to M0.1-M0.4.