use std::path::Path;

use graphia::model::{EdgeKind, Language, NodeKind};
use graphia::parse::{LanguageAnalyzer, PhpAnalyzer, RubyAnalyzer, SwiftAnalyzer, ZigAnalyzer};
use graphia::parser::parse_file;
use graphia::scan::detect_language;
use graphia::storage::build_graph_from_repo;

#[test]
fn test_language_detection_phase_d() {
    assert_eq!(
        detect_language(Path::new("src/main.zig")),
        Some(Language::Zig)
    );
    assert_eq!(
        detect_language(Path::new("src/index.php")),
        Some(Language::Php)
    );
    assert_eq!(
        detect_language(Path::new("app/models/user.rb")),
        Some(Language::Ruby)
    );
    assert_eq!(
        detect_language(Path::new("Sources/App/main.swift")),
        Some(Language::Swift)
    );
}

#[test]
fn test_zig_analyzer_trait_implementation() {
    let analyzer = ZigAnalyzer::new();
    assert_eq!(analyzer.language(), Language::Zig);

    let code = r#"
const std = @import("std");
const helper = @import("helper.zig");

pub const Status = enum {
    Active,
    Inactive,
};

pub const User = struct {
    id: u32,
    pub fn init(id: u32) User {
        return User{ .id = id };
    }
};

pub fn runMain() void {
    helper.doWork();
}
"#;
    let parsed = analyzer
        .analyze("sample.zig", code.as_bytes())
        .expect("analyze success");

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Status" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "User" && s.kind == NodeKind::Struct)
    );
    assert!(parsed.symbols.iter().any(|s| s.name == "init"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("User")));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "runMain" && s.kind == NodeKind::Function)
    );
    assert!(parsed.imports.iter().any(|i| i.path == "std"));
    assert!(parsed.imports.iter().any(|i| i.path == "helper.zig"));
}

#[test]
fn test_php_analyzer_trait_implementation() {
    let analyzer = PhpAnalyzer::new();
    assert_eq!(analyzer.language(), Language::Php);

    let code = r#"<?php
namespace App\Services;
use App\Utils\Helper;

interface ProcessorInterface {
    public function process(): void;
}

trait LoggerTrait {
    public function logMessage(): void {}
}

class SampleService implements ProcessorInterface {
    use LoggerTrait;

    public function process(): void {
        Helper::doWork();
    }
}

function standaloneFunction(): void {}
"#;
    let parsed = analyzer
        .analyze("sample.php", code.as_bytes())
        .expect("analyze success");

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "App\\Services" && s.kind == NodeKind::Module)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "ProcessorInterface" && s.kind == NodeKind::Interface)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "LoggerTrait" && s.kind == NodeKind::Trait)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "SampleService" && s.kind == NodeKind::Class)
    );
    assert!(parsed.symbols.iter().any(|s| s.name == "process"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("SampleService")));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "standaloneFunction" && s.kind == NodeKind::Function)
    );
    assert!(
        parsed
            .imports
            .iter()
            .any(|i| i.path == "App\\Utils\\Helper")
    );
}

#[test]
fn test_ruby_analyzer_trait_implementation() {
    let analyzer = RubyAnalyzer::new();
    assert_eq!(analyzer.language(), Language::Ruby);

    let code = r#"
require_relative 'helper'

module Analytics
  class DataProcessor
    def initialize(name)
      @name = name
    end

    def execute
      Helper.do_work
    end
  end

  def self.run_pipeline
  end
end

def top_level_entry
end
"#;
    let parsed = analyzer
        .analyze("sample.rb", code.as_bytes())
        .expect("analyze success");

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Analytics" && s.kind == NodeKind::Module)
    );
    assert!(parsed.symbols.iter().any(|s| s.name == "DataProcessor"
        && s.kind == NodeKind::Class
        && s.parent.as_deref() == Some("Analytics")));
    assert!(parsed.symbols.iter().any(|s| s.name == "execute"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("DataProcessor")));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "top_level_entry" && s.kind == NodeKind::Function)
    );
    assert!(parsed.imports.iter().any(|i| i.path == "helper"));
}

#[test]
fn test_swift_analyzer_trait_implementation() {
    let analyzer = SwiftAnalyzer::new();
    assert_eq!(analyzer.language(), Language::Swift);

    let code = r#"
import Foundation
import SwiftHelper

protocol ServiceProtocol {
    func start()
}

struct Config {
    let id: String
}

class SampleService: ServiceProtocol {
    init() {}
    func start() {
        SwiftHelper.doWork()
    }
}

func globalBootstrap() {}
"#;
    let parsed = analyzer
        .analyze("sample.swift", code.as_bytes())
        .expect("analyze success");

    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "ServiceProtocol" && s.kind == NodeKind::Interface)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "Config" && s.kind == NodeKind::Struct)
    );
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "SampleService" && s.kind == NodeKind::Class)
    );
    assert!(parsed.symbols.iter().any(|s| s.name == "start"
        && s.kind == NodeKind::Method
        && s.parent.as_deref() == Some("SampleService")));
    assert!(
        parsed
            .symbols
            .iter()
            .any(|s| s.name == "globalBootstrap" && s.kind == NodeKind::Function)
    );
    assert!(parsed.imports.iter().any(|i| i.path == "Foundation"));
    assert!(parsed.imports.iter().any(|i| i.path == "SwiftHelper"));
}

#[test]
fn test_phase_d_fixtures_graph_construction() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/phase_d");
    let graph = build_graph_from_repo(&fixture_dir).expect("build graph from phase_d fixtures");

    // Check Zig symbols
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "User" && n.file.ends_with(".zig"))
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "runMain" && n.file.ends_with(".zig"))
    );

    // Check PHP symbols
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "SampleService" && n.file.ends_with(".php"))
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "ProcessorInterface" && n.file.ends_with(".php"))
    );

    // Check Ruby symbols
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "Analytics" && n.file.ends_with(".rb"))
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "DataProcessor" && n.file.ends_with(".rb"))
    );

    // Check Swift symbols
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "SampleService" && n.file.ends_with(".swift"))
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|n| n.name == "ServiceProtocol" && n.file.ends_with(".swift"))
    );

    // Check cross-symbol calls or contains
    assert!(graph.edges.iter().any(|e| e.kind == EdgeKind::Contains));
}

#[test]
fn test_parse_sample_fixtures_all_phase_d() {
    let zig_code = include_str!("fixtures/phase_d/sample.zig");
    let zig_parsed = parse_file("sample.zig", Language::Zig, zig_code);
    assert!(
        zig_parsed
            .symbols
            .iter()
            .any(|s| s.name == "User" && s.kind == NodeKind::Struct)
    );
    assert!(
        zig_parsed
            .symbols
            .iter()
            .any(|s| s.name == "Status" && s.kind == NodeKind::Struct)
    );
    assert!(
        zig_parsed
            .symbols
            .iter()
            .any(|s| s.name == "init" && s.kind == NodeKind::Method)
    );
    assert!(
        zig_parsed
            .symbols
            .iter()
            .any(|s| s.name == "runMain" && s.kind == NodeKind::Function)
    );
    assert!(zig_parsed.imports.iter().any(|i| i.path == "helper.zig"));

    let php_code = include_str!("fixtures/phase_d/sample.php");
    let php_parsed = parse_file("sample.php", Language::Php, php_code);
    assert!(
        php_parsed
            .symbols
            .iter()
            .any(|s| s.name == "App\\Services" && s.kind == NodeKind::Module)
    );
    assert!(
        php_parsed
            .symbols
            .iter()
            .any(|s| s.name == "ProcessorInterface" && s.kind == NodeKind::Interface)
    );
    assert!(
        php_parsed
            .symbols
            .iter()
            .any(|s| s.name == "LoggerTrait" && s.kind == NodeKind::Trait)
    );
    assert!(
        php_parsed
            .symbols
            .iter()
            .any(|s| s.name == "SampleService" && s.kind == NodeKind::Class)
    );
    assert!(
        php_parsed
            .symbols
            .iter()
            .any(|s| s.name == "process" && s.kind == NodeKind::Method)
    );
    assert!(
        php_parsed
            .symbols
            .iter()
            .any(|s| s.name == "standaloneFunction" && s.kind == NodeKind::Function)
    );

    let rb_code = include_str!("fixtures/phase_d/sample.rb");
    let rb_parsed = parse_file("sample.rb", Language::Ruby, rb_code);
    assert!(
        rb_parsed
            .symbols
            .iter()
            .any(|s| s.name == "Analytics" && s.kind == NodeKind::Module)
    );
    assert!(
        rb_parsed
            .symbols
            .iter()
            .any(|s| s.name == "DataProcessor" && s.kind == NodeKind::Class)
    );
    assert!(
        rb_parsed
            .symbols
            .iter()
            .any(|s| s.name == "execute" && s.kind == NodeKind::Method)
    );
    assert!(
        rb_parsed
            .symbols
            .iter()
            .any(|s| s.name == "top_level_entry" && s.kind == NodeKind::Function)
    );

    let swift_code = include_str!("fixtures/phase_d/sample.swift");
    let swift_parsed = parse_file("sample.swift", Language::Swift, swift_code);
    assert!(
        swift_parsed
            .symbols
            .iter()
            .any(|s| s.name == "ServiceProtocol" && s.kind == NodeKind::Interface)
    );
    assert!(
        swift_parsed
            .symbols
            .iter()
            .any(|s| s.name == "Config" && s.kind == NodeKind::Struct)
    );
    assert!(
        swift_parsed
            .symbols
            .iter()
            .any(|s| s.name == "SampleService" && s.kind == NodeKind::Class)
    );
    assert!(
        swift_parsed
            .symbols
            .iter()
            .any(|s| s.name == "start" && s.kind == NodeKind::Method)
    );
    assert!(
        swift_parsed
            .symbols
            .iter()
            .any(|s| s.name == "globalBootstrap" && s.kind == NodeKind::Function)
    );
}

#[test]
fn test_malformed_phase_d_files_resilience() {
    let zig_malformed = "pub fn broken( const x = @import(";
    let parsed_zig = parse_file("malformed.zig", Language::Zig, zig_malformed);
    // Shouldn't panic or crash
    let _ = parsed_zig;

    let php_malformed = "<?php namespace Unclosed { class Broken { public function oops(";
    let parsed_php = parse_file("malformed.php", Language::Php, php_malformed);
    let _ = parsed_php;

    let rb_malformed = "module Broken\n  class Incomplete\n    def unclosed(";
    let parsed_rb = parse_file("malformed.rb", Language::Ruby, rb_malformed);
    let _ = parsed_rb;

    let swift_malformed = "import Foundation\nclass BrokenSwift {\n    func unclosed(";
    let parsed_swift = parse_file("malformed.swift", Language::Swift, swift_malformed);
    let _ = parsed_swift;
}
