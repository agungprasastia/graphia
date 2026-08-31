use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{
    Call, Definition, Export, Import, ParsedFile, Reference, Symbol, normalize_signature,
};

pub struct ZigAnalyzer;

impl ZigAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    fn parse_tree(&self, source: &[u8]) -> Option<Tree> {
        let mut parser = Parser::new();
        let ts_lang = tree_sitter_zig::LANGUAGE.into();
        if let Err(error) = parser.set_language(&ts_lang) {
            eprintln!("set language failed: {error:?}");
            return None;
        }
        parser.parse(source, None)
    }
}

impl Default for ZigAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAnalyzer for ZigAnalyzer {
    fn language(&self) -> GraphiaLanguage {
        GraphiaLanguage::Zig
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
        Ok(parse_zig(path, &root, source))
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

pub fn parse_zig(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let type_references = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        let kind_str = node.kind();
        let is_pub = node_text(&node, source).trim_start().starts_with("pub ");
        let vis = if is_pub {
            Visibility::Public
        } else {
            Visibility::Private
        };

        match kind_str {
            "FnProto" | "FnDecl" | "function_declaration" | "function_signature" => {
                let mut name_opt = None;
                for child in children_vec(&node) {
                    if child.kind() == "IDENTIFIER" || child.kind() == "identifier" {
                        name_opt = Some(node_text(&child, source).to_string());
                        break;
                    }
                }
                if let Some(name) = name_opt {
                    let is_method = parent_scope.is_some();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let raw_sig = format!("{name}()");
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
                    if is_pub {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified.clone()),
                        });
                    }

                    for child in children_vec(&node) {
                        if child.kind() == "Block" || child.kind() == "block" {
                            extract_calls_zig(file, &child, source, &qualified, &mut calls);
                        }
                    }
                }
            }
            "VarDecl" | "variable_declaration" => {
                let full_text = node_text(&node, source);
                let mut name_opt = None;
                for child in children_vec(&node) {
                    if child.kind() == "IDENTIFIER" || child.kind() == "identifier" {
                        name_opt = Some(node_text(&child, source).to_string());
                        break;
                    }
                }

                if full_text.contains("@import") {
                    let loc = location_for_node(file, &node);
                    if let Some(start_quote) = full_text.find("@import(\"") {
                        let rest = &full_text[start_quote + 9..];
                        if let Some(end_quote) = rest.find('"') {
                            let path = rest[..end_quote].to_string();
                            imports.push(Import {
                                path,
                                location: loc.clone(),
                            });
                        }
                    } else if let Some(start_quote) = full_text.find("@import('") {
                        let rest = &full_text[start_quote + 9..];
                        if let Some(end_quote) = rest.find('\'') {
                            let path = rest[..end_quote].to_string();
                            imports.push(Import {
                                path,
                                location: loc.clone(),
                            });
                        }
                    }
                }

                let is_struct_or_enum = full_text.contains("struct")
                    || full_text.contains("enum")
                    || full_text.contains("union")
                    || full_text.contains("opaque");

                if let Some(name) = name_opt {
                    if is_struct_or_enum
                        && (full_text.contains("struct")
                            || full_text.contains("enum")
                            || full_text.contains("union"))
                    {
                        let qualified = format!("{file}::{name}");
                        let loc = location_for_node(file, &node);
                        let kind = if full_text.contains("enum") {
                            NodeKind::Enum
                        } else {
                            NodeKind::Struct
                        };

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
                        if is_pub {
                            exports.push(Export {
                                name: name.clone(),
                                location: loc,
                                target: Some(qualified),
                            });
                        }

                        for child in children_vec(&node).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
                    }
                }
            }
            "IDENTIFIER" | "identifier" => {
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

pub fn extract_calls_zig(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "CallExpr" || n.kind() == "call_expression" {
            let mut callee_opt = None;
            for child in children_vec(&n) {
                if child.kind() == "IDENTIFIER" || child.kind() == "identifier" {
                    callee_opt = Some(node_text(&child, source).to_string());
                    break;
                }
            }
            if let Some(simple) = callee_opt {
                if !simple.is_empty()
                    && simple != "@import"
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
