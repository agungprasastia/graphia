use std::path::Path;

use graphia::model::{EdgeKind, Language, NodeKind};
use graphia::parse::{CSharpAnalyzer, JavaAnalyzer, KotlinAnalyzer, LanguageAnalyzer};
use graphia::parser::parse_file;
use graphia::scan::detect_language;
use graphia::storage::build_graph_from_repo;
use tempfile::TempDir;

#[test]
fn test_language_detection_phase_c() {
    assert_eq!(
        detect_language(Path::new("src/main/java/App.java")),
        Some(Language::Java)
    );
    assert_eq!(
        detect_language(Path::new("src/Program.cs")),
        Some(Language::CSharp)
    );
    assert_eq!(
        detect_language(Path::new("src/main/kotlin/App.kt")),
        Some(Language::Kotlin)
    );
    assert_eq!(
        detect_language(Path::new("build.gradle.kts")),
        Some(Language::Kotlin)
    );
}

#[test]
fn test_java_analyzer_trait_implementation() {
    let analyzer = JavaAnalyzer::new();
    assert_eq!(analyzer.language(), Language::Java);

    let code = "package com.test;\npublic class TestClass {\n    public void doSomething() {}\n}";
    let parsed = analyzer
        .analyze("TestClass.java", code.as_bytes())
        .expect("analyze success");

    assert!(parsed.symbols.iter().any(
        |s| s.name == "com.test" && (s.kind == NodeKind::Package || s.kind == NodeKind::Module)
    ));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "TestClass" && s.kind == NodeKind::Class)
    );
    assert!(parsed.symbols.iter().any(|s| s.name == "doSomething"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("TestClass")));
}

#[test]
fn test_csharp_analyzer_trait_implementation() {
    let analyzer = CSharpAnalyzer::new();
    assert_eq!(analyzer.language(), Language::CSharp);

    let code = "namespace MyNamespace;\npublic class Worker {\n    public void Execute() {}\n}";
    let parsed = analyzer
        .analyze("Worker.cs", code.as_bytes())
        .expect("analyze success");

    assert!(parsed.symbols.iter().any(|s| s.name == "MyNamespace"
        && (s.kind == NodeKind::Namespace || s.kind == NodeKind::Module)));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Worker" && s.kind == NodeKind::Class)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Execute" && s.kind == NodeKind::Method)
    );
}

#[test]
fn test_kotlin_analyzer_trait_implementation() {
    let analyzer = KotlinAnalyzer::new();
    assert_eq!(analyzer.language(), Language::Kotlin);

    let code = "package my.pkg\nfun main() {}\nclass App";
    let parsed = analyzer
        .analyze("App.kt", code.as_bytes())
        .expect("analyze success");

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "my.pkg"
                && (s.kind == NodeKind::Package || s.kind == NodeKind::Module))
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "main" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "App" && s.kind == NodeKind::Class)
    );
}

#[test]
fn test_parse_java_sample_fixture() {
    let content = include_str!("fixtures/java/SampleService.java");
    let parsed = parse_file("SampleService.java", Language::Java, content);

    // Package symbol
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "com.example.service"
                && (s.kind == NodeKind::Package || s.kind == NodeKind::Module))
    );

    // Types: SampleService (Class), IService (Interface), UserRecord (Struct), Status (Struct/Enum)
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "SampleService" && s.kind == NodeKind::Class)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "IService" && s.kind == NodeKind::Interface)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "UserRecord" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Status" && (s.kind == NodeKind::Enum || s.kind == NodeKind::Struct))
    );

    // Methods & Constructor
    assert!(parsed.symbols.iter().any(|s| s.name == "SampleService"
        && (s.kind == NodeKind::Constructor || s.kind == NodeKind::Method)
        && s.parent.as_deref() == Some("SampleService")));
    assert!(parsed.symbols.iter().any(|s| s.name == "start"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("SampleService")));
    assert!(parsed.symbols.iter().any(|s| s.name == "processRequest"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("SampleService")));

    // Imports: java.util.List, com.example.service.Helper
    assert!(parsed.imports.iter().any(|i| i.path == "java.util.List"));
    assert!(
        parsed
            .imports
            .iter()
            .any(|i| i.path == "com.example.service.Helper")
    );

    // Calls: processRequest in start, Helper in processRequest, doWork in processRequest
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "processRequest" && c.caller == "SampleService.java::start")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "Helper" && c.caller == "SampleService.java::processRequest")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "doWork" && c.caller == "SampleService.java::processRequest")
    );
}

#[test]
fn test_parse_csharp_sample_fixture() {
    let content = include_str!("fixtures/csharp/SampleService.cs");
    let parsed = parse_file("SampleService.cs", Language::CSharp, content);

    // Namespace
    assert!(
        parsed.symbols.iter().any(|s| s.name == "SampleApp"
            && (s.kind == NodeKind::Namespace || s.kind == NodeKind::Module))
    );

    // Types: DataService (Class), IProcessor (Interface), PointStruct (Struct), UserDto (Struct), Priority (Struct)
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "DataService" && s.kind == NodeKind::Class)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "IProcessor" && s.kind == NodeKind::Interface)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "PointStruct" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "UserDto" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Priority"
                && (s.kind == NodeKind::Enum || s.kind == NodeKind::Struct))
    );

    // Methods & Properties
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Name"
                && (s.kind == NodeKind::Property || s.kind == NodeKind::Method))
    );
    assert!(parsed.symbols.iter().any(|s| s.name == "DataService"
        && (s.kind == NodeKind::Constructor || s.kind == NodeKind::Method)));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Execute" && s.kind == NodeKind::Method)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "RunInternal" && s.kind == NodeKind::Method)
    );

    // Using
    assert!(parsed.imports.iter().any(|i| i.path == "System"));
    assert!(
        parsed
            .imports
            .iter()
            .any(|i| i.path == "System.Collections.Generic")
    );

    // Calls: RunInternal in Execute, CSharpHelper & Process in RunInternal
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "RunInternal" && c.caller == "SampleService.cs::Execute")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "CSharpHelper" && c.caller == "SampleService.cs::RunInternal")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "Process" && c.caller == "SampleService.cs::RunInternal")
    );
}

#[test]
fn test_parse_kotlin_sample_fixture() {
    let content = include_str!("fixtures/kotlin/SampleService.kt");
    let parsed = parse_file("SampleService.kt", Language::Kotlin, content);

    // Package
    assert!(parsed.symbols.iter().any(|s| s.name == "com.example.kotlin"
        && (s.kind == NodeKind::Package || s.kind == NodeKind::Module)));

    // Types: IWorker (Interface), UserInfo (Struct/DataClass), AppConfig (Class/Object), TaskManager (Class)
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "IWorker" && s.kind == NodeKind::Interface)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "UserInfo" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "AppConfig" && s.kind == NodeKind::Class)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "TaskManager" && s.kind == NodeKind::Class)
    );

    // Functions / Methods
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "work" && s.kind == NodeKind::Method)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "executeTask" && s.kind == NodeKind::Method)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "standaloneTask" && s.kind == NodeKind::Function)
    );

    // Imports
    assert!(
        parsed
            .imports
            .iter()
            .any(|i| i.path == "com.example.kotlin.KotlinHelper")
    );

    // Calls
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "executeTask" && c.caller == "SampleService.kt::work")
    );
    assert!(
        parsed
            .calls
            .iter()
            .any(|c| c.callee == "assist" && c.caller == "SampleService.kt::executeTask")
    );
}

#[test]
fn test_malformed_syntax_error_recovery_phase_c() {
    let java_content = include_str!("fixtures/java/malformed.java");
    let java_parsed = parse_file("malformed.java", Language::Java, java_content);
    assert!(
        java_parsed
            .symbols
            .iter()
            .any(|s| s.name == "validMethodAfter" && s.kind == NodeKind::Method)
    );

    let cs_content = include_str!("fixtures/csharp/malformed.cs");
    let cs_parsed = parse_file("malformed.cs", Language::CSharp, cs_content);
    assert!(
        cs_parsed
            .symbols
            .iter()
            .any(|s| s.name == "ValidCSharpAfter" && s.kind == NodeKind::Method)
    );

    let kt_content = include_str!("fixtures/kotlin/malformed.kt");
    let kt_parsed = parse_file("malformed.kt", Language::Kotlin, kt_content);
    assert!(
        kt_parsed
            .symbols
            .iter()
            .any(|s| s.name == "validKotlinAfter" && s.kind == NodeKind::Method)
    );
}

#[test]
fn test_phase_c_graph_build_and_resolution() {
    let temp_dir = TempDir::new().expect("temp dir");
    let root = temp_dir.path();

    let sample_java = include_str!("fixtures/java/SampleService.java");
    let helper_java = include_str!("fixtures/java/Helper.java");
    let sample_cs = include_str!("fixtures/csharp/SampleService.cs");
    let helper_cs = include_str!("fixtures/csharp/CSharpHelper.cs");
    let sample_kt = include_str!("fixtures/kotlin/SampleService.kt");
    let helper_kt = include_str!("fixtures/kotlin/KotlinHelper.kt");

    std::fs::write(root.join("SampleService.java"), sample_java).expect("write SampleService.java");
    std::fs::write(root.join("Helper.java"), helper_java).expect("write Helper.java");
    std::fs::write(root.join("SampleService.cs"), sample_cs).expect("write SampleService.cs");
    std::fs::write(root.join("CSharpHelper.cs"), helper_cs).expect("write CSharpHelper.cs");
    std::fs::write(root.join("SampleService.kt"), sample_kt).expect("write SampleService.kt");
    std::fs::write(root.join("KotlinHelper.kt"), helper_kt).expect("write KotlinHelper.kt");

    let graph = build_graph_from_repo(root).expect("build graph");
    graph.validate().expect("graph should be valid");

    // Check files are present
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "SampleService.java" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "Helper.java" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "SampleService.cs" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "CSharpHelper.cs" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "SampleService.kt" && n.kind == NodeKind::File)
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "KotlinHelper.kt" && n.kind == NodeKind::File)
    );

    // Check intra-file and cross-file resolution in Java
    let java_sample_file = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "SampleService.java")
        .expect("SampleService.java node");
    let java_helper_file = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "Helper.java")
        .expect("Helper.java node");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Imports
                && e.from == java_sample_file.id
                && e.to == java_helper_file.id
        }),
        "SampleService.java should import Helper.java"
    );

    let start_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "SampleService.java::start")
        .expect("start node");
    let process_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "SampleService.java::processRequest")
        .expect("processRequest node");
    let do_work_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "Helper.java::doWork")
        .expect("doWork node");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Calls && e.from == start_node.id && e.to == process_node.id
        }),
        "start should call processRequest"
    );

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Calls
                && e.from == process_node.id
                && e.to == do_work_node.id
        }),
        "processRequest should call doWork on Helper"
    );

    // Check cross-file resolution in Kotlin
    let kt_sample_file = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "SampleService.kt")
        .expect("SampleService.kt node");
    let kt_helper_file = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "KotlinHelper.kt")
        .expect("KotlinHelper.kt node");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Imports && e.from == kt_sample_file.id && e.to == kt_helper_file.id
        }),
        "SampleService.kt should import KotlinHelper.kt"
    );

    let execute_task_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "SampleService.kt::executeTask")
        .expect("executeTask node");
    let assist_node = graph
        .nodes
        .iter()
        .find(|n| n.qualified_name == "KotlinHelper.kt::assist")
        .expect("assist node");

    assert!(
        graph.edges.iter().any(|e| {
            e.kind == EdgeKind::Calls && e.from == execute_task_node.id && e.to == assist_node.id
        }),
        "executeTask should call assist on KotlinHelper"
    );
}
