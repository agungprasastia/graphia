use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{Call, Import, ParsedFile, Symbol};

pub struct GoAnalyzer {
    language: GraphiaLanguage,
}

impl Default for GoAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl GoAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            language: GraphiaLanguage::Go,
        }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn parse_tree(&self, source: &[u8]) -> Option<Tree> {
        let mut parser = Parser::new();
        let ts_lang = self.ts_language();
        if let Err(error) = parser.set_language(&ts_lang) {
            eprintln!("set language failed: {error:?}");
            return None;
        }
        parser.parse(source, None)
    }
}

impl LanguageAnalyzer for GoAnalyzer {
    fn language(&self) -> GraphiaLanguage {
        self.language
    }

    fn analyze(&self, path: &str, source: &[u8]) -> Result<ParsedFile> {
        if std::str::from_utf8(source).is_err() {
            return Err(GraphiaError::Parse {
                file: path.to_string(),
                message: "invalid UTF-8".to_string(),
            });
        }
        let Some(tree) = self.parse_tree(source) else {
            return Ok(ParsedFile {
                symbols: Vec::new(),
                imports: Vec::new(),
                calls: Vec::new(),
            });
        };
        let root = tree.root_node();
        Ok(parse_go(path, &root, source))
    }
}

fn location_for_node(file: &str, node: &TsNode<'_>) -> SourceLocation {
    let start = node.start_position();
    let end = node.end_position();
    SourceLocation {
        file: file.to_string(),
        start_line: u32::try_from(start.row + 1).unwrap_or(u32::MAX),
        start_col: u32::try_from(start.column + 1).unwrap_or(u32::MAX),
        end_line: u32::try_from(end.row + 1).unwrap_or(u32::MAX),
        end_col: u32::try_from(end.column + 1).unwrap_or(u32::MAX),
    }
}

fn node_text<'a>(node: &TsNode<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn children_vec<'a>(node: &TsNode<'a>) -> Vec<TsNode<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

pub fn parse_go(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut stack: Vec<TsNode<'_>> = vec![*root];

    while let Some(node) = stack.pop() {
        match node.kind() {
            "package_clause" => {
                // package <name>
                if let Some(pkg_name_node) = node.child_by_field_name("package") {
                    let name = node_text(&pkg_name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Module,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        parent: None,
                    });
                } else {
                    // Fallback to searching children for identifier
                    for child in children_vec(&node) {
                        if child.kind() == "package_identifier" {
                            let name = node_text(&child, source).to_string();
                            let qualified = format!("{file}::{name}");
                            let loc = location_for_node(file, &node);
                            symbols.push(Symbol {
                                kind: NodeKind::Module,
                                name,
                                qualified_name: qualified,
                                location: loc,
                                parent: None,
                            });
                            break;
                        }
                    }
                }
            }
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: None,
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_go(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "method_declaration" => {
                // Method has receiver, name, body
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    let receiver_type = node
                        .child_by_field_name("receiver")
                        .and_then(|rec_node| extract_receiver_type_name(&rec_node, source));

                    symbols.push(Symbol {
                        kind: NodeKind::Method,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: receiver_type,
                    });

                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_go(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "type_declaration" => {
                // type_declaration can contain type_spec or alias_spec
                for child in children_vec(&node) {
                    if child.kind() == "type_spec" || child.kind() == "type_alias" {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let name = node_text(&name_node, source).to_string();
                            let qualified = format!("{file}::{name}");
                            let loc = location_for_node(file, &child);

                            let kind = if let Some(type_node) = child.child_by_field_name("type") {
                                match type_node.kind() {
                                    "struct_type" => NodeKind::Struct,
                                    "interface_type" => NodeKind::Interface,
                                    _ => NodeKind::Struct, // type alias or other types map to Struct
                                }
                            } else {
                                NodeKind::Struct
                            };

                            symbols.push(Symbol {
                                kind,
                                name,
                                qualified_name: qualified,
                                location: loc,
                                parent: None,
                            });
                        }
                    }
                }
            }
            "import_declaration" => {
                // import "fmt" or import ( "fmt" \n "net/http" )
                let loc = location_for_node(file, &node);
                for child in children_vec(&node) {
                    if child.kind() == "import_spec" || child.kind() == "import_spec_list" {
                        extract_import_specs(file, &child, source, &mut imports);
                    }
                }
                // Also handle single import_spec direct child
                if imports.is_empty() {
                    let text = node_text(&node, source).trim().to_string();
                    imports.push(Import {
                        path: text,
                        location: loc,
                    });
                }
            }
            _ => {}
        }

        // Don't recurse into function/method bodies since we handle them or type_declaration children
        if node.kind() == "function_declaration" || node.kind() == "method_declaration" {
            continue;
        }

        for child in children_vec(&node).into_iter().rev() {
            stack.push(child);
        }
    }

    ParsedFile {
        symbols,
        imports,
        calls,
    }
}

fn extract_receiver_type_name(rec_node: &TsNode<'_>, source: &[u8]) -> Option<String> {
    // parameter_list -> parameter_declaration -> type (type_identifier or pointer_type -> type_identifier)
    let text = node_text(rec_node, source).trim();
    // E.g. (s *Server) or (s Server) or (*Server) or (Server)
    let stripped = text.trim_matches(['(', ')']).trim();
    // Split by whitespace: could be "s *Server" or "*Server"
    let type_part = stripped.split_whitespace().last().unwrap_or(stripped);
    let type_name = type_part.trim_start_matches('*').trim();
    if type_name.is_empty() {
        None
    } else {
        Some(type_name.to_string())
    }
}

fn extract_import_specs(file: &str, node: &TsNode<'_>, source: &[u8], imports: &mut Vec<Import>) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "import_spec" {
            if let Some(path_node) = n.child_by_field_name("path") {
                let text = node_text(&path_node, source)
                    .trim()
                    .trim_matches('"')
                    .to_string();
                let loc = location_for_node(file, &n);
                imports.push(Import {
                    path: text,
                    location: loc,
                });
            } else {
                let text = node_text(&n, source).trim().to_string();
                let loc = location_for_node(file, &n);
                imports.push(Import {
                    path: text,
                    location: loc,
                });
            }
        } else {
            for child in children_vec(&n).into_iter().rev() {
                stack.push(child);
            }
        }
    }
}

pub fn extract_calls_go(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call_expression" {
            if let Some(func) = n.child_by_field_name("function") {
                let callee_raw = node_text(&func, source).trim().to_string();
                let simple = callee_raw
                    .rsplit('.')
                    .next()
                    .unwrap_or(&callee_raw)
                    .to_string();
                if !simple.is_empty()
                    && simple
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    let loc = location_for_node(file, &n);
                    calls.push(Call {
                        caller: caller.to_string(),
                        callee: simple,
                        location: loc,
                    });
                }
            }
        }
        for child in children_vec(&n).into_iter().rev() {
            stack.push(child);
        }
    }
}
