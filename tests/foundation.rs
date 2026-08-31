use std::fs;
use std::path::{Path, PathBuf};

use graphia::model::{EdgeKind, Language, NodeKind};
use graphia::parser::parse_file;
use graphia::scan::scan_repo;
use graphia::storage::{build_graph_from_repo, save_graph_json};
use tempfile::TempDir;

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn copy_fixture_tree() -> TempDir {
    let root = tempfile::tempdir().expect("fixture tempdir");
    for (language, names) in [
        ("rust", ["basic.rs", "helper.rs"]),
        ("python", ["basic.py", "helper.py"]),
        ("typescript", ["basic.ts", "helper.ts"]),
    ] {
        for name in names {
            let source = fixture_root().join(language).join(name);
            let destination = root.path().join(language).join(name);
            fs::create_dir_all(destination.parent().expect("fixture parent"))
                .expect("create fixture directory");
            fs::copy(source, destination).expect("copy fixture");
        }
    }
    root
}

#[test]
fn scanner_orders_fixture_files_and_detects_languages() {
    let root = copy_fixture_tree();

    let files = scan_repo(root.path()).expect("scan fixtures");
    let paths: Vec<_> = files
        .iter()
        .map(|file| file.relative_path.as_str())
        .collect();

    assert_eq!(
        paths,
        [
            "python/basic.py",
            "python/helper.py",
            "rust/basic.rs",
            "rust/helper.rs",
            "typescript/basic.ts",
            "typescript/helper.ts",
        ]
    );
    assert_eq!(files[0].language, Some(Language::Python));
    assert_eq!(files[2].language, Some(Language::Rust));
    assert_eq!(files[4].language, Some(Language::TypeScript));
}

#[test]
fn parsers_extract_symbols_imports_calls_and_locations() {
    type SymbolExpectation = (&'static str, NodeKind, u32);
    type Case = (
        &'static str,
        Language,
        &'static str,
        &'static [SymbolExpectation],
        &'static str,
        &'static str,
    );
    let cases: [Case; 3] = [
        (
            "rust/basic.rs",
            Language::Rust,
            include_str!("fixtures/rust/basic.rs"),
            &[
                ("RustThing", NodeKind::Struct, 3),
                ("RustTrait", NodeKind::Trait, 5),
                ("nested", NodeKind::Module, 7),
                ("rust_entry", NodeKind::Function, 9),
                ("rust_method", NodeKind::Method, 14),
            ],
            "rust::helper",
            "rust/basic.rs::rust_entry",
        ),
        (
            "python/basic.py",
            Language::Python,
            include_str!("fixtures/python/basic.py"),
            &[
                ("PythonThing", NodeKind::Class, 4),
                ("python_method", NodeKind::Method, 5),
                ("python_entry", NodeKind::Function, 9),
            ],
            "from python.helper import helper",
            "python/basic.py::python_method",
        ),
        (
            "typescript/basic.ts",
            Language::TypeScript,
            include_str!("fixtures/typescript/basic.ts"),
            &[
                ("TypeScriptContract", NodeKind::Interface, 3),
                ("TypeScriptThing", NodeKind::Class, 5),
                ("typescript_method", NodeKind::Method, 6),
                ("typescript_entry", NodeKind::Function, 11),
            ],
            "import { helper } from \"./helper\"",
            "typescript/basic.ts::typescript_method",
        ),
    ];

    for (file, language, content, symbols, import_path, call) in cases {
        let parsed = parse_file(file, language, content);
        for &(name, kind, start_line) in symbols {
            assert!(
                parsed.symbols.iter().any(|candidate| {
                    candidate.name == name
                        && candidate.kind == kind
                        && candidate.location.start_line == start_line
                        && candidate.location.start_col
                            == match language {
                                Language::TypeScript if kind == NodeKind::Method => 3,
                                Language::Python if kind == NodeKind::Method => 5,
                                Language::Rust if kind == NodeKind::Method => 5,
                                _ => 1,
                            }
                }),
                "missing expected {kind:?} {name} at {file}:{start_line}:1"
            );
        }
        assert!(parsed.imports.iter().any(|item| item.path == import_path));
        assert!(
            parsed
                .calls
                .iter()
                .any(|item| item.callee == "helper" && item.caller == call)
        );
        assert!(
            parsed
                .symbols
                .iter()
                .all(|symbol| symbol.location.file == file)
        );
        assert!(parsed.imports.iter().all(|item| item.location.file == file));
        assert!(parsed.calls.iter().all(|item| item.location.file == file));
    }
}

#[test]
fn graph_has_file_containment_and_stable_ids() {
    let first_root = copy_fixture_tree();
    let first = build_graph_from_repo(first_root.path()).expect("first graph");
    let second_root = copy_fixture_tree();
    let second = build_graph_from_repo(second_root.path()).expect("second graph");

    assert_eq!(first, second);
    first.validate().expect("valid graph");
    assert!(first.nodes.iter().any(|node| node.kind == NodeKind::File));
    assert!(
        first
            .edges
            .iter()
            .any(|edge| edge.kind == EdgeKind::Contains)
    );
    for (file, caller) in [
        ("rust/basic.rs", "rust/basic.rs::rust_entry"),
        ("python/basic.py", "python/basic.py::python_method"),
        (
            "typescript/basic.ts",
            "typescript/basic.ts::typescript_method",
        ),
    ] {
        let file_id = first
            .nodes
            .iter()
            .find(|node| node.qualified_name == file)
            .expect("file node")
            .id;
        let helper_file_id = first
            .nodes
            .iter()
            .find(|node| node.qualified_name == file.replace("basic", "helper"))
            .expect("helper file node")
            .id;
        let helper_id = first
            .nodes
            .iter()
            .find(|node| {
                node.qualified_name == format!("{}::helper", file.replace("basic", "helper"))
            })
            .expect("helper symbol node")
            .id;
        let caller_id = first
            .nodes
            .iter()
            .find(|node| node.qualified_name == caller)
            .expect("caller node")
            .id;
        assert!(first.edges.iter().any(|edge| {
            edge.kind == EdgeKind::Imports && edge.from == file_id && edge.to == helper_file_id
        }));
        let res = first.edges.iter().any(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == caller_id && edge.to == helper_id
        });
        if !res {
            println!(
                "TEST FOUNDATION FAILURE for {file}, caller_id: {caller_id:?}, helper_id: {helper_id:?}"
            );
            for n in &first.nodes {
                if n.id == caller_id || n.id == helper_id {
                    println!("  NODE: {n:?}");
                }
            }
            for e in &first.edges {
                if e.from == caller_id {
                    println!("  OUTBOUND EDGE: {e:?}");
                }
            }
        }
        assert!(first.edges.iter().any(|edge| {
            edge.kind == EdgeKind::Calls && edge.from == caller_id && edge.to == helper_id
        }));
    }
}

#[test]
fn graph_json_is_byte_identical_across_builds() {
    let first_root = copy_fixture_tree();
    let first = build_graph_from_repo(first_root.path()).expect("first graph");
    let first_path = first_root.path().join("first.json");
    save_graph_json(&first, &first_path).expect("first JSON");

    let second_root = copy_fixture_tree();
    let second = build_graph_from_repo(second_root.path()).expect("second graph");
    let second_path = second_root.path().join("second.json");
    save_graph_json(&second, &second_path).expect("second JSON");

    assert_eq!(
        fs::read(first_path).expect("read first JSON"),
        fs::read(second_path).expect("read second JSON")
    );
}
