use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::context::slice::extract_source_slice;
use crate::graph::Graph;
use crate::intelligence::impact::analyze_impact_with_cancel;
use crate::intelligence::neighborhood::{NeighborhoodOptions, get_neighborhood_with_cancel};
use crate::intelligence::tests::{DiscoveredTest, map_source_to_tests};
use crate::model::Node;
use crate::query::QueryIndex;

/// Aggregated exploration result providing unified 1-call context for AI coding agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExploreResult {
    pub target: Node,
    pub source_code: Option<String>,
    pub container: Option<Node>,
    pub callers: Vec<Node>,
    pub callees: Vec<Node>,
    pub direct_impact_count: usize,
    pub transitive_impact_count: usize,
    pub impacted_files: Vec<String>,
    pub related_tests: Vec<DiscoveredTest>,
}

/// Unified exploration of a symbol: extracts definition, source code, call relationships,
/// blast radius, and relevant tests in a single operation.
#[must_use]
pub fn explore_symbol(
    graph: &Graph,
    target_query: &str,
    depth: usize,
    repo_root: Option<&Path>,
) -> Option<ExploreResult> {
    explore_symbol_with_cancel(graph, target_query, depth, repo_root, None)
}

/// Unified exploration with cancellation support.
pub fn explore_symbol_with_cancel(
    graph: &Graph,
    target_query: &str,
    depth: usize,
    repo_root: Option<&Path>,
    cancelled: Option<&dyn Fn() -> bool>,
) -> Option<ExploreResult> {
    if let Some(is_cancelled) = cancelled
        && is_cancelled()
    {
        return None;
    }

    let index = QueryIndex::new(graph);
    let matches = index.find(graph, target_query);
    if matches.is_empty() {
        return None;
    }

    // Prefer exact qualified name, then exact name, then first match
    let target = matches
        .iter()
        .find(|n| n.qualified_name == target_query)
        .or_else(|| matches.iter().find(|n| n.name == target_query))
        .copied()
        .unwrap_or(matches[0])
        .clone();

    if let Some(is_cancelled) = cancelled
        && is_cancelled()
    {
        return None;
    }

    // 1. Source code slice
    let source_code = extract_source_slice(repo_root, &target.location)
        .ok()
        .map(|s| s.content)
        .filter(|c| !c.is_empty());

    if let Some(is_cancelled) = cancelled
        && is_cancelled()
    {
        return None;
    }

    // 2. Structural callers, callees, container
    let neighborhood_opts = NeighborhoodOptions {
        target: target.qualified_name.clone(),
        depth: 1,
        limit: 50,
    };
    let neighborhood = get_neighborhood_with_cancel(graph, &neighborhood_opts, cancelled);
    let container = neighborhood.as_ref().and_then(|n| n.container.clone());
    let callers = neighborhood
        .as_ref()
        .map(|n| n.callers.clone())
        .unwrap_or_default();
    let callees = neighborhood
        .as_ref()
        .map(|n| n.callees.clone())
        .unwrap_or_default();

    if let Some(is_cancelled) = cancelled
        && is_cancelled()
    {
        return None;
    }

    // 3. Blast radius / Impact analysis
    let impact = analyze_impact_with_cancel(graph, &target.qualified_name, depth, cancelled);
    let (direct_impact_count, transitive_impact_count, impacted_files) = if let Some(imp) = impact {
        (imp.direct_count, imp.transitive_count, imp.impacted_files)
    } else {
        (0, 0, Vec::new())
    };

    if let Some(is_cancelled) = cancelled
        && is_cancelled()
    {
        return None;
    }

    // 4. Test discovery
    let mut related_tests = map_source_to_tests(graph, &target.qualified_name);
    if related_tests.is_empty() {
        related_tests = map_source_to_tests(graph, &target.location.file);
    }

    Some(ExploreResult {
        target,
        source_code,
        container,
        callers,
        callees,
        direct_impact_count,
        transitive_impact_count,
        impacted_files,
        related_tests,
    })
}

/// Format `ExploreResult` into high-density Markdown for AI agents and CLI.
#[must_use]
pub fn format_explore_markdown(result: &ExploreResult) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    let lang_tag = match result.target.language {
        Some(crate::model::Language::Rust) => "rust",
        Some(crate::model::Language::Python) => "python",
        Some(crate::model::Language::TypeScript) => "typescript",
        Some(crate::model::Language::JavaScript) => "javascript",
        Some(crate::model::Language::Tsx) => "tsx",
        Some(crate::model::Language::Jsx) => "jsx",
        Some(crate::model::Language::Go) => "go",
        Some(crate::model::Language::C) => "c",
        Some(crate::model::Language::Cpp) => "cpp",
        Some(crate::model::Language::Java) => "java",
        Some(crate::model::Language::CSharp) => "csharp",
        Some(crate::model::Language::Kotlin) => "kotlin",
        Some(crate::model::Language::Zig) => "zig",
        Some(crate::model::Language::Php) => "php",
        Some(crate::model::Language::Ruby) => "ruby",
        Some(crate::model::Language::Swift) => "swift",
        None => "",
    };

    let _ = writeln!(
        out,
        "### [{}] {}",
        result.target.kind.as_str(),
        result.target.qualified_name
    );
    let _ = writeln!(
        out,
        "- **Location**: `{}:{}:{}` to `{}:{}`",
        result.target.location.file,
        result.target.location.start_line,
        result.target.location.start_col,
        result.target.location.end_line,
        result.target.location.end_col
    );

    if let Some(container) = &result.container {
        let _ = writeln!(
            out,
            "- **Container**: [{}] `{}`",
            container.kind.as_str(),
            container.qualified_name
        );
    }

    if let Some(src) = &result.source_code {
        let _ = writeln!(out, "\n```{lang_tag}\n{src}\n```");
    }

    // Call relationships
    let _ = writeln!(out, "\n#### Call Hierarchy");
    if result.callers.is_empty() {
        let _ = writeln!(out, "- **Callers (0)**: none");
    } else {
        let _ = writeln!(out, "- **Callers ({})**:", result.callers.len());
        for c in &result.callers {
            let _ = writeln!(
                out,
                "  - [{}] `{}` (`{}:{}`)",
                c.kind.as_str(),
                c.qualified_name,
                c.location.file,
                c.location.start_line
            );
        }
    }

    if result.callees.is_empty() {
        let _ = writeln!(out, "- **Callees (0)**: none");
    } else {
        let _ = writeln!(out, "- **Callees ({})**:", result.callees.len());
        for c in &result.callees {
            let _ = writeln!(
                out,
                "  - [{}] `{}` (`{}:{}`)",
                c.kind.as_str(),
                c.qualified_name,
                c.location.file,
                c.location.start_line
            );
        }
    }

    // Blast radius
    let _ = writeln!(out, "\n#### Blast Radius / Impact");
    let _ = writeln!(
        out,
        "- **Direct Dependents**: {}",
        result.direct_impact_count
    );
    let _ = writeln!(
        out,
        "- **Transitive Dependents**: {}",
        result.transitive_impact_count
    );
    if !result.impacted_files.is_empty() {
        let _ = writeln!(
            out,
            "- **Impacted Files ({})**: {}",
            result.impacted_files.len(),
            result.impacted_files.join(", ")
        );
    }

    // Tests
    let _ = writeln!(out, "\n#### Related Tests");
    if result.related_tests.is_empty() {
        let _ = writeln!(out, "- None discovered");
    } else {
        for t in &result.related_tests {
            if let Some(sym) = &t.test_symbol {
                let _ = writeln!(out, "- `{sym}` in `{}` ({})", t.test_file, t.reason);
            } else {
                let _ = writeln!(out, "- `{}` ({})", t.test_file, t.reason);
            }
        }
    }

    out
}
