use std::path::Path;

use graphia::model::{EdgeKind, Language, NodeKind};
use graphia::parse::{JavaScriptAnalyzer, LanguageAnalyzer};
use graphia::parser::parse_file;
use graphia::scan::detect_language;
use graphia::storage::build_graph_from_repo;
use tempfile::TempDir;

#[test]
fn test_language_detection_phase_a() {
    assert_eq!(
        detect_language(Path::new("app/index.js")),
        Some(Language::JavaScript)
    );
    assert_eq!(
        detect_language(Path::new("app/index.mjs")),
        Some(Language::JavaScript)
    );
    assert_eq!(
        detect_language(Path::new("app/index.cjs")),
        Some(Language::JavaScript)
    );
    assert_eq!(
        detect_language(Path::new("app/Button.jsx")),
        Some(Language::Jsx)
    );
    assert_eq!(
        detect_language(Path::new("app/Widget.tsx")),
        Some(Language::Tsx)
    );
    assert_eq!(
        detect_language(Path::new("app/utils.ts")),
        Some(Language::TypeScript)
    );
}

#[test]
fn test_javascript_analyzer_trait_implementation() {
    let js_analyzer = JavaScriptAnalyzer::new(Language::JavaScript);
    assert_eq!(js_analyzer.language(), Language::JavaScript);

    let code = "function hello() { return 1; }";
    let parsed = js_analyzer
        .analyze("test.js", code.as_bytes())
        .expect("analyze success");
    assert_eq!(parsed.symbols.len(), 1);
    assert_eq!(parsed.symbols[0].name, "hello");
    assert_eq!(parsed.symbols[0].kind, NodeKind::Function);
}

#[test]
fn test_parse_javascript_sample_fixture() {
    let content = include_str!("fixtures/javascript/sample.js");
    let parsed = parse_file("sample.js", Language::JavaScript, content);

    // Symbols: Calculator (Class), add (Method), computeTotal (Function), multiply (Function)
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Calculator" && s.kind == NodeKind::Class)
    );
    assert!(parsed.symbols.iter().any(|s| s.name == "add"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("Calculator")));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "computeTotal" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "multiply" && s.kind == NodeKind::Function)
    );

    // Imports: require and import
    assert!(
        parsed
            .imports
            .iter()
            .any(|i| i.path.contains("require('./helper.js')"))
    );
    assert!(
        parsed
            .imports
            .iter()
            .any(|i| i.path.contains("import { extraUtil } from './extra.mjs'"))
    );

    // Calls: helperUtil in add, extraUtil in computeTotal, add in computeTotal
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "helperUtil" && c.caller == "sample.js::add")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "extraUtil" && c.caller == "sample.js::computeTotal")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "add" && c.caller == "sample.js::computeTotal")
    );
}

#[test]
fn test_parse_jsx_component_fixture() {
    let content = include_str!("fixtures/jsx/component.jsx");
    let parsed = parse_file("component.jsx", Language::Jsx, content);

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "ButtonComponent" && s.kind == NodeKind::Class)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "render" && s.kind == NodeKind::Method)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Header" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .imports
            .iter()
            .any(|i| i.path.contains("from 'react'"))
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "computeTotal" && c.caller == "component.jsx::render")
    );
}

#[test]
fn test_parse_tsx_widget_fixture() {
    let content = include_str!("fixtures/tsx/widget.tsx");
    let parsed = parse_file("widget.tsx", Language::Tsx, content);

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "WidgetProps" && s.kind == NodeKind::Interface)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "WidgetContainer" && s.kind == NodeKind::Class)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "calculateCount" && s.kind == NodeKind::Method)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "WidgetView" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "multiply" && c.caller == "widget.tsx::calculateCount")
    );
}

#[test]
fn test_malformed_syntax_graceful_recovery() {
    let content = include_str!("fixtures/javascript/malformed.js");
    let parsed = parse_file("malformed.js", Language::JavaScript, content);

    // Tree-sitter is error-tolerant; it will still extract valid declarations after errors
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "validAfter" && s.kind == NodeKind::Function)
    );
}

#[test]
fn test_phase_a_graph_build_and_resolution() {
    let temp_dir = TempDir::new().expect("temp dir");
    let root = temp_dir.path();

    let sample_js = include_str!("fixtures/javascript/sample.js");
    let component_jsx = include_str!("fixtures/jsx/component.jsx");
    let widget_tsx = include_str!("fixtures/tsx/widget.tsx");

    std::fs::write(root.join("sample.js"), sample_js).expect("write sample.js");
    std::fs::write(root.join("component.jsx"), component_jsx).expect("write component.jsx");
    std::fs::write(root.join("widget.tsx"), widget_tsx).expect("write widget.tsx");

    let graph = build_graph_from_repo(root).expect("build graph");
    graph.validate().expect("graph should be valid");

    // Check that files are created as nodes
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "sample.js" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "component.jsx" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "widget.tsx" && n.kind == NodeKind::File)
    );

    // Check import edge between component.jsx and sample.js
    let comp_file = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "component.jsx")
        .expect("component node");
    let sample_file = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "sample.js")
        .expect("sample node");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Imports && e.from == comp_file.id && e.to == sample_file.id
        }),
        "component.jsx should import sample.js"
    );

    // Check call edge between component.jsx::render and sample.js::computeTotal
    let render_method = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "component.jsx::render")
        .expect("render method");
    let compute_func = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "sample.js::computeTotal")
        .expect("computeTotal func");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Calls && e.from == render_method.id && e.to == compute_func.id
        }),
        "render method in component.jsx should call computeTotal in sample.js"
    );
}
