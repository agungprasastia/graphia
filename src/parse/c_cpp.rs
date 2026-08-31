use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{Call, Import, ParsedFile, Symbol};

pub struct CCppAnalyzer {
    language: GraphiaLanguage,
}

impl CCppAnalyzer {
    #[must_use]
    pub fn new(language: GraphiaLanguage) -> Self {
        Self { language }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        match self.language {
            GraphiaLanguage::C => tree_sitter_c::LANGUAGE.into(),
            GraphiaLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            _ => tree_sitter_cpp::LANGUAGE.into(),
        }
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

impl LanguageAnalyzer for CCppAnalyzer {
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
        Ok(parse_c_cpp(path, &root, source))
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

pub fn parse_c_cpp(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    // Stack contains node and optional parent scope (class/struct/namespace)
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        match node.kind() {
            "preproc_include" => {
                // #include <stdio.h> or #include "my_header.h"
                if let Some(path_node) = node.child_by_field_name("path") {
                    let path_text = node_text(&path_node, source)
                        .trim()
                        .trim_matches(['<', '>', '"'])
                        .to_string();
                    let loc = location_for_node(file, &node);
                    imports.push(Import {
                        path: path_text,
                        location: loc,
                    });
                } else {
                    let text = node_text(&node, source).trim().to_string();
                    let loc = location_for_node(file, &node);
                    imports.push(Import {
                        path: text,
                        location: loc,
                    });
                }
            }
            "namespace_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Namespace,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, None));
                        }
                        continue;
                    }
                }
            }
            "class_specifier" => {
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
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
                    }
                }
            }
            "struct_specifier" => {
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
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
                    }
                }
            }
            "type_definition" => {
                // typedef struct ... Foo; or typedef int MyInt;
                // Look for type identifier at the end
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    let name = node_text(&declarator, source).to_string();
                    if !name.is_empty() {
                        let qualified = format!("{file}::{name}");
                        let loc = location_for_node(file, &node);
                        symbols.push(Symbol {
                            kind: NodeKind::TypeAlias,
                            name,
                            qualified_name: qualified,
                            location: loc,
                            parent: parent_scope.clone(),
                            visibility: crate::model::Visibility::Public,
                            signature: None,
                            container: parent_scope.clone(),
                        });
                    }
                }
            }
            "alias_declaration" => {
                // using MyType = int;
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::TypeAlias,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        parent: parent_scope.clone(),
                        visibility: crate::model::Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                }
            }
            "function_definition" => {
                // Function or method definition with body
                if let Some(decl) = node.child_by_field_name("declarator") {
                    let (name, class_qualifier) = extract_function_name_and_scope(&decl, source);
                    if !name.is_empty() {
                        let effective_parent = class_qualifier.or_else(|| parent_scope.clone());
                        let is_method = effective_parent.is_some();
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
                            parent: effective_parent.clone(),
                            visibility: crate::model::Visibility::Public,
                            signature: None,
                            container: effective_parent,
                        });
                        if let Some(body) = node.child_by_field_name("body") {
                            extract_calls_c_cpp(file, &body, source, &qualified, &mut calls);
                        }
                    }
                }
                continue;
            }
            "field_declaration" | "declaration" => {
                // Could be function prototype or member function declaration inside class
                if let Some(decl) = node.child_by_field_name("declarator") {
                    // Check if declarator is a function_declarator
                    if is_function_declarator(&decl) {
                        let (name, class_qualifier) =
                            extract_function_name_and_scope(&decl, source);
                        if !name.is_empty() {
                            let effective_parent = class_qualifier.or_else(|| parent_scope.clone());
                            let is_method = effective_parent.is_some();
                            let qualified = format!("{file}::{name}");
                            let loc = location_for_node(file, &node);
                            symbols.push(Symbol {
                                kind: if is_method {
                                    NodeKind::Method
                                } else {
                                    NodeKind::Function
                                },
                                name,
                                qualified_name: qualified,
                                location: loc,
                                parent: effective_parent.clone(),
                                visibility: crate::model::Visibility::Public,
                                signature: None,
                                container: effective_parent,
                            });
                        }
                    }
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
        definitions: Vec::new(),
        references: Vec::new(),
        exports: Vec::new(),
        type_references: Vec::new(),
    }
}

fn is_function_declarator(node: &TsNode<'_>) -> bool {
    let mut current = *node;
    loop {
        match current.kind() {
            "function_declarator" => return true,
            "pointer_declarator" | "reference_declarator" => {
                if let Some(declarator) = current.child_by_field_name("declarator") {
                    current = declarator;
                } else {
                    return false;
                }
            }
            _ => return false,
        }
    }
}

fn extract_function_name_and_scope(
    declarator: &TsNode<'_>,
    source: &[u8],
) -> (String, Option<String>) {
    let mut current = *declarator;
    // Unwrap pointer_declarator, reference_declarator, etc.
    loop {
        match current.kind() {
            "function_declarator" => {
                if let Some(decl) = current.child_by_field_name("declarator") {
                    current = decl;
                } else {
                    break;
                }
            }
            "pointer_declarator" | "reference_declarator" => {
                if let Some(decl) = current.child_by_field_name("declarator") {
                    current = decl;
                } else {
                    break;
                }
            }
            "qualified_identifier" | "scoped_identifier" => {
                // e.g. ClassName::methodName or ns::ClassName::methodName
                let full = node_text(&current, source);
                if let Some((scope, name)) = full.rsplit_once("::") {
                    let parent = scope.rsplit("::").next().unwrap_or(scope).to_string();
                    return (name.to_string(), Some(parent));
                }
                return (full.to_string(), None);
            }
            "identifier" | "field_identifier" => {
                return (node_text(&current, source).to_string(), None);
            }
            "destructor_name" => {
                let full = node_text(&current, source);
                return (full.to_string(), None);
            }
            _ => {
                // If it's a leaf or unhandled node, fallback to text
                let text = node_text(&current, source);
                if let Some((scope, name)) = text.rsplit_once("::") {
                    let parent = scope.rsplit("::").next().unwrap_or(scope).to_string();
                    return (name.to_string(), Some(parent));
                }
                return (text.to_string(), None);
            }
        }
    }
    let text = node_text(&current, source).to_string();
    (text, None)
}

pub fn extract_calls_c_cpp(
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
                    .rsplit("::")
                    .next()
                    .unwrap_or(&callee_raw)
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
                        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '~')
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
