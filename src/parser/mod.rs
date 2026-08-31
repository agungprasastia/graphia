use serde::{Deserialize, Serialize};
use tree_sitter::{Language, Parser, Tree};

use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation, Visibility};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub location: SourceLocation,
    pub parent: Option<String>,
    #[serde(default)]
    pub visibility: Visibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Definition {
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub location: SourceLocation,
    pub container: Option<String>,
    pub visibility: Visibility,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reference {
    pub name: String,
    pub location: SourceLocation,
    pub caller: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeReference {
    pub name: String,
    pub location: SourceLocation,
    pub container: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Export {
    pub name: String,
    pub location: SourceLocation,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Import {
    pub path: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Call {
    pub caller: String,
    pub callee: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instantiation {
    pub type_name: String,
    pub location: SourceLocation,
    pub caller: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inheritance {
    pub derived_type: String,
    pub base_type: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    pub implementing_type: String,
    pub trait_or_interface: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ParsedFile {
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub calls: Vec<Call>,
    #[serde(default)]
    pub definitions: Vec<Definition>,
    #[serde(default)]
    pub references: Vec<Reference>,
    #[serde(default)]
    pub exports: Vec<Export>,
    #[serde(default)]
    pub type_references: Vec<TypeReference>,
    #[serde(default)]
    pub instantiations: Vec<Instantiation>,
    #[serde(default)]
    pub inheritances: Vec<Inheritance>,
    #[serde(default)]
    pub implementations: Vec<Implementation>,
}

pub fn normalize_signature(sig: &str) -> String {
    let mut normalized = String::with_capacity(sig.len());
    let mut in_whitespace = false;
    for ch in sig.chars() {
        if ch.is_whitespace() {
            if !in_whitespace {
                in_whitespace = true;
            }
        } else {
            if in_whitespace
                && !matches!(
                    ch,
                    '(' | ')' | '{' | '}' | '[' | ']' | ',' | ':' | ';' | '-' | '>' | '<'
                )
                && !normalized.ends_with(|c: char| {
                    matches!(
                        c,
                        '(' | ')' | '{' | '}' | '[' | ']' | ',' | ':' | ';' | '-' | '>' | '<'
                    )
                })
            {
                normalized.push(' ');
            }
            in_whitespace = false;
            normalized.push(ch);
        }
    }
    normalized
        .replace(" -> ", "->")
        .replace(": ", ":")
        .replace(", ", ",")
}
pub(crate) fn ts_language(lang: GraphiaLanguage) -> Language {
    match lang {
        GraphiaLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        GraphiaLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        GraphiaLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        GraphiaLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        GraphiaLanguage::JavaScript | GraphiaLanguage::Jsx => {
            tree_sitter_javascript::LANGUAGE.into()
        }
        GraphiaLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        GraphiaLanguage::C => tree_sitter_c::LANGUAGE.into(),
        GraphiaLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        GraphiaLanguage::Java => tree_sitter_java::LANGUAGE.into(),
        GraphiaLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        GraphiaLanguage::Kotlin => tree_sitter_kotlin::LANGUAGE.into(),
        GraphiaLanguage::Zig => tree_sitter_zig::LANGUAGE.into(),
        GraphiaLanguage::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        GraphiaLanguage::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        GraphiaLanguage::Swift => tree_sitter_swift::LANGUAGE.into(),
    }
}

fn location_for_node(file: &str, node: &tree_sitter::Node<'_>) -> SourceLocation {
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

fn node_text<'a>(node: &tree_sitter::Node<'_>, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

fn parse_tree(lang: GraphiaLanguage, source: &[u8]) -> Option<Tree> {
    let mut parser = Parser::new();
    let ts_lang = ts_language(lang);
    if let Err(error) = parser.set_language(&ts_lang) {
        eprintln!("set language failed: {error:?}");
        return None;
    }
    parser.parse(source, None)
}

#[must_use]
pub fn parse_file(path: &str, lang: GraphiaLanguage, content: &str) -> ParsedFile {
    parse_bytes(path, lang, content.as_bytes()).expect("validated UTF-8")
}

pub fn parse_bytes(
    path: &str,
    lang: GraphiaLanguage,
    source: &[u8],
) -> crate::error::Result<ParsedFile> {
    if std::str::from_utf8(source).is_err() {
        return Err(crate::error::GraphiaError::Parse {
            file: path.to_string(),
            message: "invalid UTF-8".to_string(),
        });
    }
    let Some(tree) = parse_tree(lang, source) else {
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
    let mut parsed = match lang {
        GraphiaLanguage::Rust => parse_rust(path, &root, source),
        GraphiaLanguage::Python => parse_python(path, &root, source),
        GraphiaLanguage::TypeScript
        | GraphiaLanguage::Tsx
        | GraphiaLanguage::JavaScript
        | GraphiaLanguage::Jsx => crate::parse::javascript::parse_js_family(path, &root, source),
        GraphiaLanguage::Go => crate::parse::golang::parse_go(path, &root, source),
        GraphiaLanguage::C | GraphiaLanguage::Cpp => {
            crate::parse::c_cpp::parse_c_cpp(path, &root, source)
        }
        GraphiaLanguage::Java => crate::parse::java::parse_java(path, &root, source),
        GraphiaLanguage::CSharp => crate::parse::csharp::parse_csharp(path, &root, source),
        GraphiaLanguage::Kotlin => crate::parse::kotlin::parse_kotlin(path, &root, source),
        GraphiaLanguage::Zig => crate::parse::zig::parse_zig(path, &root, source),
        GraphiaLanguage::Php => crate::parse::php::parse_php(path, &root, source),
        GraphiaLanguage::Ruby => crate::parse::ruby::parse_ruby(path, &root, source),
        GraphiaLanguage::Swift => crate::parse::swift::parse_swift(path, &root, source),
    };
    extract_relationships(path, lang, &root, source, &mut parsed);
    Ok(parsed)
}

fn extract_relationships(
    file: &str,
    _lang: GraphiaLanguage,
    root: &tree_sitter::Node<'_>,
    source: &[u8],
    parsed: &mut ParsedFile,
) {
    let mut stack = vec![(*root, None::<String>)];
    while let Some((node, caller)) = stack.pop() {
        let text = node_text(&node, source).trim();
        let kind = node.kind();

        if matches!(
            kind,
            "new_expression" | "object_creation_expression" | "constructor_expression"
        ) {
            if let Some(type_node) = node
                .child_by_field_name("type")
                .or_else(|| node.child_by_field_name("constructor"))
            {
                let type_name = node_text(&type_node, source).trim().to_string();
                if !type_name.is_empty() {
                    parsed.instantiations.push(Instantiation {
                        type_name,
                        location: location_for_node(file, &node),
                        caller: caller.clone(),
                    });
                }
            }
        } else if matches!(kind, "struct_expression" | "composite_literal") {
            if let Some(type_node) = node.child_by_field_name("type") {
                let type_name = node_text(&type_node, source).trim().to_string();
                if !type_name.is_empty() {
                    parsed.instantiations.push(Instantiation {
                        type_name,
                        location: location_for_node(file, &node),
                        caller: caller.clone(),
                    });
                }
            }
        }
        if matches!(kind, "call_expression" | "call") && text.contains("::new(") {
            if let Some(type_name) = text
                .split("::new(")
                .next()
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                parsed.instantiations.push(Instantiation {
                    type_name: type_name.to_string(),
                    location: location_for_node(file, &node),
                    caller: caller.clone(),
                });
            }
        }
        if matches!(kind, "call" | "call_expression" | "expression_statement") {
            let candidate = text.split(['(', '{']).next().unwrap_or(text).trim();
            if !candidate.is_empty()
                && candidate
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_uppercase())
                && !text.starts_with("class ")
            {
                parsed.instantiations.push(Instantiation {
                    type_name: candidate
                        .rsplit(['.', ':'])
                        .next()
                        .unwrap_or(candidate)
                        .to_string(),
                    location: location_for_node(file, &node),
                    caller: caller.clone(),
                });
            }
        }

        if matches!(
            kind,
            "class_declaration"
                | "class_definition"
                | "class_specifier"
                | "interface_declaration"
                | "object_declaration"
        ) {
            let derived = node
                .child_by_field_name("name")
                .map(|n| node_text(&n, source).to_string());
            if let Some(derived_type) = derived {
                let header = text
                    .split('{')
                    .next()
                    .unwrap_or(text)
                    .split(':')
                    .next()
                    .unwrap_or(text);
                let relation = if header.contains(" implements ") {
                    " implements "
                } else if header.contains(" extends ") {
                    " extends "
                } else if header.contains(" : ") {
                    " : "
                } else {
                    ""
                };
                if !relation.is_empty() {
                    let bases = header
                        .split_once(relation)
                        .map(|(_, rest)| rest)
                        .unwrap_or("");
                    for base in bases
                        .split(',')
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                    {
                        let base = base
                            .split_whitespace()
                            .next()
                            .unwrap_or(base)
                            .trim_matches(['<', '>']);
                        if base.is_empty() {
                            continue;
                        }
                        let item = location_for_node(file, &node);
                        if relation.contains("implements") {
                            parsed.implementations.push(Implementation {
                                implementing_type: derived_type.clone(),
                                trait_or_interface: base.to_string(),
                                location: item,
                            });
                        } else {
                            parsed.inheritances.push(Inheritance {
                                derived_type: derived_type.clone(),
                                base_type: base.to_string(),
                                location: item,
                            });
                        }
                    }
                }
            }
        }
        if kind == "impl_item" {
            let header = text.split('{').next().unwrap_or(text);
            if let Some((trait_name, type_name)) = header
                .strip_prefix("impl ")
                .and_then(|s| s.split_once(" for "))
            {
                parsed.implementations.push(Implementation {
                    implementing_type: type_name.trim().to_string(),
                    trait_or_interface: trait_name.trim().to_string(),
                    location: location_for_node(file, &node),
                });
            }
        }
        for child in children_vec(&node).into_iter().rev() {
            let next_caller = if matches!(
                kind,
                "function_item"
                    | "function_definition"
                    | "method_definition"
                    | "function_declaration"
            ) {
                node.child_by_field_name("name")
                    .map(|n| node_text(&n, source).to_string())
            } else {
                caller.clone()
            };
            stack.push((child, next_caller));
        }
    }
}

fn children_vec<'a>(node: &tree_sitter::Node<'a>) -> Vec<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn parse_rust(file: &str, root: &tree_sitter::Node<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let mut type_references = Vec::new();
    let mut stack: Vec<tree_sitter::Node<'_>> = vec![*root];

    while let Some(node) = stack.pop() {
        match node.kind() {
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let is_pub = node_text(&node, source).trim_start().starts_with("pub");
                    let vis = if is_pub {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    };

                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let return_type = node
                        .child_by_field_name("return_type")
                        .map(|r| node_text(&r, source).trim_start_matches("->").trim())
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
                        visibility: vis,
                        signature: sig.clone(),
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: vis,
                        signature: sig,
                    });

                    if is_pub {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified.clone()),
                        });
                    }

                    if let Some(params_node) = node.child_by_field_name("parameters") {
                        for p in children_vec(&params_node) {
                            if p.kind() == "parameter" {
                                if let Some(type_node) = p.child_by_field_name("type") {
                                    let tname = node_text(&type_node, source).trim().to_string();
                                    if !tname.is_empty() {
                                        type_references.push(TypeReference {
                                            name: tname,
                                            location: location_for_node(file, &type_node),
                                            container: Some(qualified.clone()),
                                        });
                                    }
                                }
                            }
                        }
                    }

                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_rust(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let is_pub = node_text(&node, source).trim_start().starts_with("pub");
                    let vis = if is_pub {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    };
                    symbols.push(Symbol {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: vis,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Struct,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: vis,
                        signature: None,
                    });
                    if is_pub {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified.clone()),
                        });
                    }
                }
            }
            "enum_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let is_pub = node_text(&node, source).trim_start().starts_with("pub");
                    let vis = if is_pub {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    };
                    symbols.push(Symbol {
                        kind: NodeKind::Enum,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: vis,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Enum,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: vis,
                        signature: None,
                    });
                    if is_pub {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified.clone()),
                        });
                    }
                }
            }
            "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let is_pub = node_text(&node, source).trim_start().starts_with("pub");
                    let vis = if is_pub {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    };
                    symbols.push(Symbol {
                        kind: NodeKind::Trait,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: vis,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Trait,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: vis,
                        signature: None,
                    });
                    if is_pub {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified.clone()),
                        });
                    }
                }
            }
            "mod_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let is_pub = node_text(&node, source).trim_start().starts_with("pub");
                    let vis = if is_pub {
                        Visibility::Public
                    } else {
                        Visibility::Private
                    };
                    symbols.push(Symbol {
                        kind: NodeKind::Module,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: vis,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Module,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: vis,
                        signature: None,
                    });
                    if is_pub {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified.clone()),
                        });
                    }
                }
            }
            "use_declaration" => {
                let text = node_text(&node, source)
                    .trim()
                    .trim_end_matches(';')
                    .to_string();
                let is_pub = text.starts_with("pub use");
                let path = if let Some(stripped) = text.strip_prefix("pub use") {
                    stripped.trim().trim_end_matches(';').trim().to_string()
                } else if let Some(stripped) = text.strip_prefix("use ") {
                    stripped.trim().trim_end_matches(';').trim().to_string()
                } else {
                    text.trim_start_matches("use")
                        .trim()
                        .trim_end_matches(';')
                        .trim()
                        .to_string()
                };
                let loc = location_for_node(file, &node);
                if !path.is_empty() {
                    imports.push(Import {
                        path: path.clone(),
                        location: loc.clone(),
                    });
                    if is_pub {
                        let exported_symbol = path.rsplit("::").next().unwrap_or(&path).to_string();
                        exports.push(Export {
                            name: exported_symbol,
                            location: loc.clone(),
                            target: Some(path),
                        });
                    }
                }
            }
            "impl_item" => {
                let type_name = node
                    .child_by_field_name("type")
                    .map(|n| node_text(&n, source).to_string());
                if let Some(tname) = type_name {
                    for child in children_vec(&node) {
                        if child.kind() == "declaration_list" {
                            for sub in children_vec(&child) {
                                if sub.kind() == "function_item" {
                                    if let Some(name_node) = sub.child_by_field_name("name") {
                                        let name = node_text(&name_node, source).to_string();
                                        let qualified = format!("{file}::{name}");
                                        let loc = location_for_node(file, &sub);
                                        let is_pub =
                                            node_text(&sub, source).trim_start().starts_with("pub");
                                        let vis = if is_pub {
                                            Visibility::Public
                                        } else {
                                            Visibility::Private
                                        };

                                        let params_text = sub
                                            .child_by_field_name("parameters")
                                            .map(|p| node_text(&p, source))
                                            .unwrap_or("()");
                                        let return_type = sub
                                            .child_by_field_name("return_type")
                                            .map(|r| {
                                                node_text(&r, source)
                                                    .trim_start_matches("->")
                                                    .trim()
                                            })
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
                                            parent: Some(tname.clone()),
                                            visibility: vis,
                                            signature: sig.clone(),
                                            container: Some(tname.clone()),
                                        });
                                        definitions.push(Definition {
                                            kind: NodeKind::Method,
                                            name: name.clone(),
                                            qualified_name: qualified.clone(),
                                            location: loc.clone(),
                                            container: Some(tname.clone()),
                                            visibility: vis,
                                            signature: sig,
                                        });
                                        if is_pub {
                                            exports.push(Export {
                                                name: name.clone(),
                                                location: loc.clone(),
                                                target: Some(qualified.clone()),
                                            });
                                        }
                                        if let Some(body) = sub.child_by_field_name("body") {
                                            extract_calls_rust(
                                                file, &body, source, &qualified, &mut calls,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                    continue;
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
        if node.kind() != "impl_item" {
            for child in children_vec(&node).into_iter().rev() {
                stack.push(child);
            }
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

fn extract_calls_rust(
    file: &str,
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call_expression" {
            if let Some(func) = n.child_by_field_name("function") {
                let called_text = node_text(&func, source).trim().to_string();
                let simple = called_text
                    .rsplit("::")
                    .next()
                    .unwrap_or(&called_text)
                    .rsplit('.')
                    .next()
                    .unwrap_or(&called_text)
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

fn parse_python(file: &str, root: &tree_sitter::Node<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut definitions = Vec::new();
    let mut references = Vec::new();
    let mut exports = Vec::new();
    let mut type_references = Vec::new();
    let mut stack: Vec<(tree_sitter::Node<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_class)) = stack.pop() {
        match node.kind() {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let is_method = parent_class.is_some();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let vis = if name.starts_with('_') && !name.starts_with("__") {
                        Visibility::Private
                    } else {
                        Visibility::Public
                    };
                    let params_text = node
                        .child_by_field_name("parameters")
                        .map(|p| node_text(&p, source))
                        .unwrap_or("()");
                    let return_type = node
                        .child_by_field_name("return_type")
                        .map(|r| node_text(&r, source).trim_start_matches("->").trim())
                        .unwrap_or("");
                    let raw_sig = if return_type.is_empty() {
                        format!("{name}{params_text}")
                    } else {
                        format!("{name}{params_text}->{return_type}")
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
                        parent: parent_class.clone(),
                        visibility: vis,
                        signature: sig.clone(),
                        container: parent_class.clone(),
                    });
                    definitions.push(Definition {
                        kind,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: parent_class.clone(),
                        visibility: vis,
                        signature: sig,
                    });
                    if !is_method && vis == Visibility::Public {
                        exports.push(Export {
                            name: name.clone(),
                            location: loc.clone(),
                            target: Some(qualified.clone()),
                        });
                    }
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_python(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    let vis = if name.starts_with('_') {
                        Visibility::Private
                    } else {
                        Visibility::Public
                    };
                    symbols.push(Symbol {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                        visibility: vis,
                        signature: None,
                        container: None,
                    });
                    definitions.push(Definition {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        container: None,
                        visibility: vis,
                        signature: None,
                    });
                    if vis == Visibility::Public {
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
                        continue;
                    }
                }
            }
            "type" => {
                let name = node_text(&node, source).to_string();
                if !name.is_empty() {
                    type_references.push(TypeReference {
                        name,
                        location: location_for_node(file, &node),
                        container: parent_class.clone(),
                    });
                }
            }
            "import_statement" | "import_from_statement" => {
                let text = node_text(&node, source).trim().to_string();
                let loc = location_for_node(file, &node);
                imports.push(Import {
                    path: text,
                    location: loc,
                });
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
        if node.kind() == "class_definition" {
            continue;
        }
        let next_parent = if node.kind() == "block" {
            parent_class.clone()
        } else {
            None
        };
        for child in children_vec(&node).into_iter().rev() {
            stack.push((child, next_parent.clone()));
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

fn extract_calls_python(
    file: &str,
    node: &tree_sitter::Node<'_>,
    source: &[u8],
    caller: &str,
    calls: &mut Vec<Call>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "call" {
            if let Some(func) = n.child_by_field_name("function") {
                let callee_raw = node_text(&func, source).trim().to_string();
                let simple = callee_raw
                    .rsplit('.')
                    .next()
                    .unwrap_or(&callee_raw)
                    .to_string();
                if !simple.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeKind;

    #[test]
    fn rust_extracts_function_and_struct() {
        let code = "struct Foo; fn bar() { bar(); }";
        let pf = parse_file("src/main.rs", GraphiaLanguage::Rust, code);
        assert!(
            pf.symbols
                .iter()
                .any(|s| s.name == "Foo" && s.kind == NodeKind::Struct)
        );
        assert!(
            pf.symbols
                .iter()
                .any(|s| s.name == "bar" && s.kind == NodeKind::Function)
        );
    }

    #[test]
    fn python_extracts_class_and_method() {
        let code = "class Foo:\n    def method(self):\n        pass\ndef func():\n    pass";
        let pf = parse_file("a.py", GraphiaLanguage::Python, code);
        assert!(
            pf.symbols
                .iter()
                .any(|s| s.name == "Foo" && s.kind == NodeKind::Class)
        );
        assert!(
            pf.symbols
                .iter()
                .any(|s| s.name == "method" && s.kind == NodeKind::Method)
        );
        assert!(
            pf.symbols
                .iter()
                .any(|s| s.name == "func" && s.kind == NodeKind::Function)
        );
    }

    #[test]
    fn typescript_extracts_class_and_interface() {
        let code = "interface IFoo {} class Foo implements IFoo { method() {} } function bar() {}";
        let pf = parse_file("a.ts", GraphiaLanguage::TypeScript, code);
        assert!(
            pf.symbols
                .iter()
                .any(|s| s.name == "IFoo" && s.kind == NodeKind::Interface)
        );
        assert!(
            pf.symbols
                .iter()
                .any(|s| s.name == "Foo" && s.kind == NodeKind::Class)
        );
    }
}
