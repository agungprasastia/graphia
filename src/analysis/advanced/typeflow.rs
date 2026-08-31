use serde::{Deserialize, Serialize};

use crate::model::SourceLocation;

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
