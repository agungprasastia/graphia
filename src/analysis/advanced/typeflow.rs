use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::model::{Language, SourceLocation};

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
pub fn extract_intraprocedural_typeflow(
    procedure_name: &str,
    file: &str,
    body_src: &str,
    start_line: u32,
) -> ProceduralTypeFlow {
    if let Some(ast_flow) = extract_ast_typeflow(procedure_name, file, body_src, start_line) {
        return ast_flow;
    }

    // Explicit fallback for an unsupported file extension or malformed source.
    let mut assignments = Vec::new();
    let mut return_sources = Vec::new();
    let mut parameter_flows = Vec::new();

    if let Some(param_start) = body_src.find('(') {
        if let Some(param_end) = body_src[param_start..].find(')') {
            let params_text = &body_src[param_start + 1..param_start + param_end];
            for p in params_text.split(',') {
                let p_trim = p.trim();
                if !p_trim.is_empty() {
                    let param_name = p_trim.split([':', ' ']).next().unwrap_or("").trim();
                    if !param_name.is_empty()
                        && param_name != "self"
                        && param_name != "&self"
                        && param_name != "&mut self"
                    {
                        parameter_flows.push(param_name.to_string());
                    }
                }
            }
        }
    }

    for (line_idx, line) in body_src.lines().enumerate() {
        let current_line = start_line + line_idx as u32;
        let trimmed = line.trim();

        if trimmed.starts_with("return ") || trimmed.starts_with("return;") {
            let expr = trimmed
                .trim_start_matches("return")
                .trim()
                .trim_end_matches(';');
            if !expr.is_empty() {
                return_sources.push(expr.to_string());
            }
        }

        if let Some((left, right)) = parse_assignment_line(trimmed) {
            assignments.push(AssignmentEdge {
                from_var: right,
                to_var: left,
                location: SourceLocation {
                    file: file.to_string(),
                    start_line: current_line,
                    start_col: 1,
                    end_line: current_line,
                    end_col: line.len() as u32,
                },
            });
        }
    }

    ProceduralTypeFlow {
        procedure_name: procedure_name.to_string(),
        file: file.to_string(),
        assignments,
        parameter_flows,
        return_sources,
    }
}

fn extract_ast_typeflow(
    procedure_name: &str,
    file: &str,
    source: &str,
    start_line: u32,
) -> Option<ProceduralTypeFlow> {
    let language = language_for_path(file)?;
    let ts_language = crate::parser::ts_language(language);
    let mut parser = Parser::new();
    parser.set_language(&ts_language).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();
    let mut assignments = Vec::new();
    let mut parameter_flows = Vec::new();
    let mut return_sources = Vec::new();
    collect_ast_flow(
        root,
        source.as_bytes(),
        file,
        start_line,
        &mut assignments,
        &mut parameter_flows,
        &mut return_sources,
    );
    Some(ProceduralTypeFlow {
        procedure_name: procedure_name.to_string(),
        file: file.to_string(),
        assignments,
        parameter_flows,
        return_sources,
    })
}

fn collect_ast_flow(
    node: Node<'_>,
    source: &[u8],
    file: &str,
    start_line: u32,
    assignments: &mut Vec<AssignmentEdge>,
    parameters: &mut Vec<String>,
    returns: &mut Vec<String>,
) {
    let kind = node.kind();
    if kind == "parameters" || kind == "formal_parameters" || kind == "parameter_list" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            let name = child
                .child_by_field_name("name")
                .or_else(|| first_identifier(child))
                .map(|name| name.utf8_text(source).unwrap_or("").to_string());
            if let Some(name) = name.filter(|name| !name.is_empty() && name != "self") {
                if !parameters.contains(&name) {
                    parameters.push(name);
                }
            }
        }
    }

    if matches!(
        kind,
        "assignment_expression" | "assignment" | "short_var_declaration"
    ) {
        if let (Some(left), Some(right)) = (
            node.child_by_field_name("left")
                .or_else(|| node.child_by_field_name("name")),
            node.child_by_field_name("right")
                .or_else(|| node.child_by_field_name("value")),
        ) {
            let left = identifier_text(left, source);
            let right = identifier_text(right, source);
            if let (Some(to_var), Some(from_var)) = (left, right) {
                assignments.push(AssignmentEdge {
                    from_var,
                    to_var,
                    location: ast_location(file, &node, start_line),
                });
            }
        }
    }

    if kind == "variable_declarator" {
        if let (Some(name), Some(value)) = (
            node.child_by_field_name("name"),
            node.child_by_field_name("value"),
        ) {
            if let (Some(to_var), Some(from_var)) = (
                identifier_text(name, source),
                identifier_text(value, source),
            ) {
                assignments.push(AssignmentEdge {
                    from_var,
                    to_var,
                    location: ast_location(file, &node, start_line),
                });
            }
        }
    }

    if kind == "return_statement" {
        let mut cursor = node.walk();
        if let Some(value) = node.named_children(&mut cursor).next()
            && let Some(name) = identifier_text(value, source)
            && !returns.contains(&name)
        {
            returns.push(name);
        }
    }

    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_ast_flow(
            child,
            source,
            file,
            start_line,
            assignments,
            parameters,
            returns,
        );
    }
}

fn first_identifier(node: Node<'_>) -> Option<Node<'_>> {
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier"
    ) {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor).find_map(first_identifier)
}

fn identifier_text(node: Node<'_>, source: &[u8]) -> Option<String> {
    let node = first_identifier(node)?;
    let text = node.utf8_text(source).ok()?.trim();
    (!text.is_empty()).then(|| text.to_string())
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

fn parse_assignment_line(line: &str) -> Option<(String, String)> {
    let stripped = line
        .trim_start_matches("let ")
        .trim_start_matches("mut ")
        .trim_start_matches("var ")
        .trim_start_matches("val ")
        .trim_start_matches("const ");

    if let Some((left_raw, right_raw)) = stripped.split_once(":=") {
        let left = left_raw.trim().to_string();
        let right = right_raw.trim().trim_end_matches(';').to_string();
        if !left.is_empty() && !right.is_empty() {
            return Some((left, right));
        }
    }

    if let Some((left_raw, right_raw)) = stripped.split_once('=') {
        if !left_raw.ends_with('!')
            && !left_raw.ends_with('<')
            && !left_raw.ends_with('>')
            && !left_raw.ends_with('=')
            && !right_raw.starts_with('=')
        {
            let left_part = left_raw.split(':').next().unwrap_or(left_raw).trim();
            let right_part = right_raw.trim().trim_end_matches(';').trim();
            if !left_part.is_empty() && !right_part.is_empty() && !left_part.contains(' ') {
                return Some((left_part.to_string(), right_part.to_string()));
            }
        }
    }

    None
}
