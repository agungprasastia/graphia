use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{
    Call, Definition, Export, Import, ParsedFile, Reference, Symbol, normalize_signature,
};

pub struct KotlinAnalyzer;

impl KotlinAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse_tree(&self, source: &[u8]) -> Option<Tree> {
        let mut parser = Parser::new();
        let ts_lang = tree_sitter_kotlin::LANGUAGE.into();
        if let Err(error) = parser.set_language(&ts_lang) {
            eprintln!("set language failed: {error:?}");
            return None;
        }
        parser.parse(source, None)
    }
}

impl Default for KotlinAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAnalyzer for KotlinAnalyzer {
    fn language(&self) -> GraphiaLanguage {
        GraphiaLanguage::Kotlin
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
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let type_references = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        let is_private = node_text(&node, source).contains("private ");
        let vis = if is_private {
            Visibility::Private
        } else {
            Visibility::Public
        };

        match node.kind() {
            "package_header" => {
                let mut pkg_name = None;
                for child in children_vec(&node) {
                    if child.kind() == "identifier"
                        || child.kind() == "package_identifier"
                        || child.kind() == "identifier"
                    {
                        pkg_name = Some(node_text(&child, source).trim().to_string());
                        break;
                    }
                }
                if pkg_name.is_none() {
                    let text = node_text(&node, source).trim();
                    let stripped = text
                        .trim_start_matches("package")
                        .trim()
                        .trim_end_matches(';')
                        .trim()
                        .to_string();
                    if !stripped.is_empty() {
                        pkg_name = Some(stripped);
                    }
                }
                if let Some(name) = pkg_name {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Package,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: Visibility::Public,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Package,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        container: None,
                        visibility: Visibility::Public,
                        signature: None,
                    });
                }
            }
            "import_header" | "import" => {
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
            "class_declaration" | "object_declaration" => {
                let mut kind = NodeKind::Class;
                let mut class_name = None;

                for child in children_vec(&node) {
                    if child.kind() == "type_identifier"
                        || child.kind() == "identifier"
                        || child.kind() == "simple_identifier"
                    {
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
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                    });
                    if vis == Visibility::Public {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified),
                        });
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    } else {
                        for child in children_vec(&node) {
                            if child.kind() == "class_body" {
                                for sub in children_vec(&child).into_iter().rev() {
                                    stack.push((sub, Some(name.clone())));
                                }
                            }
                        }
                    }
                }
                continue;
            }
            "function_declaration" => {
                let mut fn_name = None;
                for child in children_vec(&node) {
                    if child.kind() == "identifier" || child.kind() == "simple_identifier" {
                        fn_name = Some(node_text(&child, source).to_string());
                        break;
                    }
                }
                if let Some(name) = fn_name {
                    let is_method = parent_scope.is_some();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let raw_sig = format!("{name}{params_text}");
                    let sig = Some(normalize_signature(&raw_sig));

                    let kind = if is_method {
                        NodeKind::Method
                    } else {
                        NodeKind::Function
                    };

                    symbols.push(Symbol {
                        kind,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: sig.clone(),
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: sig,
                    });
                    if vis == Visibility::Public {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified.clone()),
                        });
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_kotlin(file, &body, source, &qualified, &mut calls);
                    } else {
                        for child in children_vec(&node) {
                            if child.kind() == "function_body" || child.kind() == "block" {
                                extract_calls_kotlin(file, &child, source, &qualified, &mut calls);
                            }
                        }
                    }
                }
                continue;
            }
            "identifier" | "simple_identifier" => {
                let name = node_text(&node, source).to_string();
                if !name.is_empty() {
                    references.push(Reference {
                        name,
                        location: location_for_node(file, &node),
                        caller: None,
                    });
                }
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
        definitions,
        references,
        exports,
        type_references,
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
            let mut callee_simple = None;
            for child in children_vec(&n) {
                if child.kind() == "identifier" || child.kind() == "simple_identifier" {
                    callee_simple = Some(node_text(&child, source).trim().to_string());
                    break;
                } else if child.kind() == "navigation_expression" {
                    let text = node_text(&child, source).trim().to_string();
                    let simple = text.rsplit('.').next().unwrap_or(&text).to_string();
                    callee_simple = Some(simple);
                    break;
                }
            }
            if let Some(simple) = callee_simple {
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
