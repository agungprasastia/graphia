use tree_sitter::{Node as TsNode, Parser, Tree};

use crate::error::{GraphiaError, Result};
use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};
use crate::parse::analyzer::LanguageAnalyzer;
use crate::parser::{
    Call, Definition, Export, Import, ParsedFile, Reference, Symbol, TypeReference,
    normalize_signature,
};

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
            _ => tree_sitter_c::LANGUAGE.into(),
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
                instantiations: Vec::new(),
                inheritances: Vec::new(),
                implementations: Vec::new(),
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
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let mut type_references = Vec::new();
    let mut stack: Vec<(TsNode<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_scope)) = stack.pop() {
        match node.kind() {
            "preproc_include" => {
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
            "struct_specifier" => {
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
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Struct,
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
            "type_definition" | "alias_declaration" => {
                let mut alias_name = None;
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    if declarator.kind() == "type_identifier" || declarator.kind() == "identifier" {
                        alias_name = Some(node_text(&declarator, source).to_string());
                    }
                }
                if alias_name.is_none() {
                    if let Some(name_node) = node.child_by_field_name("name") {
                        alias_name = Some(node_text(&name_node, source).to_string());
                    }
                }
                if let Some(name) = alias_name {
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::TypeAlias,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: parent_scope.clone(),
                        visibility: Visibility::Public,
                        signature: None,
                        container: parent_scope.clone(),
                    });
                    definitions.push(Definition {
                        kind: NodeKind::TypeAlias,
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
            "function_definition" => {
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    let mut fn_name = None;
                    let mut params_str = String::from("()");
                    let mut scope_prefix = None;

                    let mut decl_stack = vec![declarator];
                    while let Some(d) = decl_stack.pop() {
                        if d.kind() == "function_declarator" {
                            if let Some(params_node) = d.child_by_field_name("parameters") {
                                params_str = node_text(&params_node, source).to_string();
                            }
                            if let Some(sub_decl) = d.child_by_field_name("declarator") {
                                decl_stack.push(sub_decl);
                            }
                        } else if d.kind() == "qualified_identifier"
                            || d.kind() == "scoped_identifier"
                        {
                            let text = node_text(&d, source).to_string();
                            if let Some((scope, n)) = text.rsplit_once("::") {
                                scope_prefix = Some(scope.to_string());
                                fn_name = Some(n.to_string());
                            } else {
                                fn_name = Some(text);
                            }
                        } else if d.kind() == "identifier" || d.kind() == "field_identifier" {
                            fn_name = Some(node_text(&d, source).to_string());
                        } else {
                            for child in children_vec(&d).into_iter().rev() {
                                decl_stack.push(child);
                            }
                        }
                    }

                    if let Some(name) = fn_name {
                        let final_parent = scope_prefix.or_else(|| parent_scope.clone());
                        let is_method = final_parent.is_some();
                        let qualified = format!("{file}::{name}");
                        let loc = location_for_node(file, &node);

                        let ret_type = node
                            .child_by_field_name("type")
                            .map(|t| node_text(&t, source).trim())
                            .unwrap_or("");
                        let raw_sig = if ret_type.is_empty() {
                            format!("{name}{params_str}")
                        } else {
                            format!("{name}{params_str}->{ret_type}")
                        };
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
                            parent: final_parent.clone(),
                            visibility: Visibility::Public,
                            signature: sig.clone(),
                            container: final_parent.clone(),
                        });
                        definitions.push(Definition {
                            kind,
                            name: name.clone(),
                            qualified_name: qualified.clone(),
                            location: loc.clone(),
                            container: final_parent,
                            visibility: Visibility::Public,
                            signature: sig,
                        });
                        exports.push(Export {
                            name: name.clone(),
                            location: loc,
                            target: Some(qualified.clone()),
                        });
                        if let Some(body) = node.child_by_field_name("body") {
                            extract_calls_c_cpp(file, &body, source, &qualified, &mut calls);
                        }
                    }
                }
            }
            "type_identifier" => {
                let name = node_text(&node, source).to_string();
                if !name.is_empty() {
                    type_references.push(TypeReference {
                        name,
                        location: location_for_node(file, &node),
                        container: parent_scope.clone(),
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

        if node.kind() == "function_definition" {
            continue;
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
                    .rsplit("->")
                    .next()
                    .unwrap_or(&callee_raw)
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
        for child in children_vec(&n).into_iter().rev() {
            stack.push(child);
        }
    }
}
