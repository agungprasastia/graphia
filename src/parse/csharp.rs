use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{
    Call, Definition, Export, Import, ParsedFile, Reference, Symbol, normalize_signature,
};

pub struct CSharpAnalyzer;

impl CSharpAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse_tree(&self, source: &[u8]) -> Option<Tree> {
        let mut parser = Parser::new();
        let ts_lang = tree_sitter_c_sharp::LANGUAGE.into();
        if let Err(error) = parser.set_language(&ts_lang) {
            eprintln!("set language failed: {error:?}");
            return None;
        }
        parser.parse(source, None)
    }
}

impl Default for CSharpAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAnalyzer for CSharpAnalyzer {
    fn language(&self) -> GraphiaLanguage {
        GraphiaLanguage::CSharp
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
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let type_references = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        let is_public = node_text(&node, source).contains("public ");
        let vis = if is_public {
            Visibility::Public
        } else if node_text(&node, source).contains("private ") {
            Visibility::Private
        } else if node_text(&node, source).contains("protected ") {
            Visibility::Protected
        } else if node_text(&node, source).contains("internal ") {
            Visibility::Internal
        } else {
            Visibility::Private
        };

        match node.kind() {
            "namespace_declaration" | "file_scoped_namespace_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Namespace,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Namespace,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        container: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
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
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                    });
                    if is_public {
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
                    }
                }
                continue;
            }
            "struct_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                    });
                    if is_public {
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
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                    });
                    if is_public {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified),
                        });
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
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Enum,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                    });
                    if is_public {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified),
                        });
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
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Interface,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                    });
                    if is_public {
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
                        kind: NodeKind::Property,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Property,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: None,
                    });
                    if is_public {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified),
                        });
                    }
                }
                continue;
            }
            "constructor_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let raw_sig = format!("{name}{params_text}");
                    let sig = Some(normalize_signature(&raw_sig));

                    symbols.push(Symbol {
                        kind: NodeKind::Constructor,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: vis,
                        signature: sig.clone(),
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Constructor,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_scope.clone(),
                        visibility: vis,
                        signature: sig,
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_csharp(file, &body, source, &qualified, &mut calls);
                    }
                }
                continue;
            }
            "method_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let return_type = node
                        .child_by_field_name("returns")
                        .map(|r| node_text(&r, source).trim())
                        .unwrap_or("");
                    let raw_sig = if return_type.is_empty() {
                        format!("{name}{params_text}")
                    } else {
                        format!("{name}{params_text}->{return_type}")
                    };
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
                    if is_public {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified.clone()),
                        });
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_csharp(file, &body, source, &qualified, &mut calls);
                    }
                }
                continue;
            }
            "identifier" => {
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

pub fn extract_calls_csharp(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "invocation_expression" {
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
        } else if n.kind() == "object_creation_expression" {
            if let Some(type_node) = n.child_by_field_name("type") {
                let simple = node_text(&type_node, source).trim().to_string();
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
