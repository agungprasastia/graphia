use std::path::Path;

use graphia::model::{EdgeKind, Language, NodeKind};
use graphia::parse::{CCppAnalyzer, GoAnalyzer, LanguageAnalyzer};
use graphia::parser::parse_file;
use graphia::scan::detect_language;
use graphia::storage::build_graph_from_repo;
use tempfile::TempDir;

#[test]
fn test_language_detection_phase_b() {
    assert_eq!(
        detect_language(Path::new("pkg/server.go")),
        Some(Language::Go)
    );
    assert_eq!(detect_language(Path::new("src/main.c")), Some(Language::C));
    assert_eq!(
        detect_language(Path::new("include/header.h")),
        Some(Language::C)
    );
    assert_eq!(
        detect_language(Path::new("src/engine.cpp")),
        Some(Language::Cpp)
    );
    assert_eq!(
        detect_language(Path::new("src/core.cc")),
        Some(Language::Cpp)
    );
    assert_eq!(
        detect_language(Path::new("src/core.cxx")),
        Some(Language::Cpp)
    );
    assert_eq!(
        detect_language(Path::new("include/engine.hpp")),
        Some(Language::Cpp)
    );
    assert_eq!(
        detect_language(Path::new("include/engine.hxx")),
        Some(Language::Cpp)
    );
    assert_eq!(
        detect_language(Path::new("include/engine.hh")),
        Some(Language::Cpp)
    );
}

#[test]
fn test_go_analyzer_trait_implementation() {
    let go_analyzer = GoAnalyzer::new();
    assert_eq!(go_analyzer.language(), Language::Go);

    let code = "package main\nfunc hello() int { return 1 }";
    let parsed = go_analyzer
        .analyze("test.go", code.as_bytes())
        .expect("analyze success");

    assert!(
        parsed.symbols.iter().any(
            |s| s.name == "main" && (s.kind == NodeKind::Package || s.kind == NodeKind::Module)
        )
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "hello" && s.kind == NodeKind::Function)
    );
}

#[test]
fn test_cpp_analyzer_trait_implementation() {
    let c_analyzer = CCppAnalyzer::new(Language::C);
    assert_eq!(c_analyzer.language(), Language::C);

    let cpp_analyzer = CCppAnalyzer::new(Language::Cpp);
    assert_eq!(cpp_analyzer.language(), Language::Cpp);

    let code = "int add(int a, int b) { return a + b; }";
    let parsed = c_analyzer
        .analyze("test.c", code.as_bytes())
        .expect("analyze success");

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "add" && s.kind == NodeKind::Function)
    );
}

#[test]
fn test_parse_go_sample_fixture() {
    let content = include_str!("fixtures/go/sample.go");
    let parsed = parse_file("sample.go", Language::Go, content);

    // Package symbol
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "sample"
                && (s.kind == NodeKind::Package || s.kind == NodeKind::Module))
    );

    // Types: Server (Struct), Service (Interface), ConfigMap (Struct / Type Alias)
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Server" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Service" && s.kind == NodeKind::Interface)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "ConfigMap" && s.kind == NodeKind::Struct)
    );

    // Functions: NewServer, computeStats
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "NewServer" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "computeStats" && s.kind == NodeKind::Function)
    );

    // Methods with receiver Server: Start, handleRequests
    assert!(parsed.symbols.iter().any(|s| s.name == "Start"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("Server")));
    assert!(parsed.symbols.iter().any(|s| s.name == "handleRequests"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("Server")));

    // Imports: "fmt", "math"
    assert!(parsed.imports.iter().any(|i| i.path == "fmt"));
    assert!(parsed.imports.iter().any(|i| i.path == "math"));

    // Calls: Println in NewServer, handleRequests in Start, computeStats in handleRequests
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "Println" && c.caller == "sample.go::NewServer")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "handleRequests" && c.caller == "sample.go::Start")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "computeStats" && c.caller == "sample.go::handleRequests")
    );
}

#[test]
fn test_parse_c_sample_fixture() {
    let content = include_str!("fixtures/c/sample.c");
    let parsed = parse_file("sample.c", Language::C, content);

    // Struct / Typedef: Point / Point_t, custom_int
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Point" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed.symbols.iter().any(|s| s.name == "Point_t"
            && (s.kind == NodeKind::TypeAlias || s.kind == NodeKind::Struct))
    );

    // Functions: init_point, calculate_area, main
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "init_point" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "calculate_area" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == NodeKind::Function)
    );

    // Includes: stdio.h, helper.h
    assert!(parsed.imports.iter().any(|i| i.path == "stdio.h"));
    assert!(parsed.imports.iter().any(|i| i.path == "helper.h"));

    // Calls: helper_print in calculate_area, init_point & calculate_area in main
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "helper_print" && c.caller == "sample.c::calculate_area")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "init_point" && c.caller == "sample.c::main")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "calculate_area" && c.caller == "sample.c::main")
    );
}

#[test]
fn test_parse_cpp_sample_fixture() {
    let content = include_str!("fixtures/cpp/sample.cpp");
    let parsed = parse_file("sample.cpp", Language::Cpp, content);

    // Namespace: Engine
    assert!(parsed.symbols.iter().any(
        |s| s.name == "Engine" && (s.kind == NodeKind::Namespace || s.kind == NodeKind::Module)
    ));

    // Class: Renderer, Struct: Buffer, Using: BufferList
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Renderer" && s.kind == NodeKind::Class)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Buffer" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed.symbols.iter().any(|s| s.name == "BufferList"
            && (s.kind == NodeKind::TypeAlias || s.kind == NodeKind::Struct))
    );

    // Methods: renderScene, getFps, calculateFps (parent = Renderer)
    assert!(parsed.symbols.iter().any(|s| s.name == "renderScene"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("Renderer")));
    assert!(parsed.symbols.iter().any(|s| s.name == "getFps"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("Renderer")));
    assert!(parsed.symbols.iter().any(|s| s.name == "calculateFps"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("Renderer")));

    // Function: runEngine
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "runEngine" && s.kind == NodeKind::Function)
    );

    // Includes: iostream, vector, helper.h
    assert!(parsed.imports.iter().any(|i| i.path == "iostream"));
    assert!(parsed.imports.iter().any(|i| i.path == "helper.h"));

    // Calls: calculateFps in getFps, helper_print in renderScene, renderScene in runEngine
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "calculateFps" && c.caller == "sample.cpp::getFps")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "helper_print" && c.caller == "sample.cpp::renderScene")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "renderScene" && c.caller == "sample.cpp::runEngine")
    );
}

#[test]
fn test_malformed_syntax_error_recovery_phase_b() {
    let go_content = include_str!("fixtures/go/malformed.go");
    let go_parsed = parse_file("malformed.go", Language::Go, go_content);
    assert!(
        go_parsed
            .symbols
            .iter()
            .any(|s| s.name == "ValidFuncAfter" && s.kind == NodeKind::Function)
    );

    let cpp_content = include_str!("fixtures/cpp/malformed.cpp");
    let cpp_parsed = parse_file("malformed.cpp", Language::Cpp, cpp_content);
    assert!(
        cpp_parsed
            .symbols
            .iter()
            .any(|s| s.name == "valid_after_error" && s.kind == NodeKind::Function)
    );
}

#[test]
fn test_phase_b_graph_build_and_resolution() {
    let temp_dir = TempDir::new().expect("temp dir");
    let root = temp_dir.path();

    let sample_c = include_str!("fixtures/c/sample.c");
    let helper_h = include_str!("fixtures/c/helper.h");
    let sample_go = include_str!("fixtures/go/sample.go");

    std::fs::write(root.join("sample.c"), sample_c).expect("write sample.c");
    std::fs::write(root.join("helper.h"), helper_h).expect("write helper.h");
    std::fs::write(root.join("sample.go"), sample_go).expect("write sample.go");

    let graph = build_graph_from_repo(root).expect("build graph");
    graph.validate().expect("graph should be valid");

    // Check files are present
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "sample.c" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "helper.h" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "sample.go" && n.kind == NodeKind::File)
    );

    // Check import edge between sample.c and helper.h
    let sample_c_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "sample.c")
        .expect("sample.c node");
    let helper_h_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "helper.h")
        .expect("helper.h node");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Imports && e.from == sample_c_node.id && e.to == helper_h_node.id
        }),
        "sample.c should import helper.h"
    );

    // Check call edge within sample.c (main -> init_point)
    let main_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "sample.c::main")
        .expect("main node");
    let init_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "sample.c::init_point")
        .expect("init_point node");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Calls && e.from == main_node.id && e.to == init_node.id
        }),
        "main should call init_point"
    );
}
