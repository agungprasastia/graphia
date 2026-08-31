use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::model::{Language, SourceLocation};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalBinding {
    pub name: String,
    pub type_name: Option<String>,
    pub location: SourceLocation,
    pub scope_id: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParameterFlow {
    pub name: String,
    pub type_name: Option<String>,
    pub location: SourceLocation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentFlow {
    pub from: String,
    pub to: String,
    pub location: SourceLocation,
    pub scope_id: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnFlow {
    pub source: String,
    pub location: SourceLocation,
    pub scope_id: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallArgumentFlow {
    pub call: String,
    pub argument: String,
    pub index: usize,
    pub location: SourceLocation,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LocalFlowGraph {
    pub procedure_name: String,
    pub file: String,
    pub bindings: Vec<LocalBinding>,
    pub parameters: Vec<ParameterFlow>,
    pub assignments: Vec<AssignmentFlow>,
    pub returns: Vec<ReturnFlow>,
    pub call_arguments: Vec<CallArgumentFlow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssignmentEdge {
    pub from_var: String,
    pub to_var: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProceduralTypeFlow {
    pub procedure_name: String,
    pub file: String,
    pub assignments: Vec<AssignmentEdge>,
    pub parameter_flows: Vec<String>,
    pub return_sources: Vec<String>,
}

#[must_use]
pub fn extract_local_flow_graph(
    procedure_name: &str,
    file: &str,
    source: &str,
    start_line: u32,
) -> LocalFlowGraph {
    let Some(language) = language_for_path(file) else {
        return fallback_local_flow(procedure_name, file, source, start_line);
    };
    let mut parser = Parser::new();
    if parser
        .set_language(&crate::parser::ts_language(language))
        .is_err()
    {
        return fallback_local_flow(procedure_name, file, source, start_line);
    }
    let Some(tree) = parser.parse(source, None) else {
        return fallback_local_flow(procedure_name, file, source, start_line);
    };
    let mut flow = LocalFlowGraph {
        procedure_name: procedure_name.into(),
        file: file.into(),
        ..Default::default()
    };
    collect_local_ast(
        tree.root_node(),
        source.as_bytes(),
        file,
        start_line,
        &mut flow,
        0,
    );
    flow
}

#[must_use]
pub fn extract_intraprocedural_typeflow(
    procedure_name: &str,
    file: &str,
    body_src: &str,
    start_line: u32,
) -> ProceduralTypeFlow {
    let graph = extract_local_flow_graph(procedure_name, file, body_src, start_line);
    ProceduralTypeFlow {
        procedure_name: graph.procedure_name,
        file: graph.file,
        assignments: graph
            .assignments
            .iter()
            .map(|a| AssignmentEdge {
                from_var: a.from.clone(),
                to_var: a.to.clone(),
                location: a.location.clone(),
            })
            .collect(),
        parameter_flows: graph.parameters.iter().map(|p| p.name.clone()).collect(),
        return_sources: graph.returns.iter().map(|r| r.source.clone()).collect(),
    }
}

fn collect_local_ast(
    node: Node<'_>,
    source: &[u8],
    file: &str,
    start_line: u32,
    flow: &mut LocalFlowGraph,
    scope_id: usize,
) {
    let location = ast_location(file, &node, start_line);
    if matches!(
        node.kind(),
        "parameters" | "formal_parameters" | "parameter_list"
    ) {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if let Some(name) =
                identifier_from_node(child.child_by_field_name("name").unwrap_or(child), source)
            {
                let type_name = child
                    .child_by_field_name("type")
                    .and_then(|n| n.utf8_text(source).ok())
                    .map(str::to_string);
                flow.parameters.push(ParameterFlow {
                    name,
                    type_name,
                    location: ast_location(file, &child, start_line),
                });
            }
        }
    }
    if matches!(node.kind(), "let_declaration" | "variable_declarator")
        && let Some(name_node) = node
            .child_by_field_name("name")
            .or_else(|| node.child_by_field_name("pattern"))
        && let Some(name) = identifier_from_node(name_node, source)
    {
        let type_name = node
            .child_by_field_name("type")
            .and_then(|n| n.utf8_text(source).ok())
            .map(str::to_string);
        flow.bindings.push(LocalBinding {
            name: name.clone(),
            type_name,
            location: location.clone(),
            scope_id,
        });
        if let Some(value) = node.child_by_field_name("value") {
            add_local_assignment(value, &name, &location, scope_id, source, flow);
        }
    }
    if matches!(node.kind(), "assignment_expression" | "assignment")
        && let (Some(left), Some(right)) = (
            node.child_by_field_name("left"),
            node.child_by_field_name("right"),
        )
        && let Some(to) = identifier_from_node(left, source)
    {
        add_local_assignment(right, &to, &location, scope_id, source, flow);
    }
    if matches!(
        node.kind(),
        "return_statement" | "return" | "return_expression"
    ) && let Some(value) = node.named_children(&mut node.walk()).next()
        && let Some(name) = identifier_from_node(value, source)
    {
        flow.returns.push(ReturnFlow {
            source: name,
            location: location.clone(),
            scope_id,
        });
    }
    if matches!(
        node.kind(),
        "call" | "call_expression" | "method_call_expression"
    ) && let Some(arguments) = node.child_by_field_name("arguments")
    {
        let call = node
            .child_by_field_name("function")
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("")
            .rsplit(['.', ':'])
            .next()
            .unwrap_or("")
            .to_string();
        let mut cursor = arguments.walk();
        for (index, argument) in arguments.named_children(&mut cursor).enumerate() {
            if let Some(value) = identifier_from_node(argument, source) {
                flow.call_arguments.push(CallArgumentFlow {
                    call: call.clone(),
                    argument: value,
                    index,
                    location: ast_location(file, &argument, start_line),
                });
            }
        }
    }
    let next_scope = if matches!(
        node.kind(),
        "block" | "statement_block" | "compound_statement"
    ) {
        scope_id + 1
    } else {
        scope_id
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_local_ast(child, source, file, start_line, flow, next_scope);
    }
}

fn add_local_assignment(
    value: Node<'_>,
    to: &str,
    location: &SourceLocation,
    scope_id: usize,
    source: &[u8],
    flow: &mut LocalFlowGraph,
) {
    if let Some(from) = identifier_from_node(value, source)
        .or_else(|| value.utf8_text(source).ok().map(str::to_string))
    {
        flow.assignments.push(AssignmentFlow {
            from,
            to: to.into(),
            location: location.clone(),
            scope_id,
        });
    }
}
fn identifier_from_node(node: Node<'_>, source: &[u8]) -> Option<String> {
    if matches!(
        node.kind(),
        "identifier"
            | "field_identifier"
            | "type_identifier"
            | "shorthand_property_identifier_pattern"
    ) {
        return node.utf8_text(source).ok().map(str::to_string);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| identifier_from_node(child, source))
}

fn fallback_local_flow(
    procedure_name: &str,
    file: &str,
    source: &str,
    start_line: u32,
) -> LocalFlowGraph {
    let mut flow = LocalFlowGraph {
        procedure_name: procedure_name.into(),
        file: file.into(),
        ..Default::default()
    };
    for (offset, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        let location = SourceLocation {
            file: file.into(),
            start_line: start_line + offset as u32,
            start_col: 1,
            end_line: start_line + offset as u32,
            end_col: line.len() as u32,
        };
        if let Some((to, from)) = trimmed.split_once('=') {
            flow.assignments.push(AssignmentFlow {
                from: from.trim().trim_end_matches(';').into(),
                to: to
                    .trim()
                    .trim_start_matches("let ")
                    .trim_start_matches("var ")
                    .trim_start_matches("const ")
                    .into(),
                location,
                scope_id: 0,
            });
        }
    }
    flow
}

fn ast_location(file: &str, node: &Node<'_>, start_line: u32) -> SourceLocation {
    let start = node.start_position();
    let end = node.end_position();
    SourceLocation {
        file: file.to_string(),
        start_line: start_line.saturating_add(start.row as u32),
        start_col: start.column as u32 + 1,
        end_line: start_line.saturating_add(end.row as u32),
        end_col: end.column as u32 + 1,
    }
}

fn language_for_path(path: &str) -> Option<Language> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    match extension.as_str() {
        "rs" => Some(Language::Rust),
        "py" => Some(Language::Python),
        "ts" | "mts" | "cts" => Some(Language::TypeScript),
        "tsx" => Some(Language::Tsx),
        "js" | "mjs" | "cjs" => Some(Language::JavaScript),
        "jsx" => Some(Language::Jsx),
        "go" => Some(Language::Go),
        "c" | "h" => Some(Language::C),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" | "hh" => Some(Language::Cpp),
        "java" => Some(Language::Java),
        "cs" => Some(Language::CSharp),
        "kt" | "kts" => Some(Language::Kotlin),
        "zig" => Some(Language::Zig),
        "php" | "phtml" => Some(Language::Php),
        "rb" | "erb" => Some(Language::Ruby),
        "swift" => Some(Language::Swift),
        _ => None,
    }
}
