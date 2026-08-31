use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{Call, Import, ParsedFile, Symbol};

pub struct KotlinAnalyzer {
    language: GraphiaLanguage,
}

impl Default for KotlinAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl KotlinAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            language: GraphiaLanguage::Kotlin,
        }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        tree_sitter_kotlin::LANGUAGE.into()
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

impl LanguageAnalyzer for KotlinAnalyzer {
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
        Ok(parse_kotlin(path, &root, source))
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

pub fn parse_kotlin(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        match node.kind() {
            "package_header" => {
                // package com.example.service
                for child in children_vec(&node) {
                    if child.kind() == "identifier" || child.kind() == "package_identifier" {
                        let name = node_text(&child, source).trim().to_string();
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
                // Fallback: if no package_identifier node, look for identifier/user_type children or text
                if symbols.is_empty() {
                    let text = node_text(&node, source).trim();
                    let pkg = text.trim_start_matches("package").trim().to_string();
                    if !pkg.is_empty() {
                        let qualified = format!("{file}::{pkg}");
                        let loc = location_for_node(file, &node);
                        symbols.push(Symbol {
                            kind: NodeKind::Module,
                            name: pkg,
                            qualified_name: qualified,
                            location: loc,
                            parent: None,
                        });
                    }
                }
            }
            "import_header" | "import" => {
                // import com.example.service.Helper
                let text = node_text(&node, source)
                    .trim()
                    .trim_end_matches(';')
                    .to_string();
                let path = text.trim_start_matches("import").trim().to_string();
                let loc = location_for_node(file, &node);
                imports.push(Import {
                    path,
                    location: loc,
                });
            }
            "class_declaration" => {
                // class / interface / enum / data class
                let mut kind = NodeKind::Class;
                let mut class_name = None;

                for child in children_vec(&node) {
                    if child.kind() == "type_identifier" || child.kind() == "identifier" {
                        if class_name.is_none() {
                            class_name = Some(node_text(&child, source).to_string());
                        }
                    } else if child.kind() == "interface" {
                        kind = NodeKind::Interface;
                    } else if child.kind() == "modifiers" {
                        let mod_text = node_text(&child, source);
                        if mod_text.contains("data") || mod_text.contains("enum") {
                            kind = NodeKind::Struct;
                        }
                    }
                }

                if let Some(name) = class_name {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                    });

                    // Search for class_body
                    for child in children_vec(&node) {
                        if child.kind() == "class_body" || child.kind() == "enum_class_body" {
                            for sub in children_vec(&child).into_iter().rev() {
                                stack.push((sub, Some(name.clone())));
                            }
                        }
                    }
                }
                continue;
            }
            "object_declaration" => {
                // object Singleton
                let mut object_name = None;
                for child in children_vec(&node) {
                    if (child.kind() == "type_identifier" || child.kind() == "identifier")
                        && object_name.is_none()
                    {
                        object_name = Some(node_text(&child, source).to_string());
                    }
                }
                if let Some(name) = object_name {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                    });
                    for child in children_vec(&node) {
                        if child.kind() == "class_body" {
                            for sub in children_vec(&child).into_iter().rev() {
                                stack.push((sub, Some(name.clone())));
                            }
                        }
                    }
                }
                continue;
            }
            "function_declaration" => {
                // fun doWork() or fun Class.doWork()
                let mut func_name = None;
                for child in children_vec(&node) {
                    if child.kind() == "simple_identifier" || child.kind() == "identifier" {
                        func_name = Some(node_text(&child, source).to_string());
                        break;
                    }
                }
                if let Some(name) = func_name {
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
                    for child in children_vec(&node) {
                        if child.kind() == "function_body" || child.kind() == "block" {
                            extract_calls_kotlin(file, &child, source, &qualified, &mut calls);
                        }
                    }
                }
                continue;
            }
            "secondary_constructor" | "primary_constructor" => {
                // constructor(...)
                let is_method = parent_scope.is_some();
                let name = parent_scope.clone().unwrap_or_else(|| "constructor".into());
                let qualified = format!("{file}::{name}");
                let loc = location_for_node(file, &node);
                symbols.push(Symbol {
                    kind: if is_method {
                        NodeKind::Method
                    } else {
                        NodeKind::Function
                    },
                    name,
                    qualified_name: qualified.clone(),
                    location: loc,
                    parent: parent_scope.clone(),
                });
                extract_calls_kotlin(file, &node, source, &qualified, &mut calls);
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

pub fn extract_calls_kotlin(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call_expression" {
            // call_expression in kotlin tree-sitter: child 0 is callee or navigation_expression
            if let Some(first_child) = n.child(0) {
                let callee_raw = node_text(&first_child, source).trim().to_string();
                let simple = callee_raw
                    .rsplit('.')
                    .next()
                    .unwrap_or(&callee_raw)
                    .rsplit("::")
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
