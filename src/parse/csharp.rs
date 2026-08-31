use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{Call, Import, ParsedFile, Symbol};

pub struct CSharpAnalyzer {
    language: GraphiaLanguage,
}

impl Default for CSharpAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl CSharpAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            language: GraphiaLanguage::CSharp,
        }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        tree_sitter_c_sharp::LANGUAGE.into()
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

impl LanguageAnalyzer for CSharpAnalyzer {
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
        Ok(parse_csharp(path, &root, source))
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

pub fn parse_csharp(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        match node.kind() {
            "namespace_declaration" | "file_scoped_namespace_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Module,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, parent_scope.clone()));
                        }
                        continue;
                    }
                }
            }
            "using_directive" => {
                let text = node_text(&node, source)
                    .trim()
                    .trim_end_matches(';')
                    .to_string();
                let path = text
                    .trim_start_matches("global")
                    .trim_start_matches("using")
                    .trim_start_matches("static")
                    .trim()
                    .to_string();
                let loc = location_for_node(file, &node);
                imports.push(Import {
                    path,
                    location: loc,
                });
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    }
                }
                continue;
            }
            "struct_declaration" | "enum_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    }
                }
                continue;
            }
            "record_declaration" | "record_struct_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    }
                }
                continue;
            }
            "interface_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Interface,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    }
                }
                continue;
            }
            "method_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let is_method = parent_scope.is_some();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: if is_method {
                            NodeKind::Method
                        } else {
                            NodeKind::Function
                        },
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_csharp(file, &body, source, &qualified, &mut calls);
                    }
                }
                continue;
            }
            "constructor_declaration" | "destructor_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Method,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_csharp(file, &body, source, &qualified, &mut calls);
                    }
                }
                continue;
            }
            "property_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Method,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    // Extract calls inside accessors if any
                    extract_calls_csharp(file, &node, source, &qualified, &mut calls);
                }
                continue;
            }
            "local_function_statement" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_csharp(file, &body, source, &qualified, &mut calls);
                    }
                }
                continue;
            }
            _ => {}
        }

        for child in children_vec(&node).into_iter().rev() {
            stack.push((child, parent_scope.clone()));
        }
    }

    ParsedFile {
        symbols,
        imports,
        calls,
    }
}

pub fn extract_calls_csharp(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "invocation_expression" => {
                if let Some(func) = n.child_by_field_name("function") {
                    let callee_raw = node_text(&func, source).trim().to_string();
                    let simple = callee_raw
                        .rsplit('.')
                        .next()
                        .unwrap_or(&callee_raw)
                        .rsplit("->")
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
            "object_creation_expression" => {
                if let Some(type_node) = n.child_by_field_name("type") {
                    let callee_raw = node_text(&type_node, source).trim().to_string();
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
            _ => {}
        }
        for child in children_vec(&n).into_iter().rev() {
            stack.push(child);
        }
    }
}
