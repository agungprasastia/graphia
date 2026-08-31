use graphia::graph::build_graph;
use graphia::model::{EdgeKind, Language, NodeKind, SourceLocation};
use graphia::parser::{Call, Import, ParsedFile, Symbol};
use graphia::resolve::{ScopeKind, ScopeTree, parse_import_directive};

fn loc(file: &str, line: u32) -> SourceLocation {
    SourceLocation {
        file: file.to_string(),
        start_line: line,
        start_col: 1,
        end_line: line,
        end_col: 10,
    }
}

fn func(file: &str, name: &str, line: u32) -> Symbol {
    Symbol {
        kind: NodeKind::Function,
        name: name.to_string(),
        qualified_name: format!("{file}::{name}"),
        location: loc(file, line),
        parent: None,
    }
}

fn method(file: &str, class_name: &str, name: &str, line: u32) -> Symbol {
    Symbol {
        kind: NodeKind::Method,
        name: name.to_string(),
        qualified_name: format!("{file}::{class_name}::{name}"),
        location: loc(file, line),
        parent: Some(class_name.to_string()),
    }
}

#[test]
fn test_scope_hierarchy_and_lexical_shadowing() {
    let mut tree = ScopeTree::new();
    let root = 0;
    let file_scope = tree.add_scope(
        Some(root),
        ScopeKind::File,
        "main.rs",
        "main.rs",
        Some(Language::Rust),
        None,
    );
    let class_scope = tree.add_scope(
        Some(file_scope),
        ScopeKind::Class,
        "User",
        "main.rs",
        Some(Language::Rust),
        None,
    );
    let func_scope = tree.add_scope(
        Some(class_scope),
        ScopeKind::Method,
        "login",
        "main.rs",
        Some(Language::Rust),
        None,
    );

    let outer_id = graphia::model::NodeId(10);
    let inner_id = graphia::model::NodeId(20);

    tree.define_symbol(file_scope, "foo", outer_id);
    tree.define_symbol(func_scope, "foo", inner_id);

    // Lexical lookup from inside func_scope should find inner_id (shadowing)
    let (found_scope, ids) = tree.lookup_lexical(func_scope, "foo").expect("lookup");
    assert_eq!(found_scope, func_scope);
    assert_eq!(ids, &[inner_id]);

    // Lexical lookup from file_scope should find outer_id
    let (found_scope, ids) = tree.lookup_lexical(file_scope, "foo").expect("lookup");
    assert_eq!(found_scope, file_scope);
    assert_eq!(ids, &[outer_id]);
}

#[test]
fn test_import_parsing_all_languages() {
    // Rust
    let r_imp = Import {
        path: "use std::collections::{HashMap, BTreeMap as BTree}".to_string(),
        location: loc("a.rs", 1),
    };
    let r_dirs = parse_import_directive(&r_imp, Some(Language::Rust));
    assert_eq!(r_dirs.len(), 2);
    assert_eq!(r_dirs[0].imported_symbol.as_deref(), Some("HashMap"));
    assert_eq!(r_dirs[1].alias.as_deref(), Some("BTree"));

    // Python
    let py_imp = Import {
        path: "from os.path import join as path_join, exists".to_string(),
        location: loc("a.py", 1),
    };
    let py_dirs = parse_import_directive(&py_imp, Some(Language::Python));
    assert_eq!(py_dirs.len(), 2);
    assert_eq!(py_dirs[0].alias.as_deref(), Some("path_join"));
    assert_eq!(py_dirs[1].imported_symbol.as_deref(), Some("exists"));

    // JS/TS
    let ts_imp = Import {
        path: "import { Component as Comp, useState } from 'react'".to_string(),
        location: loc("a.ts", 1),
    };
    let ts_dirs = parse_import_directive(&ts_imp, Some(Language::TypeScript));
    assert_eq!(ts_dirs.len(), 2);
    assert_eq!(ts_dirs[0].alias.as_deref(), Some("Comp"));

    // CommonJS
    let cjs_imp = Import {
        path: "const fs = require('fs')".to_string(),
        location: loc("a.js", 1),
    };
    let cjs_dirs = parse_import_directive(&cjs_imp, Some(Language::JavaScript));
    assert_eq!(cjs_dirs[0].target_module_or_path, "fs");
    assert_eq!(cjs_dirs[0].alias.as_deref(), Some("fs"));

    // Go
    let go_imp = Import {
        path: "f \"fmt\"".to_string(),
        location: loc("a.go", 1),
    };
    let go_dirs = parse_import_directive(&go_imp, Some(Language::Go));
    assert_eq!(go_dirs[0].target_module_or_path, "fmt");
    assert_eq!(go_dirs[0].alias.as_deref(), Some("f"));

    // C#
    let cs_imp = Import {
        path: "using Project = MyNamespace.Project;".to_string(),
        location: loc("a.cs", 1),
    };
    let cs_dirs = parse_import_directive(&cs_imp, Some(Language::CSharp));
    assert_eq!(cs_dirs[0].alias.as_deref(), Some("Project"));

    // PHP
    let php_imp = Import {
        path: "use Foo\\Bar as Baz;".to_string(),
        location: loc("a.php", 1),
    };
    let php_dirs = parse_import_directive(&php_imp, Some(Language::Php));
    assert_eq!(php_dirs[0].alias.as_deref(), Some("Baz"));
}

#[test]
fn test_explicit_alias_resolution() {
    let caller = ParsedFile {
        symbols: vec![func("app.py", "main", 1)],
        imports: vec![Import {
            path: "from helper import original_func as renamed_func".to_string(),
            location: loc("app.py", 1),
        }],
        calls: vec![Call {
            caller: "app.py::main".to_string(),
            callee: "renamed_func".to_string(),
            location: loc("app.py", 2),
        }],
    };
    let helper = ParsedFile {
        symbols: vec![func("helper.py", "original_func", 1)],
        imports: vec![],
        calls: vec![],
    };

    let mut graph = build_graph(vec![
        ("app.py".to_string(), Some(Language::Python), caller),
        ("helper.py".to_string(), Some(Language::Python), helper),
    ]);

    let report = graph.resolve_cross_file().expect("resolve");
    assert_eq!(report.resolved_calls, 1);

    let caller_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "app.py::main")
        .unwrap()
        .id;
    let target_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "helper.py::original_func")
        .unwrap()
        .id;

    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::Calls && e.from == caller_id && e.to == target_id)
    );
}

#[test]
fn test_receiver_aware_method_resolution() {
    let service = ParsedFile {
        symbols: vec![
            Symbol {
                kind: NodeKind::Class,
                name: "UserService".to_string(),
                qualified_name: "service.ts::UserService".to_string(),
                location: loc("service.ts", 1),
                parent: None,
            },
            method("service.ts", "UserService", "authenticate", 2),
        ],
        imports: vec![],
        calls: vec![],
    };

    let controller = ParsedFile {
        symbols: vec![func("controller.ts", "handleLogin", 1)],
        imports: vec![Import {
            path: "import { UserService } from './service'".to_string(),
            location: loc("controller.ts", 1),
        }],
        calls: vec![Call {
            caller: "controller.ts::handleLogin".to_string(),
            callee: "authenticate".to_string(),
            location: loc("controller.ts", 2),
        }],
    };

    let mut graph = build_graph(vec![
        (
            "service.ts".to_string(),
            Some(Language::TypeScript),
            service,
        ),
        (
            "controller.ts".to_string(),
            Some(Language::TypeScript),
            controller,
        ),
    ]);

    let report = graph.resolve_cross_file().expect("resolve");
    assert_eq!(report.resolved_calls, 1);

    let caller_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "controller.ts::handleLogin")
        .unwrap()
        .id;
    let target_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "service.ts::UserService::authenticate")
        .unwrap()
        .id;

    assert!(
        graph
            .edges
            .iter()
            .any(|e| e.kind == EdgeKind::Calls && e.from == caller_id && e.to == target_id)
    );
}

#[test]
fn test_ambiguity_preservation_and_negative_test() {
    // Two imported files with identical symbol name and no aliases -> ambiguous, no false positive edge
    let caller = ParsedFile {
        symbols: vec![func("main.rs", "run", 1)],
        imports: vec![
            Import {
                path: "use mod_a::action;".to_string(),
                location: loc("main.rs", 1),
            },
            Import {
                path: "use mod_b::action;".to_string(),
                location: loc("main.rs", 2),
            },
        ],
        calls: vec![Call {
            caller: "main.rs::run".to_string(),
            callee: "action".to_string(),
            location: loc("main.rs", 3),
        }],
    };
    let mod_a = ParsedFile {
        symbols: vec![func("mod_a.rs", "action", 1)],
        imports: vec![],
        calls: vec![],
    };
    let mod_b = ParsedFile {
        symbols: vec![func("mod_b.rs", "action", 1)],
        imports: vec![],
        calls: vec![],
    };

    let mut graph = build_graph(vec![
        ("main.rs".to_string(), Some(Language::Rust), caller),
        ("mod_a.rs".to_string(), Some(Language::Rust), mod_a),
        ("mod_b.rs".to_string(), Some(Language::Rust), mod_b),
    ]);

    let report = graph.resolve_cross_file().expect("resolve");
    assert_eq!(report.ambiguous_calls, 1);
    assert_eq!(report.resolved_calls, 0);

    // Negative verification: no spurious Calls edge generated!
    assert!(graph.edges.iter().all(|e| e.kind != EdgeKind::Calls));
    graph.validate().expect("graph valid");
}

#[test]
fn test_unimported_foreign_symbol_remains_unresolved() {
    let caller = ParsedFile {
        symbols: vec![func("caller.py", "main", 1)],
        imports: vec![], // No imports!
        calls: vec![Call {
            caller: "caller.py::main".to_string(),
            callee: "foreign_helper".to_string(),
            location: loc("caller.py", 2),
        }],
    };
    let other = ParsedFile {
        symbols: vec![func("other.py", "foreign_helper", 1)],
        imports: vec![],
        calls: vec![],
    };

    let mut graph = build_graph(vec![
        ("caller.py".to_string(), Some(Language::Python), caller),
        ("other.py".to_string(), Some(Language::Python), other),
    ]);

    let report = graph.resolve_cross_file().expect("resolve");
    assert_eq!(report.unresolved_calls, 1);
    assert_eq!(report.resolved_calls, 0);
    assert!(graph.edges.iter().all(|e| e.kind != EdgeKind::Calls));
}
