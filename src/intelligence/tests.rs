use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::{EdgeKind, Node, NodeId, NodeKind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredTest {
    pub test_file: String,
    pub test_symbol: Option<String>,
    pub test_symbol_id: Option<NodeId>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTestMapping {
    pub source_file: String,
    pub source_symbol: Option<String>,
    pub tests: Vec<DiscoveredTest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestDiscoveryReport {
    pub total_tests: usize,
    pub mappings: Vec<SourceTestMapping>,
}

#[must_use]
pub fn is_test_file(path: &str) -> bool {
    let normalized = path.replace('\\', "/").to_lowercase();
    normalized.starts_with("tests/")
        || normalized.starts_with("test/")
        || normalized.contains("/tests/")
        || normalized.contains("/test/")
        || normalized.ends_with("_test.rs")
        || normalized.ends_with("_test.go")
        || normalized.ends_with("_test.py")
        || normalized.ends_with(".test.ts")
        || normalized.ends_with(".test.js")
        || normalized.ends_with(".test.tsx")
        || normalized.ends_with(".test.jsx")
        || normalized.ends_with(".spec.ts")
        || normalized.ends_with(".spec.js")
        || normalized.ends_with(".spec.tsx")
        || normalized.ends_with(".spec.jsx")
        || normalized.ends_with("test.java")
        || normalized.ends_with("tests.java")
        || normalized.ends_with("test.cs")
        || normalized.ends_with("tests.cs")
        || normalized.ends_with("test.kt")
        || normalized.ends_with("test.zig")
        || normalized.ends_with("test.php")
        || normalized.ends_with("_test.rb")
        || normalized.ends_with("tests.swift")
}

#[must_use]
pub fn is_test_symbol(node: &Node) -> bool {
    if is_test_file(&node.file) {
        return true;
    }
    let name_lower = node.name.to_lowercase();
    name_lower.starts_with("test_")
        || name_lower.ends_with("_test")
        || name_lower.starts_with("test")
        || name_lower.starts_with("it_")
        || name_lower.starts_with("should_")
}

#[must_use]
pub fn discover_tests(graph: &Graph) -> TestDiscoveryReport {
    let mut mappings_map: BTreeMap<(String, Option<String>), Vec<DiscoveredTest>> = BTreeMap::new();

    // Strategy 1: Call graph / Reference linking (Test calls Source)
    for edge in &graph.edges {
        if edge.kind == EdgeKind::Calls || edge.kind == EdgeKind::Imports {
            let caller = graph.nodes.iter().find(|n| n.id == edge.from);
            let callee = graph.nodes.iter().find(|n| n.id == edge.to);

            if let (Some(caller_node), Some(callee_node)) = (caller, callee)
                && (is_test_symbol(caller_node) || is_test_file(&caller_node.file))
            {
                // Test caller calls target callee
                let reason = format!(
                    "test {} {} target {}",
                    caller_node.name,
                    edge.kind.as_str().to_lowercase(),
                    callee_node.name
                );

                let key = (
                    callee_node.file.clone(),
                    Some(callee_node.qualified_name.clone()),
                );
                mappings_map.entry(key).or_default().push(DiscoveredTest {
                    test_file: caller_node.file.clone(),
                    test_symbol: Some(caller_node.qualified_name.clone()),
                    test_symbol_id: Some(caller_node.id),
                    reason,
                });
            }
        }
    }

    // Strategy 2: Naming conventions & Test directory hierarchy
    let test_files: Vec<&Node> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::File && is_test_file(&n.file))
        .collect();

    let source_files: Vec<&Node> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::File && !is_test_file(&n.file))
        .collect();

    for src in &source_files {
        let src_stem = extract_stem(&src.file);
        for tst in &test_files {
            let tst_stem = extract_stem(&tst.file);
            if tst_stem.contains(&src_stem) || src_stem.contains(&tst_stem) {
                let reason = format!(
                    "matching naming convention between {} and {}",
                    src.file, tst.file
                );
                let key = (src.file.clone(), None);
                mappings_map.entry(key).or_default().push(DiscoveredTest {
                    test_file: tst.file.clone(),
                    test_symbol: None,
                    test_symbol_id: None,
                    reason,
                });
            }
        }
    }

    let mut total_tests = 0;
    let mut mappings = Vec::new();

    for ((source_file, source_symbol), mut tests) in mappings_map {
        tests.sort_by(|a, b| {
            a.test_file
                .cmp(&b.test_file)
                .then_with(|| a.test_symbol.cmp(&b.test_symbol))
        });
        tests.dedup_by(|a, b| a.test_file == b.test_file && a.test_symbol == b.test_symbol);
        total_tests += tests.len();
        mappings.push(SourceTestMapping {
            source_file,
            source_symbol,
            tests,
        });
    }

    mappings.sort_by(|a, b| {
        a.source_file
            .cmp(&b.source_file)
            .then_with(|| a.source_symbol.cmp(&b.source_symbol))
    });

    TestDiscoveryReport {
        total_tests,
        mappings,
    }
}

#[must_use]
pub fn map_source_to_tests(graph: &Graph, source_path_or_symbol: &str) -> Vec<DiscoveredTest> {
    let report = discover_tests(graph);
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for mapping in report.mappings {
        let matches_exact_file = mapping.source_file == source_path_or_symbol;
        let matches_exact_symbol = mapping.source_symbol.as_deref() == Some(source_path_or_symbol);
        let matches_symbol_suffix = mapping
            .source_symbol
            .as_deref()
            .is_some_and(|sym| sym.ends_with(&format!("::{source_path_or_symbol}")));
        let matches_file_contains = mapping.source_file.contains(source_path_or_symbol);

        if matches_exact_file
            || matches_exact_symbol
            || matches_symbol_suffix
            || matches_file_contains
        {
            for test in mapping.tests {
                let key = (test.test_file.clone(), test.test_symbol.clone());
                if seen.insert(key) {
                    results.push(test);
                }
            }
        }
    }

    results
}

fn extract_stem(file_path: &str) -> String {
    let normalized = file_path.replace('\\', "/");
    let filename = normalized.split('/').next_back().unwrap_or(&normalized);
    let stem = filename
        .split('.')
        .next()
        .unwrap_or(filename)
        .trim_end_matches("_test")
        .trim_end_matches(".test")
        .trim_end_matches(".spec")
        .trim_start_matches("test_");
    stem.to_lowercase()
}
