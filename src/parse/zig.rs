use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{Call, Import, ParsedFile, Symbol};

pub struct ZigAnalyzer {
    language: GraphiaLanguage,
}

impl Default for ZigAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl ZigAnalyzer {
    #[must_use]
    pub fn new() -> Self {
        Self {
            language: GraphiaLanguage::Zig,
        }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        tree_sitter_zig::LANGUAGE.into()
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

impl LanguageAnalyzer for ZigAnalyzer {
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
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        let kind_str = node.kind();
        match kind_str {
            // Function declaration: `pub fn foo(...) ...` or `fn bar(...) ...`
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

                    // Search for function body or block
                    for child in children_vec(&node) {
                        if child.kind() == "Block" || child.kind() == "block" {
                            extract_calls_zig(file, &child, source, &qualified, &mut calls);
                        }
                    }
                }
            }
            // Variable / Constant declaration: `pub const MyStruct = struct { ... };` or `const x = @import("x");`
            "VarDecl" | "variable_declaration" => {
                let full_text = node_text(&node, source);
                let mut name_opt = None;
                for child in children_vec(&node) {
                    if child.kind() == "IDENTIFIER" || child.kind() == "identifier" {
                        name_opt = Some(node_text(&child, source).to_string());
                        break;
                    }
                }

                // Check for @import extraction
                if full_text.contains("@import") {
                    let loc = location_for_node(file, &node);
                    // extract import path between quotes
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

                // Check if struct, enum, union or interface declaration
                let is_struct_or_enum = full_text.contains("struct")
                    || full_text.contains("enum")
                    || full_text.contains("union")
                    || full_text.contains("opaque");

                if let Some(name) = name_opt {
                    if is_struct_or_enum
                        && (full_text.contains("struct")
                            || full_text.contains("union")
                            || full_text.contains("enum"))
                    {
                        let qualified = format!("{file}::{name}");
                        let loc = location_for_node(file, &node);
                        symbols.push(Symbol {
                            kind: NodeKind::Struct,
                            name: name.clone(),
                            qualified_name: qualified,
                            location: loc,
                            parent: parent_scope.clone(),
                        });

                        // Push inner children with this struct name as parent
                        for child in children_vec(&node).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
                    }
                }
            }
            _ => {}
        }

        // Generic fallback for function definitions embedded in container declarations
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

pub fn extract_calls_zig(
    file: &str,
    node: &TsNode<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        let kind = n.kind();
        if kind == "CallExpr" || kind == "call_expression" || kind == "FnCallArguments" {
            // callee is either the first child or previous sibling depending on grammar
            let callee_text = if let Some(func) = n.child_by_field_name("function") {
                node_text(&func, source).trim().to_string()
            } else if let Some(first) = n.child(0) {
                node_text(&first, source).trim().to_string()
            } else {
                String::new()
            };

            let simple = callee_text
                .rsplit('.')
                .next()
                .unwrap_or(&callee_text)
                .trim();

            if !simple.is_empty()
                && !simple.starts_with('@')
                && simple
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
            {
                let loc = location_for_node(file, &n);
                calls.push(Call {
                    caller: caller.to_string(),
                    callee: simple.to_string(),
                    location: loc,
                });
            }
        }
        for child in children_vec(&n).into_iter().rev() {
            stack.push(child);
        }
    }
}
