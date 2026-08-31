# Language Support Matrix

This matrix documents the actual semantic extraction and resolution capabilities across all 16 supported languages in Graphia M4.1.

Status definitions:
- **FULL**: Core definitions, types, functions/methods, imports, calls, visibility, and signatures are extracted from syntax.
- **PARTIAL**: Basic entities, calls, and imports are extracted; some advanced language constructs or type systems are normalized to common kinds.
- **EXPERIMENTAL**: Syntax parsing and baseline entity extraction supported; advanced resolution or macro expansion may be limited.
- **UNSUPPORTED**: Not recognized by parser pipeline.

| Language | Extension(s) | Status | Definitions Extracted | Method/Constructor | Visibility Model | Directives/Imports |
|---|---|---|---|---|---|---|
| **Rust** | `.rs` | FULL | Functions, Structs, Enums, Traits, Modules | Methods, Impls | `pub`, crate/private | `use`, `mod`, `pub use` |
| **Python** | `.py` | FULL | Functions, Classes, Modules | Methods, Constructors (`__init__`) | Underscore convention | `import`, `from ... import` |
| **TypeScript** | `.ts`, `.mts`, `.cts` | FULL | Functions, Classes, Interfaces, Types, Variables | Methods, Constructors | `export`, `public`, `private` | `import`, `require`, `export` |
| **JavaScript** | `.js`, `.mjs`, `.cjs` | FULL | Functions, Classes, Variables | Methods | `export`, default | `import`, `require`, `export` |
| **TSX** | `.tsx` | FULL | Functions, Classes, Interfaces, Components | Methods | `export` | `import`, `export` |
| **JSX** | `.jsx` | FULL | Functions, Classes, Components | Methods | `export` | `import`, `export` |
| **Go** | `.go` | FULL | Packages, Functions, Structs, Interfaces | Receiver Methods | Capitalization rule | `import` statements |
| **C** | `.c`, `.h` | FULL | Functions, Structs, Enums, TypeAliases | Function Declarations | `static` vs public | `#include` |
| **C++** | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hxx`, `.hh`, `.h` | FULL | Namespaces, Classes, Structs, Functions, TypeAliases | Methods, Constructors, Destructors | `public`, `private`, `protected` | `#include` |
| **Java** | `.java` | FULL | Packages, Classes, Interfaces, Structs/Records, Enums | Methods, Constructors | `public`, `protected`, `private`, `package` | `import`, `package` |
| **C#** | `.cs` | FULL | Namespaces, Classes, Structs, Interfaces, Enums, Properties | Methods, Constructors, Destructors | `public`, `private`, `internal`, `protected` | `using`, `namespace` |
| **Kotlin** | `.kt`, `.kts` | FULL | Packages, Classes, Interfaces, Structs/DataClasses, Objects | Functions, Constructors | `public`, `private`, `internal` | `import`, `package` |
| **Zig** | `.zig` | FULL | Functions, Structs, Enums, Constants | Methods (inside structs) | `pub` keyword | `@import(...)` |
| **PHP** | `.php`, `.phtml`, `.php3..7`, `.phps` | FULL | Namespaces, Classes, Interfaces, Traits, Enums, Functions | Methods | `public`, `protected`, `private` | `use`, `namespace` |
| **Ruby** | `.rb`, `.erb` | FULL | Modules, Classes, Functions | Methods, Singleton Methods | Standard public | `require`, `require_relative` |
| **Swift** | `.swift` | FULL | Protocols, Classes, Structs, Enums, Functions | Methods, Initializers (`init`) | `public`, `internal`, `private` | `import` |
