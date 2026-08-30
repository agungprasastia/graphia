use tree_sitter::{Language, Parser, Tree};

use crate::model::{Language as GraphiaLanguage, NodeKind, SourceLocation};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub location: SourceLocation,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub path: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub caller: String,
    pub callee: String,
    pub location: SourceLocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    pub symbols: Vec<Symbol>,
    pub imports: Vec<Import>,
    pub calls: Vec<Call>,
}

fn ts_language(lang: GraphiaLanguage) -> Language {
    match lang {
        GraphiaLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        GraphiaLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        GraphiaLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
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
    let source = content.as_bytes();
    let Some(tree) = parse_tree(lang, source) else {
        return ParsedFile {
            symbols: Vec::new(),
            imports: Vec::new(),
            calls: Vec::new(),
        };
    };
    let root = tree.root_node();
    match lang {
        GraphiaLanguage::Rust => parse_rust(path, &root, source),
        GraphiaLanguage::Python => parse_python(path, &root, source),
        GraphiaLanguage::TypeScript => parse_typescript(path, &root, source),
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
    let mut stack: Vec<tree_sitter::Node<'_>> = vec![*root];

    while let Some(node) = stack.pop() {
        match node.kind() {
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc.clone(),
                        parent: None,
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_rust(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "struct_item" | "enum_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Struct,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        parent: None,
                    });
                }
            }
            "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Trait,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        parent: None,
                    });
                }
            }
            "mod_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Module,
                        name,
                        qualified_name: qualified,
                        location: loc,
                        parent: None,
                    });
                }
            }
            "use_declaration" => {
                let text = node_text(&node, source).trim().to_string();
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
                                        let qualified = format!("{file}::{tname}::{name}");
                                        let loc = location_for_node(file, &sub);
                                        symbols.push(Symbol {
                                            kind: NodeKind::Method,
                                            name: name.clone(),
                                            qualified_name: qualified.clone(),
                                            location: loc.clone(),
                                            parent: Some(tname.clone()),
                                        });
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
    let mut stack: Vec<(tree_sitter::Node<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_class)) = stack.pop() {
        match node.kind() {
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let is_method = parent_class.is_some();
                    let qualified = if let Some(ref p) = parent_class {
                        format!("{file}::{p}::{name}")
                    } else {
                        format!("{file}::{name}")
                    };
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
                        parent: parent_class.clone(),
                    });
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
                    symbols.push(Symbol {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: None,
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
                    }
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

fn parse_typescript(file: &str, root: &tree_sitter::Node<'_>, source: &[u8]) -> ParsedFile {
    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut calls = Vec::new();
    let mut stack: Vec<(tree_sitter::Node<'_>, Option<String>)> = vec![(*root, None)];

    while let Some((node, parent_class)) = stack.pop() {
        match node.kind() {
            "function_declaration" | "function" | "generator_function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Function,
                        name: name.clone(),
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: None,
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_ts(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = if let Some(ref p) = parent_class {
                        format!("{file}::{p}::{name}")
                    } else {
                        format!("{file}::{name}")
                    };
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Method,
                        name,
                        qualified_name: qualified.clone(),
                        location: loc,
                        parent: parent_class.clone(),
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        extract_calls_ts(file, &body, source, &qualified, &mut calls);
                    }
                }
            }
            "class_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(&name_node, source).to_string();
                    let qualified = format!("{file}::{name}");
                    let loc = location_for_node(file, &node);
                    symbols.push(Symbol {
                        kind: NodeKind::Class,
                        name: name.clone(),
                        qualified_name: qualified,
                        location: loc,
                        parent: None,
                    });
                    if let Some(body) = node.child_by_field_name("body") {
                        for child in children_vec(&body).into_iter().rev() {
                            stack.push((child, Some(name.clone())));
                        }
                        continue;
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
                        name,
                        qualified_name: qualified,
                        location: loc,
                        parent: None,
                    });
                }
            }
            "import_statement" => {
                let text = node_text(&node, source).trim().to_string();
                let loc = location_for_node(file, &node);
                imports.push(Import {
                    path: text,
                    location: loc,
                });
            }
            _ => {}
        }
        if node.kind() == "class_declaration" || node.kind() == "class_body" {
            continue;
        }
        for child in children_vec(&node).into_iter().rev() {
            stack.push((child, parent_class.clone()));
        }
    }

    ParsedFile {
        symbols,
        imports,
        calls,
    }
}

fn extract_calls_ts(
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
