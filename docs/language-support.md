# Language Support Matrix

This matrix documents actual semantic extraction, AST-aware typeflow, and resolution capabilities across Graphia's 16 supported languages.

## Status definitions

- **FULL**: Core definitions, types, functions/methods, imports, calls, visibility, and signatures are extracted from syntax.
- **AST TYPEFLOW**: Tree-sitter visitors extract parameters, bindings, assignments, call arguments, and returns for approximate intra-procedural flow.
- **FALLBACK**: Normalized parser extraction and textual/conservative relationship support exist, but AST-aware typeflow is not claimed for that language.
- **EXPERIMENTAL**: Syntax parsing and baseline entity extraction supported; advanced resolution or macro expansion may be limited.

| Language | Extension(s) | Extraction | AST-aware typeflow | Resolution / fallback |
|---|---|---|---|---|
| **Rust** | `.rs` | FULL | AST TYPEFLOW | Scope, imports, re-exports, overload-aware resolution |
| **Python** | `.py` | FULL | AST TYPEFLOW | Scope, imports, calls, approximate typing |
| **TypeScript** | `.ts`, `.mts`, `.cts` | FULL | AST TYPEFLOW | Imports, exports, aliases, overload-aware resolution |
| **JavaScript** | `.js`, `.mjs`, `.cjs` | FULL | AST TYPEFLOW | Imports, exports, aliases, conservative dynamic dispatch |
| **TSX** | `.tsx` | FULL | AST TYPEFLOW via TypeScript syntax nodes | React/component extraction; approximate flow |
| **JSX** | `.jsx` | FULL | AST TYPEFLOW via JavaScript syntax nodes | Component extraction; approximate flow |
| **Go** | `.go` | FULL | FALLBACK | Normalized AST extraction and structural resolution |
| **C** | `.c`, `.h` | FULL | FALLBACK | Normalized extraction; conservative structural links |
| **C++** | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hxx`, `.hh`, `.h` | FULL | FALLBACK | Header/content disambiguation and normalized resolution |
| **Java** | `.java` | FULL | FALLBACK | Normalized extraction and structural resolution |
| **C#** | `.cs` | FULL | FALLBACK | Normalized extraction and structural resolution |
| **Kotlin** | `.kt`, `.kts` | FULL | FALLBACK | Normalized extraction and structural resolution |
| **Zig** | `.zig` | FULL | FALLBACK | Normalized extraction and structural resolution |
| **PHP** | `.php`, `.phtml`, `.php3..7`, `.phps` | FULL | FALLBACK | Normalized extraction and structural resolution |
| **Ruby** | `.rb`, `.erb` | FULL | FALLBACK | Normalized extraction and structural resolution |
| **Swift** | `.swift` | FULL | FALLBACK | Normalized extraction and structural resolution |

AST-aware typeflow is intentionally approximate. It preserves `Known`, `Partial`, and `Unknown` uncertainty rather than presenting heuristic paths as compiler-verified facts. Dataflow is separate from structural BFS and does not treat `Imports` or `Contains` edges as value flow. All languages retain baseline normalized extraction; fallback does not mean unsupported.
> **Data-flow coverage:** AST-aware value flow is runtime-backed for Rust, TypeScript/JavaScript, and Python when source root is available; other languages remain partial/unsupported for value-flow claims.
