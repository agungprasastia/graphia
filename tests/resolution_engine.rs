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
        visibility: graphia::model::Visibility::Public,
        signature: None,
        container: None,
    }
}

fn method(file: &str, class_name: &str, name: &str, line: u32) -> Symbol {
    Symbol {
        kind: NodeKind::Method,
        name: name.to_string(),
        qualified_name: format!("{file}::{class_name}::{name}"),
        location: loc(file, line),
        parent: Some(class_name.to_string()),
        visibility: graphia::model::Visibility::Public,
        signature: None,
        container: Some(class_name.to_string()),
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
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        instantiations: vec![],
        inheritances: vec![],
        implementations: vec![],
    };
    let helper = ParsedFile {
        symbols: vec![func("helper.py", "original_func", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
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
                visibility: graphia::model::Visibility::Public,
                signature: None,
                container: None,
            },
            method("service.ts", "UserService", "authenticate", 2),
        ],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
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
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
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
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let mod_a = ParsedFile {
        symbols: vec![func("mod_a.rs", "action", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let mod_b = ParsedFile {
        symbols: vec![func("mod_b.rs", "action", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
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
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let other = ParsedFile {
        symbols: vec![func("other.py", "foreign_helper", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
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

fn class_sym(file: &str, name: &str, line: u32) -> Symbol {
    Symbol {
        kind: NodeKind::Class,
        name: name.to_string(),
        qualified_name: format!("{file}::{name}"),
        location: loc(file, line),
        parent: None,
        visibility: graphia::model::Visibility::Public,
        signature: None,
        container: None,
    }
}

fn trait_sym(file: &str, name: &str, line: u32) -> Symbol {
    Symbol {
        kind: NodeKind::Trait,
        name: name.to_string(),
        qualified_name: format!("{file}::{name}"),
        location: loc(file, line),
        parent: None,
        visibility: graphia::model::Visibility::Public,
        signature: None,
        container: None,
    }
}

fn func_with_sig(file: &str, name: &str, sig: &str, line: u32) -> Symbol {
    Symbol {
        kind: NodeKind::Function,
        name: name.to_string(),
        qualified_name: format!("{file}::{name}"),
        location: loc(file, line),
        parent: None,
        visibility: graphia::model::Visibility::Public,
        signature: Some(sig.to_string()),
        container: None,
    }
}

#[test]
fn test_engine_same_name_different_modules() {
    let mod_a = ParsedFile {
        symbols: vec![func("mod_a.rs", "compute", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let mod_b = ParsedFile {
        symbols: vec![func("mod_b.rs", "compute", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let app = ParsedFile {
        symbols: vec![func("app.rs", "main", 1)],
        imports: vec![Import {
            path: "use mod_a::compute;".to_string(),
            location: loc("app.rs", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("mod_a.rs".to_string(), Some(Language::Rust), mod_a),
        ("mod_b.rs".to_string(), Some(Language::Rust), mod_b),
        ("app.rs".to_string(), Some(Language::Rust), app),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let res = engine.resolve_reference("app.rs", None, "compute", EdgeKind::Calls, None);
    let target_a = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "mod_a.rs::compute")
        .unwrap()
        .id;
    assert_eq!(res, graphia::resolve::Resolution::Resolved(target_a));
}

#[test]
fn test_engine_same_name_different_containers() {
    let service = ParsedFile {
        symbols: vec![
            class_sym("service.ts", "Alpha", 1),
            method("service.ts", "Alpha", "run", 2),
            class_sym("service.ts", "Beta", 3),
            method("service.ts", "Beta", "run", 4),
        ],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![(
        "service.ts".to_string(),
        Some(Language::TypeScript),
        service,
    )];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let alpha_run_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "service.ts::Alpha::run")
        .unwrap()
        .id;

    let res = engine.resolve_reference(
        "service.ts",
        Some(alpha_run_id),
        "run",
        EdgeKind::Calls,
        None,
    );
    assert_eq!(res, graphia::resolve::Resolution::Resolved(alpha_run_id));
}

#[test]
fn test_engine_overload_resolution_by_param_count() {
    let math = ParsedFile {
        symbols: vec![
            func_with_sig("math.cpp", "add", "add(int,int)", 1),
            func_with_sig("math.cpp", "add", "add(int,int,int)", 2),
        ],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![("math.cpp".to_string(), Some(Language::Cpp), math)];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let add2_id = graph
        .nodes
        .iter()
        .find(|n| n.signature.as_deref() == Some("add(int,int)"))
        .unwrap()
        .id;
    let add3_id = graph
        .nodes
        .iter()
        .find(|n| n.signature.as_deref() == Some("add(int,int,int)"))
        .unwrap()
        .id;

    let res2 = engine.resolve_reference("math.cpp", None, "add", EdgeKind::Calls, Some(2));
    assert_eq!(res2, graphia::resolve::Resolution::Resolved(add2_id));

    let res3 = engine.resolve_reference("math.cpp", None, "add", EdgeKind::Calls, Some(3));
    assert_eq!(res3, graphia::resolve::Resolution::Resolved(add3_id));

    let res_ambig = engine.resolve_reference("math.cpp", None, "add", EdgeKind::Calls, None);
    assert!(res_ambig.is_ambiguous());
}

#[test]
fn test_engine_import_alias() {
    let lib = ParsedFile {
        symbols: vec![func("lib.py", "original_handler", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let app = ParsedFile {
        symbols: vec![func("app.py", "start", 1)],
        imports: vec![Import {
            path: "from lib import original_handler as aliased_handler".to_string(),
            location: loc("app.py", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("lib.py".to_string(), Some(Language::Python), lib),
        ("app.py".to_string(), Some(Language::Python), app),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let target_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "lib.py::original_handler")
        .unwrap()
        .id;

    let res = engine.resolve_reference("app.py", None, "aliased_handler", EdgeKind::Calls, None);
    assert_eq!(res, graphia::resolve::Resolution::Resolved(target_id));
}

#[test]
fn test_engine_multi_hop_reexport() {
    let file_a = ParsedFile {
        symbols: vec![class_sym("a.ts", "Foo", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![graphia::parser::Export {
            name: "Foo".to_string(),
            location: loc("a.ts", 1),
            target: Some("a.ts::Foo".to_string()),
        }],
        type_references: vec![],
        ..Default::default()
    };
    let file_b = ParsedFile {
        symbols: vec![],
        imports: vec![Import {
            path: "import { Foo } from './a'".to_string(),
            location: loc("b.ts", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![graphia::parser::Export {
            name: "Foo".to_string(),
            location: loc("b.ts", 2),
            target: Some("a.ts::Foo".to_string()),
        }],
        type_references: vec![],
        ..Default::default()
    };
    let file_c = ParsedFile {
        symbols: vec![func("c.ts", "main", 1)],
        imports: vec![Import {
            path: "import { Foo } from './b'".to_string(),
            location: loc("c.ts", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("a.ts".to_string(), Some(Language::TypeScript), file_a),
        ("b.ts".to_string(), Some(Language::TypeScript), file_b),
        ("c.ts".to_string(), Some(Language::TypeScript), file_c),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let foo_a_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "a.ts::Foo")
        .unwrap()
        .id;

    let res = engine.resolve_type_reference("c.ts", "Foo");
    assert_eq!(res, graphia::resolve::Resolution::Resolved(foo_a_id));
}

#[test]
fn test_engine_receiver_method() {
    let service = ParsedFile {
        symbols: vec![
            class_sym("service.ts", "AuthService", 1),
            method("service.ts", "AuthService", "verify", 2),
        ],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let client = ParsedFile {
        symbols: vec![func("client.ts", "login", 1)],
        imports: vec![Import {
            path: "import { AuthService } from './service'".to_string(),
            location: loc("client.ts", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        (
            "service.ts".to_string(),
            Some(Language::TypeScript),
            service,
        ),
        ("client.ts".to_string(), Some(Language::TypeScript), client),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let verify_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "service.ts::AuthService::verify")
        .unwrap()
        .id;

    let client_login_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "client.ts::login")
        .unwrap()
        .id;

    let res = engine.resolve_reference(
        "client.ts",
        Some(client_login_id),
        "verify",
        EdgeKind::Calls,
        None,
    );
    assert_eq!(res, graphia::resolve::Resolution::Resolved(verify_id));
}

#[test]
fn test_engine_type_reference() {
    let model = ParsedFile {
        symbols: vec![class_sym("model.rs", "UserRecord", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let handler = ParsedFile {
        symbols: vec![func("handler.rs", "handle_request", 1)],
        imports: vec![Import {
            path: "use model::UserRecord;".to_string(),
            location: loc("handler.rs", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("model.rs".to_string(), Some(Language::Rust), model),
        ("handler.rs".to_string(), Some(Language::Rust), handler),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let user_record_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "model.rs::UserRecord")
        .unwrap()
        .id;

    let res = engine.resolve_type_reference("handler.rs", "UserRecord");
    assert_eq!(res, graphia::resolve::Resolution::Resolved(user_record_id));
}

#[test]
fn test_engine_instantiation() {
    let db = ParsedFile {
        symbols: vec![class_sym("db.py", "ConnectionPool", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let app = ParsedFile {
        symbols: vec![func("app.py", "init_db", 1)],
        imports: vec![Import {
            path: "from db import ConnectionPool".to_string(),
            location: loc("app.py", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("db.py".to_string(), Some(Language::Python), db),
        ("app.py".to_string(), Some(Language::Python), app),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let pool_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "db.py::ConnectionPool")
        .unwrap()
        .id;

    let res = engine.resolve_instantiation("app.py", None, "ConnectionPool");
    assert_eq!(res, graphia::resolve::Resolution::Resolved(pool_id));
}

#[test]
fn test_engine_inheritance() {
    let base = ParsedFile {
        symbols: vec![class_sym("base.py", "BaseController", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let derived = ParsedFile {
        symbols: vec![class_sym("user_ctrl.py", "UserController", 1)],
        imports: vec![Import {
            path: "from base import BaseController".to_string(),
            location: loc("user_ctrl.py", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("base.py".to_string(), Some(Language::Python), base),
        ("user_ctrl.py".to_string(), Some(Language::Python), derived),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let base_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "base.py::BaseController")
        .unwrap()
        .id;

    let res = engine.resolve_inheritance("user_ctrl.py", "UserController", "BaseController");
    assert_eq!(res, graphia::resolve::Resolution::Resolved(base_id));
}

#[test]
fn test_engine_implementation() {
    let proto = ParsedFile {
        symbols: vec![trait_sym("repo.rs", "Repository", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let impl_file = ParsedFile {
        symbols: vec![class_sym("pg_repo.rs", "PostgresRepository", 1)],
        imports: vec![Import {
            path: "use repo::Repository;".to_string(),
            location: loc("pg_repo.rs", 1),
        }],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("repo.rs".to_string(), Some(Language::Rust), proto),
        ("pg_repo.rs".to_string(), Some(Language::Rust), impl_file),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let repo_id = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "repo.rs::Repository")
        .unwrap()
        .id;

    let res = engine.resolve_implementation("pg_repo.rs", "PostgresRepository", "Repository");
    assert_eq!(res, graphia::resolve::Resolution::Resolved(repo_id));
}

#[test]
fn test_engine_ambiguous_reference() {
    let mod_a = ParsedFile {
        symbols: vec![func("mod_a.rs", "duplicate_fn", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let mod_b = ParsedFile {
        symbols: vec![func("mod_b.rs", "duplicate_fn", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };
    let caller = ParsedFile {
        symbols: vec![func("caller.rs", "main", 1)],
        imports: vec![
            Import {
                path: "use mod_a::duplicate_fn;".to_string(),
                location: loc("caller.rs", 1),
            },
            Import {
                path: "use mod_b::duplicate_fn;".to_string(),
                location: loc("caller.rs", 2),
            },
        ],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![
        ("mod_a.rs".to_string(), Some(Language::Rust), mod_a),
        ("mod_b.rs".to_string(), Some(Language::Rust), mod_b),
        ("caller.rs".to_string(), Some(Language::Rust), caller),
    ];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let res = engine.resolve_reference("caller.rs", None, "duplicate_fn", EdgeKind::Calls, None);
    assert!(res.is_ambiguous());
    if let graphia::resolve::Resolution::Ambiguous(candidates) = res {
        assert_eq!(candidates.len(), 2);
    } else {
        panic!("Expected Ambiguous resolution");
    }
}

#[test]
fn test_engine_unresolved_reference() {
    let caller = ParsedFile {
        symbols: vec![func("caller.rs", "main", 1)],
        imports: vec![],
        calls: vec![],
        definitions: vec![],
        references: vec![],
        exports: vec![],
        type_references: vec![],
        ..Default::default()
    };

    let files = vec![("caller.rs".to_string(), Some(Language::Rust), caller)];
    let graph = build_graph(files.clone());
    let mut engine = graphia::resolve::ResolutionEngine::new();
    engine.index_files(&graph.nodes, &files);

    let res = engine.resolve_reference(
        "caller.rs",
        None,
        "non_existent_symbol",
        EdgeKind::Calls,
        None,
    );
    assert_eq!(res, graphia::resolve::Resolution::Unresolved);
}
