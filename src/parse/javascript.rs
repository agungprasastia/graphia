use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{
    Call, Definition, Export, Import, ParsedFile, Reference, Symbol, TypeReference,
    normalize_signature,
};

pub struct JavaScriptAnalyzer {
    language: GraphiaLanguage,
}

impl JavaScriptAnalyzer {
    #[must_use]
    pub fn new(language: GraphiaLanguage) -> Self {
        Self { language }
    }

    fn ts_language(&self) -> tree_sitter::Language {
        match self.language {
            GraphiaLanguage::JavaScript | GraphiaLanguage::Jsx => {
                tree_sitter_javascript::LANGUAGE.into()
            }
            GraphiaLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            GraphiaLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            _ => tree_sitter_javascript::LANGUAGE.into(),
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

impl LanguageAnalyzer for JavaScriptAnalyzer {
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
                instantiations: Vec::new(),
                inheritances: Vec::new(),
                implementations: Vec::new(),
            });
        };
        let root = tree.root_node();
        Ok(parse_js_family(path, &root, source))
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

pub fn parse_js_family(file: &str, root: &TsNode<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let mut type_references = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_class)) = stack.pop() {
        let is_exported = node.parent().is_some_and(|p| {
            p.kind() == "export_statement" || p.kind() == "export_default_statement"
        });

        match node.kind() {
            "function_declaration" | "function" | "generator_function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let return_type = node
                        .child_by_field_name("return_type")
                        .map(|r| node_text(&r, source).trim_start_matches(':').trim())
                        .unwrap_or("");
                    let raw_sig = if return_type.is_empty() {
                        format!("{name}{params_text}")
                    } else {
                        format!("{name}{params_text}->{return_type}")
                    };
                    let sig = Some(normalize_signature(&raw_sig));

                    symbols.push(Symbol {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: Visibility::Public,
                        signature: sig.clone(),
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: Visibility::Public,
                        signature: sig,
                    });
                    if is_exported {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified.clone()),
                        });
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_js(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let return_type = node
                        .child_by_field_name("return_type")
                        .map(|r| node_text(&r, source).trim_start_matches(':').trim())
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
                        parent: parent_class.clone(),
                        visibility: Visibility::Public,
                        signature: sig.clone(),
                        container: parent_class.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Method,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_class.clone(),
                        visibility: Visibility::Public,
                        signature: sig,
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_js(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "class_declaration" | "class" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: Visibility::Public,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: Visibility::Public,
                        signature: None,
                    });
                    if is_exported {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified),
                        });
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                    }
                }
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
                        parent: None,
                        visibility: Visibility::Public,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Interface,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: Visibility::Public,
                        signature: None,
                    });
                    if is_exported {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified),
                        });
                    }
                }
            }
            "variable_declarator" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);

                    if let Some(value_node) = node.child_by_field_name("value") {
                        let value_kind = value_node.kind();
                        if value_kind == "arrow_function" || value_kind == "function" {
                            let params_text = value_node
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
                                parent: None,
                                visibility: Visibility::Public,
                                signature: sig.clone(),
                                container: None,
                            });
                            definitions.push(Definition {
                                kind: NodeKind::Function,
                                name: name.clone(),
                                qualified_name: qualified.clone(),
                                location: loc.clone(),
                                container: None,
                                visibility: Visibility::Public,
                                signature: sig,
                            });
                            if is_exported {
                                exports.push(Export {
                                    name: name.clone(),
                                    location: loc.clone(),
                                    target: Some(qualified.clone()),
                                });
                            }
                            if let Some(body) = value_node.child_by_field_name("body") {
                                extract_calls_js(file, &body, source, &qualified, &mut calls);
                            }
                        } else if value_kind == "call_expression" {
                            let call_text = node_text(&value_node, source);
                            if call_text.contains("require(") {
                                imports.push(Import {
                                    path: format!("const {{ {name} }} = {call_text}"),
                                    location: loc.clone(),
                                });
                            }
                        }
                    }
                }
            }
            "type_alias_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::TypeAlias,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: Visibility::Public,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::TypeAlias,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: Visibility::Public,
                        signature: None,
                    });
                    if is_exported {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified),
                        });
                    }
                }
            }
            "import_statement" => {
                let text = node_text(&node, source)
                    .trim()
                    .trim_end_matches(';')
                    .to_string();
                let loc = location_for_node(file, &node);
                imports.push(Import {
                    path: text,
                    location: loc,
                });
            }
            "identifier" | "property_identifier" => {
                let name = node_text(&node, source).to_string();
                if !name.is_empty() {
                    references.push(Reference {
                        name,
                        location: location_for_node(file, &node),
                        caller: None,
                    });
                }
            }
            "type_annotation" => {
                let text = node_text(&node, source)
                    .trim_start_matches(':')
                    .trim()
                    .to_string();
                let loc = location_for_node(file, &node);
                if !text.is_empty() {
                    type_references.push(TypeReference {
                        name: text,
                        location: loc,
                        container: parent_class.clone(),
                    });
                }
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
        definitions,
        references,
        exports,
        type_references,
        instantiations: Vec::new(),
        inheritances: Vec::new(),
        implementations: Vec::new(),
    }
}

pub fn extract_calls_js(
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
                    .rsplit('.')
                    .next()
                    .unwrap_or(&callee_raw)
                    .to_string();
                // Avoid capturing require as a standard function call if it is an import
                if !simple.is_empty()
                    && simple != "require"
                    && simple
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
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
