use std::path::Path;

use serde_json::json;

use super::error::{McpError, Result};
use super::protocol::{CallToolResult, CancellationToken, Tool};
use crate::context::{BudgetValueType, ContextRequest};
use crate::graph::Graph;
use crate::intelligence::{
    NeighborhoodOptions, SearchOptions, analyze_impact_with_cancel, discover_tests,
    get_architecture_overview, get_neighborhood, get_neighborhood_with_cancel, map_source_to_tests,
    search_graph,
};
use crate::model::{Node, NodeKind};
use crate::query::{QueryIndex, TraversalLimits};

pub const MAX_RESULTS: usize = 500;
pub const MAX_DEPTH: usize = 20;
pub const MAX_CONTEXT_BUDGET: usize = 100_000;

fn parse_checked_limit(
    args: &serde_json::Map<String, serde_json::Value>,
    default: usize,
) -> Result<usize> {
    if let Some(val) = args.get("limit") {
        let n = val.as_u64().ok_or_else(|| {
            McpError::InvalidParams("Argument 'limit' must be an integer".to_string())
        })?;
        let n_usize = usize::try_from(n).map_err(|_| {
            McpError::InvalidParams("Argument 'limit' exceeds platform limits".to_string())
        })?;
        if n_usize > MAX_RESULTS {
            return Err(McpError::InvalidParams(format!(
                "Argument 'limit' exceeds server maximum {MAX_RESULTS}"
            )));
        }
        Ok(n_usize)
    } else {
        Ok(default)
    }
}

fn parse_checked_depth(
    args: &serde_json::Map<String, serde_json::Value>,
    default: usize,
) -> Result<usize> {
    if let Some(val) = args.get("depth") {
        let n = val.as_u64().ok_or_else(|| {
            McpError::InvalidParams("Argument 'depth' must be an integer".to_string())
        })?;
        let n_usize = usize::try_from(n).map_err(|_| {
            McpError::InvalidParams("Argument 'depth' exceeds platform limits".to_string())
        })?;
        if n_usize > MAX_DEPTH {
            return Err(McpError::InvalidParams(format!(
                "Argument 'depth' exceeds server maximum {MAX_DEPTH}"
            )));
        }
        Ok(n_usize)
    } else {
        Ok(default)
    }
}
pub fn get_tool_definitions() -> Vec<Tool> {
    vec![
        Tool {
            name: "graphia_search_symbol".to_string(),
            description: Some("Search symbols across the code graph using structural scoring and filters.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Symbol name or substring query to search for"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Optional node kind filter (e.g. function, method, struct, class, trait, interface, module, file)"
                    },
                    "file": {
                        "type": "string",
                        "description": "Optional relative file path filter"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of search results to return (default: 20)"
                    }
                },
                "required": ["query"]
            }),
        },
        Tool {
            name: "graphia_get_symbol".to_string(),
            description: Some("Get detailed definition, source location, container, and relationships of a symbol.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Exact or qualified symbol name to look up"
                    }
                },
                "required": ["symbol"]
            }),
        },
        Tool {
            name: "graphia_find_callers".to_string(),
            description: Some("Find all inbound callers of a function or method.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Function or method symbol name"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Call graph traversal depth (default: 1)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum callers to return (default: 50)"
                    }
                },
                "required": ["symbol"]
            }),
        },
        Tool {
            name: "graphia_find_callees".to_string(),
            description: Some("Find all outbound function and method calls made by a symbol.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Function or method symbol name"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Call graph traversal depth (default: 1)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum callees to return (default: 50)"
                    }
                },
                "required": ["symbol"]
            }),
        },
        Tool {
            name: "graphia_find_references".to_string(),
            description: Some("Find all references to a symbol categorized by calls, types, and imports.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol name to find references for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum references per category to return (default: 50)"
                    }
                },
                "required": ["symbol"]
            }),
        },
        Tool {
            name: "graphia_dependency_path".to_string(),
            description: Some("Find the shortest structural dependency path between two symbols.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "from": {
                        "type": "string",
                        "description": "Starting symbol name"
                    },
                    "to": {
                        "type": "string",
                        "description": "Target symbol name"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum search depth (default: 50)"
                    }
                },
                "required": ["from", "to"]
            }),
        },
        Tool {
            name: "graphia_neighborhood".to_string(),
            description: Some("Get the structural neighborhood around a symbol (container, children, callers, callees, imports, implementations, tests).".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol name"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Hop depth for neighbors (default: 1)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum neighbors per relationship (default: 50)"
                    }
                },
                "required": ["symbol"]
            }),
        },
        Tool {
            name: "graphia_impact".to_string(),
            description: Some("Analyze change surface and blast radius when modifying a symbol.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol name to analyze impact for"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Transitive impact traversal depth (default: 3)"
                    }
                },
                "required": ["symbol"]
            }),
        },
        Tool {
            name: "graphia_find_tests".to_string(),
            description: Some("Discover tests covering a symbol or file using deterministic heuristic mapping.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Optional symbol name to find tests for"
                    },
                    "file": {
                        "type": "string",
                        "description": "Optional source file path to find tests for"
                    }
                }
            }),
        },
        Tool {
            name: "graphia_architecture".to_string(),
            description: Some("Get a high-level structural overview of the repository (totals, entrypoints, cycles, communities, high-centrality modules).".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        Tool {
            name: "graphia_context".to_string(),
            description: Some("Generate a token-budgeted minimal sufficient context bundle for a symbol, file, query, or changed files.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "symbol": {
                        "type": "string",
                        "description": "Symbol seed"
                    },
                    "file": {
                        "type": "string",
                        "description": "File seed"
                    },
                    "query": {
                        "type": "string",
                        "description": "Natural language or keyword query seed"
                    },
                    "changed": {
                        "type": "boolean",
                        "description": "Use uncommitted / modified files as seeds"
                    },
                    "token_budget": {
                        "type": "integer",
                        "description": "Maximum token/byte/char budget limit"
                    },
                    "budget_type": {
                        "type": "string",
                        "description": "Budget type: 'tokens', 'bytes', or 'chars' (default: 'tokens')"
                    },
                    "depth": {
                        "type": "integer",
                        "description": "Candidate expansion depth (default: 3)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Candidate count limit (default: 100)"
                    }
                }
            }),
        },
    ]
}

/// Dispatch and execute an MCP tool against the graph and repository root.
pub fn call_tool(
    graph: &Graph,
    repo_root: Option<&Path>,
    name: &str,
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Result<CallToolResult> {
    call_tool_with_cancellation(graph, repo_root, name, arguments, &CancellationToken::new())
}

pub fn call_tool_with_cancellation(
    graph: &Graph,
    repo_root: Option<&Path>,
    name: &str,
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
    token: &CancellationToken,
) -> Result<CallToolResult> {
    if token.is_cancelled() {
        return Err(McpError::Cancelled);
    }
    let empty_args = serde_json::Map::new();
    let args = arguments.unwrap_or(&empty_args);

    match name {
        "graphia_search_symbol" => tool_search_symbol(graph, args),
        "graphia_get_symbol" => tool_get_symbol(graph, args),
        "graphia_find_callers" => tool_find_callers(graph, args),
        "graphia_find_callees" => tool_find_callees(graph, args),
        "graphia_find_references" => tool_find_references(graph, args),
        "graphia_dependency_path" => tool_dependency_path(graph, args, token),
        "graphia_neighborhood" => tool_neighborhood(graph, args, token),
        "graphia_impact" => tool_impact(graph, args, token),
        "graphia_find_tests" => tool_find_tests(graph, args),
        "graphia_architecture" => tool_architecture(graph),
        "graphia_context" => tool_context(graph, repo_root, args, token),
        _ => Err(McpError::MethodNotFound(format!("Tool '{name}' not found"))),
    }
    .and_then(|result| {
        if token.is_cancelled() {
            Err(McpError::Cancelled)
        } else {
            Ok(result)
        }
    })
}

fn parse_node_kind(s: &str) -> Option<NodeKind> {
    match s.to_ascii_lowercase().as_str() {
        "file" => Some(NodeKind::File),
        "module" => Some(NodeKind::Module),
        "function" => Some(NodeKind::Function),
        "method" => Some(NodeKind::Method),
        "class" => Some(NodeKind::Class),
        "struct" => Some(NodeKind::Struct),
        "trait" => Some(NodeKind::Trait),
        "interface" => Some(NodeKind::Interface),
        _ => None,
    }
}

fn tool_search_symbol(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResult> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'query' argument".to_string()))?;

    let kind_filter = args
        .get("kind")
        .and_then(|v| v.as_str())
        .and_then(parse_node_kind);

    let file_filter = args
        .get("file")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let limit = if args.contains_key("limit") {
        Some(parse_checked_limit(args, 20)?)
    } else {
        None
    };

    let options = SearchOptions {
        query: query.to_string(),
        kind_filter,
        file_filter,
        limit,
    };

    let results = search_graph(graph, &options);
    let output = serde_json::to_string_pretty(&results)
        .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

fn tool_get_symbol(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResult> {
    let symbol = args
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'symbol' argument".to_string()))?;

    let index = QueryIndex::new(graph);
    let matches = index.find(graph, symbol);

    if matches.is_empty() {
        return Ok(CallToolResult::error(format!(
            "Symbol '{symbol}' not found"
        )));
    }

    let mut explanations = Vec::new();
    for node in matches {
        if let Ok(exp) = index.explain(graph, node.id) {
            let parent_node = exp
                .parent
                .and_then(|pid| graph.nodes.iter().find(|n| n.id == pid));
            let callers_nodes: Vec<_> = exp
                .callers
                .iter()
                .filter_map(|cid| graph.nodes.iter().find(|n| n.id == *cid))
                .collect();
            let callees_nodes: Vec<_> = exp
                .callees
                .iter()
                .filter_map(|cid| graph.nodes.iter().find(|n| n.id == *cid))
                .collect();
            let imports_nodes: Vec<_> = exp
                .imports
                .iter()
                .filter_map(|iid| graph.nodes.iter().find(|n| n.id == *iid))
                .collect();

            explanations.push(json!({
                "node": node,
                "location": exp.location,
                "parent": parent_node,
                "callers": callers_nodes,
                "callees": callees_nodes,
                "imports": imports_nodes,
            }));
        } else {
            explanations.push(json!({
                "node": node,
            }));
        }
    }

    let output = serde_json::to_string_pretty(&explanations)
        .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

fn tool_find_callers(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResult> {
    let symbol = args
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'symbol' argument".to_string()))?;

    let depth = parse_checked_depth(args, 1)?;
    let limit = parse_checked_limit(args, 50)?;

    let options = NeighborhoodOptions {
        target: symbol.to_string(),
        depth,
        limit,
    };

    let Some(neighborhood) = get_neighborhood(graph, &options) else {
        return Ok(CallToolResult::error(format!(
            "Symbol '{symbol}' not found"
        )));
    };

    let output = serde_json::to_string_pretty(&json!({
        "target": neighborhood.target,
        "callers": neighborhood.callers,
    }))
    .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

fn tool_find_callees(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResult> {
    let symbol = args
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'symbol' argument".to_string()))?;

    let depth = parse_checked_depth(args, 1)?;
    let limit = parse_checked_limit(args, 50)?;

    let options = NeighborhoodOptions {
        target: symbol.to_string(),
        depth,
        limit,
    };

    let Some(neighborhood) = get_neighborhood(graph, &options) else {
        return Ok(CallToolResult::error(format!(
            "Symbol '{symbol}' not found"
        )));
    };

    let output = serde_json::to_string_pretty(&json!({
        "target": neighborhood.target,
        "callees": neighborhood.callees,
    }))
    .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

fn tool_find_references(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResult> {
    let symbol = args
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'symbol' argument".to_string()))?;

    let limit = parse_checked_limit(args, 50)?;

    let options = NeighborhoodOptions {
        target: symbol.to_string(),
        depth: 1,
        limit,
    };

    let Some(neighborhood) = get_neighborhood(graph, &options) else {
        return Ok(CallToolResult::error(format!(
            "Symbol '{symbol}' not found"
        )));
    };

    let output = serde_json::to_string_pretty(&json!({
        "target": neighborhood.target,
        "calls": neighborhood.callers,
        "types": neighborhood.referenced_types,
        "imports": neighborhood.exports, // inbound imports
        "implementations": neighborhood.trait_implementations,
    }))
    .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

fn tool_dependency_path(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
    token: &CancellationToken,
) -> Result<CallToolResult> {
    let from = args
        .get("from")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'from' argument".to_string()))?;

    let to = args
        .get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'to' argument".to_string()))?;

    let max_depth = parse_checked_depth(args, 20)?;

    let index = QueryIndex::new(graph);
    let from_matches = index.find(graph, from);
    let to_matches = index.find(graph, to);

    if from_matches.is_empty() {
        return Ok(CallToolResult::error(format!(
            "Source symbol '{from}' not found"
        )));
    }
    if to_matches.is_empty() {
        return Ok(CallToolResult::error(format!(
            "Target symbol '{to}' not found"
        )));
    }

    let start_node = from_matches[0];
    let end_node = to_matches[0];

    match index.shortest_path_with_cancel(
        start_node.id,
        end_node.id,
        TraversalLimits::new(max_depth, 10_000),
        Some(&|| token.is_cancelled()),
    ) {
        Ok(Some(path_edges)) => {
            let mut steps: Vec<&Node> = vec![start_node];
            for edge_id in path_edges {
                if let Some(edge) = graph.edges.iter().find(|e| e.id == edge_id) {
                    if let Some(node) = graph.nodes.iter().find(|n| n.id == edge.to) {
                        steps.push(node);
                    }
                }
            }
            let output = serde_json::to_string_pretty(&json!({
                "found": true,
                "length": steps.len() - 1,
                "path": steps,
            }))
            .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;
            Ok(CallToolResult::text(output))
        }
        Ok(None) if token.is_cancelled() => Err(McpError::Cancelled),
        Ok(None) => {
            let output = serde_json::to_string_pretty(&json!({
                "found": false,
                "from": start_node,
                "to": end_node,
                "message": "No structural path found between symbols",
            }))
            .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;
            Ok(CallToolResult::text(output))
        }
        Err(traversal_err) => Ok(CallToolResult::error(format!(
            "Path traversal limit exceeded after {} visits (max limit: {})",
            traversal_err.visited, traversal_err.limit
        ))),
    }
}

fn tool_neighborhood(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
    token: &CancellationToken,
) -> Result<CallToolResult> {
    let symbol = args
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'symbol' argument".to_string()))?;

    let depth = parse_checked_depth(args, 1)?;
    let limit = parse_checked_limit(args, 50)?;

    let options = NeighborhoodOptions {
        target: symbol.to_string(),
        depth,
        limit,
    };

    let Some(neighborhood) =
        get_neighborhood_with_cancel(graph, &options, Some(&|| token.is_cancelled()))
    else {
        if token.is_cancelled() {
            return Err(McpError::Cancelled);
        }
        return Ok(CallToolResult::error(format!(
            "Symbol '{symbol}' not found"
        )));
    };

    let output = serde_json::to_string_pretty(&neighborhood)
        .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

fn tool_impact(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
    token: &CancellationToken,
) -> Result<CallToolResult> {
    let symbol = args
        .get("symbol")
        .and_then(|v| v.as_str())
        .ok_or_else(|| McpError::InvalidParams("Missing 'symbol' argument".to_string()))?;

    let depth = parse_checked_depth(args, 3)?;

    let Some(impact) =
        analyze_impact_with_cancel(graph, symbol, depth, Some(&|| token.is_cancelled()))
    else {
        if token.is_cancelled() {
            return Err(McpError::Cancelled);
        }
        return Ok(CallToolResult::error(format!(
            "Symbol '{symbol}' not found"
        )));
    };

    let output = serde_json::to_string_pretty(&impact)
        .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

fn tool_find_tests(
    graph: &Graph,
    args: &serde_json::Map<String, serde_json::Value>,
) -> Result<CallToolResult> {
    let symbol = args.get("symbol").and_then(|v| v.as_str());
    let file = args.get("file").and_then(|v| v.as_str());

    if let Some(target) = symbol.or(file) {
        let tests = map_source_to_tests(graph, target);
        let output = serde_json::to_string_pretty(&tests)
            .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;
        Ok(CallToolResult::text(output))
    } else {
        let report = discover_tests(graph);
        let output = serde_json::to_string_pretty(&report)
            .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;
        Ok(CallToolResult::text(output))
    }
}

fn tool_architecture(graph: &Graph) -> Result<CallToolResult> {
    let overview = get_architecture_overview(graph);
    let output = serde_json::to_string_pretty(&overview)
        .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;
    Ok(CallToolResult::text(output))
}

fn tool_context(
    graph: &Graph,
    repo_root: Option<&Path>,
    args: &serde_json::Map<String, serde_json::Value>,
    token: &CancellationToken,
) -> Result<CallToolResult> {
    let symbol = args
        .get("symbol")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let file = args
        .get("file")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let changed = args
        .get("changed")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let budget = if let Some(val) = args.get("token_budget").or_else(|| args.get("budget")) {
        let b = val
            .as_u64()
            .ok_or_else(|| McpError::InvalidParams("Budget must be an integer".to_string()))?;
        let b_usize = usize::try_from(b)
            .map_err(|_| McpError::InvalidParams("Budget exceeds platform limits".to_string()))?;
        if b_usize > MAX_CONTEXT_BUDGET {
            return Err(McpError::InvalidParams(format!(
                "Budget exceeds server maximum {MAX_CONTEXT_BUDGET}"
            )));
        }
        Some(b_usize)
    } else {
        None
    };

    let budget_type = args.get("budget_type").and_then(|v| v.as_str()).map_or(
        BudgetValueType::ApproxTokens,
        |s| match s {
            "bytes" => BudgetValueType::Bytes,
            "chars" | "characters" => BudgetValueType::Characters,
            _ => BudgetValueType::ApproxTokens,
        },
    );

    let max_depth = parse_checked_depth(args, 3)?;
    let max_candidates = parse_checked_limit(args, 100)?;

    let req = ContextRequest {
        symbol,
        file,
        query,
        changed,
        budget,
        budget_type,
        max_depth,
        max_candidates,
    };

    let bundle = crate::context::generate_context_with_cancel(
        graph,
        &req,
        repo_root,
        Some(&|| token.is_cancelled()),
    );
    let output = serde_json::to_string_pretty(&bundle)
        .map_err(|e| McpError::Internal(format!("Serialization error: {e}")))?;

    Ok(CallToolResult::text(output))
}

#[cfg(test)]
mod tests {
    use super::super::protocol::Content;
    use super::*;
    use crate::model::{Confidence, Edge, EdgeId, EdgeKind, NodeId};

    fn sample_node(id: u64, name: &str, kind: NodeKind) -> Node {
        Node {
            id: crate::model::NodeId(id),
            kind,
            name: name.to_string(),
            qualified_name: name.to_string(),
            file: "test.rs".to_string(),
            location: crate::model::SourceLocation {
                file: "test.rs".to_string(),
                start_line: 1,
                start_col: 1,
                end_line: 5,
                end_col: 1,
            },
            language: Some(crate::model::Language::Rust),
            visibility: crate::model::Visibility::Public,
            signature: None,
            container: None,
        }
    }

    fn sample_graph() -> Graph {
        let n1 = sample_node(1, "foo", NodeKind::Function);
        let n2 = sample_node(2, "bar", NodeKind::Function);
        let e1 = Edge {
            id: EdgeId(1),
            kind: EdgeKind::Calls,
            from: NodeId(1),
            to: NodeId(2),
            confidence: Confidence::Extracted,
            label: None,
        };
        Graph::new(vec![n1, n2], vec![e1])
    }

    #[test]
    fn tool_definitions_contains_all_11_tools() {
        let tools = get_tool_definitions();
        assert_eq!(tools.len(), 11);
        let names: Vec<_> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"graphia_search_symbol"));
        assert!(names.contains(&"graphia_get_symbol"));
        assert!(names.contains(&"graphia_find_callers"));
        assert!(names.contains(&"graphia_find_callees"));
        assert!(names.contains(&"graphia_find_references"));
        assert!(names.contains(&"graphia_dependency_path"));
        assert!(names.contains(&"graphia_neighborhood"));
        assert!(names.contains(&"graphia_impact"));
        assert!(names.contains(&"graphia_find_tests"));
        assert!(names.contains(&"graphia_architecture"));
        assert!(names.contains(&"graphia_context"));
    }

    #[test]
    fn test_search_symbol() {
        let graph = sample_graph();
        let mut args = serde_json::Map::new();
        args.insert("query".to_string(), json!("foo"));
        let res = call_tool(&graph, None, "graphia_search_symbol", Some(&args)).unwrap();
        assert_eq!(res.is_error, None);
        let Content::Text { text } = &res.content[0];
        assert!(text.contains("foo"));
    }

    #[test]
    fn test_find_callees_and_callers() {
        let graph = sample_graph();
        let mut args = serde_json::Map::new();
        args.insert("symbol".to_string(), json!("foo"));
        let res_callees = call_tool(&graph, None, "graphia_find_callees", Some(&args)).unwrap();
        let Content::Text { text } = &res_callees.content[0];
        assert!(text.contains("bar"));

        let mut args_bar = serde_json::Map::new();
        args_bar.insert("symbol".to_string(), json!("bar"));
        let res_callers = call_tool(&graph, None, "graphia_find_callers", Some(&args_bar)).unwrap();
        let Content::Text { text: callers_text } = &res_callers.content[0];
        assert!(callers_text.contains("foo"));
    }
}
