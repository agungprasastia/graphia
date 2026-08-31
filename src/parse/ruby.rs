use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{
    Call, Definition, Export, Import, ParsedFile, Reference, Symbol, normalize_signature,
};

pub struct RubyAnalyzer;

impl RubyAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse_tree(&self, source: &[u8]) -> Option<Tree> {
        let mut parser = Parser::new();
        let ts_lang = tree_sitter_ruby::LANGUAGE.into();
        if let Err(error) = parser.set_language(&ts_lang) {
            eprintln!("set language failed: {error:?}");
            return None;
        }
        parser.parse(source, None)
    }
}

impl Default for RubyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAnalyzer for RubyAnalyzer {
    fn language(&self) -> GraphiaLanguage {
        GraphiaLanguage::Ruby
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
        Ok(parse_ruby(path, &root, source))
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

pub fn parse_ruby(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let type_references = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        match node.kind() {
            "module" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Module,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Module,
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
            "class" => {
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
            "method" | "singleton_method" => {
                let mut name_opt = None;
                if let Some(name_node) = node.child_by_field_name("name") {
                    name_opt = Some(node_text(&name_node, source).to_string());
                }
                if let Some(name) = name_opt {
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
                        visibility: Visibility::Public,
                        signature: sig.clone(),
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind,
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
                        extract_calls_ruby(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "call" => {
                let text = node_text(&node, source);
                if text.starts_with("require ") || text.starts_with("require_relative ") {
                    let path = text
                        .trim_start_matches("require_relative")
                        .trim_start_matches("require")
                        .trim()
                        .trim_matches(['"', '\''])
                        .to_string();
                    let loc = location_for_node(file, &node);
                    imports.push(Import {
                        path,
                        location: loc,
                    });
                }
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
        instantiations: Vec::new(),
        inheritances: Vec::new(),
        implementations: Vec::new(),
    }
}

pub fn extract_calls_ruby(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call"
            && let Some(method_node) = n.child_by_field_name("method")
        {
            let callee_raw = node_text(&method_node, source).trim().to_string();
            let simple = callee_raw
                .rsplit('.')
                .next()
                .unwrap_or(&callee_raw)
                .to_string();
            if !simple.is_empty()
                && simple != "require"
                && simple != "require_relative"
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
        for child in children_vec(&n).into_iter().rev() {
            stack.push(child);
        }
    }
}
