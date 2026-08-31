use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{
    Call, Definition, Export, Import, ParsedFile, Reference, Symbol, normalize_signature,
};

pub struct PhpAnalyzer;

impl PhpAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse_tree(&self, source: &[u8]) -> Option<Tree> {
        let mut parser = Parser::new();
        let ts_lang = tree_sitter_php::LANGUAGE_PHP.into();
        if let Err(error) = parser.set_language(&ts_lang) {
            eprintln!("set language failed: {error:?}");
            return None;
        }
        parser.parse(source, None)
    }
}

impl Default for PhpAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAnalyzer for PhpAnalyzer {
    fn language(&self) -> GraphiaLanguage {
        GraphiaLanguage::Php
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
                instantiations: Vec::new(),
                inheritances: Vec::new(),
                implementations: Vec::new(),
            });
        };
        let root = tree.root_node();
        Ok(parse_php(path, &root, source))
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

pub fn parse_php(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
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
            "namespace_definition" => {
                let mut ns_name = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    ns_name = Some(node_text(&name_node, source).to_string());
                } else {
                    for child in children_vec(&node) {
                        if child.kind() == "namespace_name" || child.kind() == "name" {
                            ns_name = Some(node_text(&child, source).to_string());
                            break;
                        }
                    }
                }
                if let Some(name) = ns_name {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Namespace,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: Visibility::Public,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Namespace,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        container: None,
                        visibility: Visibility::Public,
                        signature: None,
                    });
                }
            }
            "namespace_use_declaration" => {
                let text = node_text(&node, source)
                    .trim()
                    .trim_end_matches(';')
                    .to_string();
                let path = text
                    .trim_start_matches("use")
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                let loc = location_for_node(file, &node);
                if !path.is_empty() {
                    imports.push(Import {
                        path,
                        location: loc,
                    });
                }
            }
            "class_declaration" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                    });
                    exports.push(Export {
                        name: name.clone(),
                        location: loc,
                        target: Some(qualified),
                    });

                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
                    }
                }
            }
            "trait_declaration" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Trait,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Trait,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                    });
                    exports.push(Export {
                        name: name.clone(),
                        location: loc,
                        target: Some(qualified),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
                    }
                }
            }
            "enum_declaration" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Enum,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Enum,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                    });
                    exports.push(Export {
                        name: name.clone(),
                        location: loc,
                        target: Some(qualified),
                    });
                }
            }
            "interface_declaration" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Interface,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Interface,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                    });
                    exports.push(Export {
                        name: name.clone(),
                        location: loc,
                        target: Some(qualified),
                    });
                }
            }
            "method_declaration" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let raw_sig = format!("{name}{params_text}");
                    let sig = Some(normalize_signature(&raw_sig));

                    symbols.push(Symbol {
                        kind: NodeKind::Method,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: sig.clone(),
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Method,
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
                        extract_calls_php(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "function_definition" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let raw_sig = format!("{name}{params_text}");
                    let sig = Some(normalize_signature(&raw_sig));

                    symbols.push(Symbol {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: sig.clone(),
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: sig,
                    });
                    exports.push(Export {
                        name: name.clone(),
                        location: loc,
                        target: Some(qualified.clone()),
                    });

                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_php(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "name" | "variable_name" => {
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
        instantiations: Vec::new(),
        inheritances: Vec::new(),
        implementations: Vec::new(),
    }
}

pub fn extract_calls_php(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "function_call_expression" || n.kind() == "member_call_expression" {
            if let Some(func) = n
                .child_by_field_name("function")
                .or_else(|| n.child_by_field_name("name"))
            {
                let callee_raw = node_text(&func, source).trim().to_string();
                let simple = callee_raw
                    .rsplit("->")
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
