use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{Call, Import, ParsedFile, Symbol};

pub struct JavaAnalyzer {
    language: GraphiaLanguage,
}

impl Default for JavaAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            language: GraphiaLanguage::Java,
        }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        tree_sitter_java::LANGUAGE.into()
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

impl LanguageAnalyzer for JavaAnalyzer {
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
                definitions: Vec::new(),
                references: Vec::new(),
                exports: Vec::new(),
                type_references: Vec::new(),
            });
        };
        let root = tree.root_node();
        Ok(parse_java(path, &root, source))
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

pub fn parse_java(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_class)) = stack.pop() {
        match node.kind() {
            "package_declaration" => {
                // package com.example.service;
                // identifier or scoped_identifier
                for child in children_vec(&node) {
                    if child.kind() == "scoped_identifier" || child.kind() == "identifier" {
                        let name = node_text(&child, source).trim().to_string();
                        let qualified = format!("{file}::{name}");
                        let loc = location_for_node(file, &node);
                        symbols.push(Symbol {
                            kind: NodeKind::Package,
                            name,
                            qualified_name: qualified,
                            location: loc,
                            parent: None,
                            visibility: crate::model::Visibility::Public,
                            signature: None,
                            container: None,
                        });
                        break;
                    }
                }
            }
            "import_declaration" => {
                let text = node_text(&node, source)
                    .trim()
                    .trim_end_matches(';')
                    .to_string();
                let path = text
                    .trim_start_matches("import")
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
                        parent: parent_class.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_class.clone(),
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
                        parent: parent_class.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_class.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    }
                }
                continue;
            }
            "record_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_class.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_class.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    }
                }
                continue;
            }
            "enum_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Enum,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_class.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_class.clone(),
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
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Method,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: parent_class.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_class.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_java(file, &body, source, &qualified, &mut calls);
                    }
                }
                continue;
            }
            "constructor_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Constructor,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: parent_class.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_class.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_java(file, &body, source, &qualified, &mut calls);
                    }
                }
                continue;
            }
            _ => {}
        }

        for child in children_vec(&node).into_iter().rev() {
            stack.push((child, parent_class.clone()));
        }
    }

    ParsedFile {
        symbols,
        imports,
        calls,
        definitions: Vec::new(),
        references: Vec::new(),
        exports: Vec::new(),
        type_references: Vec::new(),
    }
}

pub fn extract_calls_java(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        match n.kind() {
            "method_invocation" => {
                if let Some(name_node) = n.child_by_field_name("name") {
                    let callee = node_text(&name_node, source).trim().to_string();
                    if !callee.is_empty()
                        && callee
                            .chars()
                            .next()
                            .is_some_and(|c| c.is_alphabetic() || c == '_')
                    {
                        let loc = location_for_node(file, &n);
                        calls.push(Call {
                            caller: caller.to_string(),
                            callee,
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
            "explicit_constructor_invocation" => {
                // this(...) or super(...)
                if let Some(constructor) = n.child_by_field_name("constructor") {
                    let callee = node_text(&constructor, source).trim().to_string();
                    if !callee.is_empty() {
                        let loc = location_for_node(file, &n);
                        calls.push(Call {
                            caller: caller.to_string(),
                            callee,
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
