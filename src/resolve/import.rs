use std::collections::BTreeMap;

use crate::model::Language;
use crate::parser::Import;

/// Normalized representation of an import directive.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ImportDirective {
    /// Raw string from parser
    pub raw: String,
    /// Language context
    pub language: Option<Language>,
    /// Resolved target file/module path if known
    pub target_module_or_path: String,
    /// Specific imported symbol name (e.g. `foo` from `import { foo } from './bar'`) or `*`
    pub imported_symbol: Option<String>,
    /// Local alias if bound (e.g. `baz` in `import { foo as baz } from './bar'`)
    pub alias: Option<String>,
    /// Is this a wildcard / glob import? (e.g. `use foo::*`, `from foo import *`, `import * as ns`)
    pub is_wildcard: bool,
}

/// Parse and normalize language-specific import strings.
#[must_use]
pub fn parse_import_directive(import: &Import, language: Option<Language>) -> Vec<ImportDirective> {
    let raw = import.path.trim().to_string();
    if raw.is_empty() {
        return Vec::new();
    }

    match language {
        Some(Language::Rust) => parse_rust_import(&raw),
        Some(Language::Python) => parse_python_import(&raw),
        Some(Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx) => {
            parse_js_ts_import(&raw)
        }
        Some(Language::Go) => parse_go_import(&raw),
        Some(Language::C | Language::Cpp) => parse_c_cpp_import(&raw),
        Some(Language::Java) => parse_java_import(&raw),
        Some(Language::CSharp) => parse_csharp_import(&raw),
        Some(Language::Kotlin) => parse_kotlin_import(&raw),
        Some(Language::Php) => parse_php_import(&raw),
        Some(Language::Ruby) => parse_ruby_import(&raw),
        Some(Language::Swift) => parse_swift_import(&raw),
        Some(Language::Zig) => parse_zig_import(&raw),
        None => parse_generic_import(&raw),
    }
}

fn parse_rust_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw
        .trim()
        .trim_start_matches("use ")
        .trim_end_matches(';')
        .trim();
    if text.is_empty() {
        return Vec::new();
    }

    // Handles:
    // use a::b::c;
    // use a::b::{c, d as e};
    // use a::b::*;
    // use a::b as alias;
    if let Some((module, rest)) = text.split_once("::{") {
        let items = rest.trim_end_matches('}');
        let mut directives = Vec::new();
        for item in items.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if item == "*" {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Rust),
                    target_module_or_path: module.to_string(),
                    imported_symbol: Some("*".to_string()),
                    alias: None,
                    is_wildcard: true,
                });
            } else if let Some((sym, alias)) = item.split_once(" as ") {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Rust),
                    target_module_or_path: module.to_string(),
                    imported_symbol: Some(sym.trim().to_string()),
                    alias: Some(alias.trim().to_string()),
                    is_wildcard: false,
                });
            } else {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Rust),
                    target_module_or_path: module.to_string(),
                    imported_symbol: Some(item.to_string()),
                    alias: None,
                    is_wildcard: false,
                });
            }
        }
        return directives;
    }

    if text.ends_with("::*") {
        let module = text.trim_end_matches("::*");
        return vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Rust),
            target_module_or_path: module.to_string(),
            imported_symbol: Some("*".to_string()),
            alias: None,
            is_wildcard: true,
        }];
    }

    if let Some((target, alias)) = text.split_once(" as ") {
        let (module, symbol) = target.rsplit_once("::").unwrap_or(("", target));
        return vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Rust),
            target_module_or_path: module.to_string(),
            imported_symbol: Some(symbol.to_string()),
            alias: Some(alias.trim().to_string()),
            is_wildcard: false,
        }];
    }

    if let Some((module, symbol)) = text.rsplit_once("::") {
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Rust),
            target_module_or_path: module.to_string(),
            imported_symbol: Some(symbol.to_string()),
            alias: None,
            is_wildcard: false,
        }]
    } else {
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Rust),
            target_module_or_path: text.to_string(),
            imported_symbol: None,
            alias: None,
            is_wildcard: false,
        }]
    }
}

fn parse_python_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw.trim();
    // from foo import bar as baz, qux
    if let Some(rest) = text.strip_prefix("from ")
        && let Some((module, imports_part)) = rest.split_once(" import ")
    {
        let module = module.trim();
        let mut directives = Vec::new();
        for item in imports_part.trim().trim_matches(['(', ')']).split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if item == "*" {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Python),
                    target_module_or_path: module.to_string(),
                    imported_symbol: Some("*".to_string()),
                    alias: None,
                    is_wildcard: true,
                });
            } else if let Some((sym, alias)) = item.split_once(" as ") {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Python),
                    target_module_or_path: module.to_string(),
                    imported_symbol: Some(sym.trim().to_string()),
                    alias: Some(alias.trim().to_string()),
                    is_wildcard: false,
                });
            } else {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Python),
                    target_module_or_path: module.to_string(),
                    imported_symbol: Some(item.to_string()),
                    alias: None,
                    is_wildcard: false,
                });
            }
        }
        return directives;
    }

    // import foo.bar as baz
    if let Some(rest) = text.strip_prefix("import ") {
        let mut directives = Vec::new();
        for item in rest.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some((module, alias)) = item.split_once(" as ") {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Python),
                    target_module_or_path: module.trim().to_string(),
                    imported_symbol: None,
                    alias: Some(alias.trim().to_string()),
                    is_wildcard: false,
                });
            } else {
                directives.push(ImportDirective {
                    raw: raw.to_string(),
                    language: Some(Language::Python),
                    target_module_or_path: item.to_string(),
                    imported_symbol: None,
                    alias: None,
                    is_wildcard: false,
                });
            }
        }
        return directives;
    }

    vec![ImportDirective {
        raw: raw.to_string(),
        language: Some(Language::Python),
        target_module_or_path: text.to_string(),
        imported_symbol: None,
        alias: None,
        is_wildcard: false,
    }]
}

fn parse_js_ts_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw.trim().trim_end_matches(';').trim();

    // const x = require('./x') or require('./x')
    if text.contains("require(") {
        let path = text
            .split("require(")
            .nth(1)
            .and_then(|p| p.split(')').next())
            .unwrap_or("")
            .trim_matches(['\'', '"', '`'])
            .trim();
        let alias = if let Some((lhs, _)) = text.split_once('=') {
            lhs.trim_start_matches("const ")
                .trim_start_matches("let ")
                .trim_start_matches("var ")
                .trim()
                .to_string()
        } else {
            String::new()
        };
        return vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::TypeScript),
            target_module_or_path: path.to_string(),
            imported_symbol: None,
            alias: if alias.is_empty() { None } else { Some(alias) },
            is_wildcard: false,
        }];
    }

    // import ... from './path' or export ... from './path'
    let from_split = text
        .rfind(" from ")
        .map(|idx| (&text[..idx], &text[idx + 6..]));

    if let Some((left, right)) = from_split {
        let path = right.trim().trim_matches(['\'', '"', '`', ';']).trim();
        let spec = left
            .trim()
            .trim_start_matches("import ")
            .trim_start_matches("export ")
            .trim_start_matches("type ")
            .trim();

        if spec.starts_with("* as ") {
            let alias = spec.trim_start_matches("* as ").trim();
            return vec![ImportDirective {
                raw: raw.to_string(),
                language: Some(Language::TypeScript),
                target_module_or_path: path.to_string(),
                imported_symbol: Some("*".to_string()),
                alias: Some(alias.to_string()),
                is_wildcard: true,
            }];
        }

        if spec.starts_with('{') && spec.ends_with('}') {
            let inner = &spec[1..spec.len() - 1];
            let mut directives = Vec::new();
            for item in inner.split(',') {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                if let Some((sym, alias)) = item.split_once(" as ") {
                    directives.push(ImportDirective {
                        raw: raw.to_string(),
                        language: Some(Language::TypeScript),
                        target_module_or_path: path.to_string(),
                        imported_symbol: Some(sym.trim().to_string()),
                        alias: Some(alias.trim().to_string()),
                        is_wildcard: false,
                    });
                } else {
                    directives.push(ImportDirective {
                        raw: raw.to_string(),
                        language: Some(Language::TypeScript),
                        target_module_or_path: path.to_string(),
                        imported_symbol: Some(item.to_string()),
                        alias: None,
                        is_wildcard: false,
                    });
                }
            }
            return directives;
        }

        // default import: import React from 'react'
        return vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::TypeScript),
            target_module_or_path: path.to_string(),
            imported_symbol: Some("default".to_string()),
            alias: Some(spec.to_string()),
            is_wildcard: false,
        }];
    }

    // Bare import: import './style.css'
    let path = text
        .trim_start_matches("import ")
        .trim()
        .trim_matches(['\'', '"', '`', ';'])
        .trim();
    vec![ImportDirective {
        raw: raw.to_string(),
        language: Some(Language::TypeScript),
        target_module_or_path: path.to_string(),
        imported_symbol: None,
        alias: None,
        is_wildcard: false,
    }]
}

fn parse_go_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw.trim().trim_matches('"').trim();
    if let Some((alias, path)) = text.split_once(' ') {
        let path = path.trim().trim_matches('"');
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Go),
            target_module_or_path: path.to_string(),
            imported_symbol: None,
            alias: Some(alias.trim().to_string()),
            is_wildcard: alias.trim() == ".",
        }]
    } else {
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Go),
            target_module_or_path: text.to_string(),
            imported_symbol: None,
            alias: None,
            is_wildcard: false,
        }]
    }
}

fn parse_c_cpp_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw.trim().trim_matches(['<', '>', '"']).trim();
    vec![ImportDirective {
        raw: raw.to_string(),
        language: Some(Language::Cpp),
        target_module_or_path: text.to_string(),
        imported_symbol: None,
        alias: None,
        is_wildcard: false,
    }]
}

fn parse_java_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw
        .trim()
        .trim_start_matches("import ")
        .trim_end_matches(';')
        .trim();
    let is_wildcard = text.ends_with(".*");
    let clean = text.trim_end_matches(".*");
    let (pkg, sym) = clean.rsplit_once('.').unwrap_or(("", clean));
    vec![ImportDirective {
        raw: raw.to_string(),
        language: Some(Language::Java),
        target_module_or_path: if is_wildcard {
            clean.to_string()
        } else {
            pkg.to_string()
        },
        imported_symbol: if is_wildcard {
            Some("*".to_string())
        } else {
            Some(sym.to_string())
        },
        alias: None,
        is_wildcard,
    }]
}

fn parse_csharp_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw
        .trim()
        .trim_start_matches("using ")
        .trim_end_matches(';')
        .trim();
    if let Some((alias, target)) = text.split_once('=') {
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::CSharp),
            target_module_or_path: target.trim().to_string(),
            imported_symbol: None,
            alias: Some(alias.trim().to_string()),
            is_wildcard: false,
        }]
    } else {
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::CSharp),
            target_module_or_path: text.to_string(),
            imported_symbol: None,
            alias: None,
            is_wildcard: true,
        }]
    }
}

fn parse_kotlin_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw.trim().trim_start_matches("import ").trim();
    if let Some((target, alias)) = text.split_once(" as ") {
        let (pkg, sym) = target.rsplit_once('.').unwrap_or(("", target));
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Kotlin),
            target_module_or_path: pkg.to_string(),
            imported_symbol: Some(sym.trim().to_string()),
            alias: Some(alias.trim().to_string()),
            is_wildcard: false,
        }]
    } else {
        parse_java_import(raw)
    }
}

fn parse_php_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw
        .trim()
        .trim_start_matches("use ")
        .trim_end_matches(';')
        .trim();
    if let Some((target, alias)) = text.split_once(" as ") {
        let (pkg, sym) = target.rsplit_once('\\').unwrap_or(("", target));
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Php),
            target_module_or_path: pkg.to_string(),
            imported_symbol: Some(sym.trim().to_string()),
            alias: Some(alias.trim().to_string()),
            is_wildcard: false,
        }]
    } else if let Some((pkg, sym)) = text.rsplit_once('\\') {
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Php),
            target_module_or_path: pkg.to_string(),
            imported_symbol: Some(sym.trim().to_string()),
            alias: None,
            is_wildcard: false,
        }]
    } else {
        vec![ImportDirective {
            raw: raw.to_string(),
            language: Some(Language::Php),
            target_module_or_path: text.to_string(),
            imported_symbol: None,
            alias: None,
            is_wildcard: false,
        }]
    }
}

fn parse_ruby_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw
        .trim()
        .trim_start_matches("require_relative ")
        .trim_start_matches("require ")
        .trim_matches(['\'', '"'])
        .trim();
    vec![ImportDirective {
        raw: raw.to_string(),
        language: Some(Language::Ruby),
        target_module_or_path: text.to_string(),
        imported_symbol: None,
        alias: None,
        is_wildcard: false,
    }]
}

fn parse_swift_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw.trim().trim_start_matches("import ").trim();
    vec![ImportDirective {
        raw: raw.to_string(),
        language: Some(Language::Swift),
        target_module_or_path: text.to_string(),
        imported_symbol: None,
        alias: None,
        is_wildcard: true,
    }]
}

fn parse_zig_import(raw: &str) -> Vec<ImportDirective> {
    let text = raw.trim();
    let path = if let Some(idx) = text.find("@import(") {
        text[idx + 8..]
            .split(')')
            .next()
            .unwrap_or("")
            .trim_matches(['"', '\''])
    } else {
        text
    };
    vec![ImportDirective {
        raw: raw.to_string(),
        language: Some(Language::Zig),
        target_module_or_path: path.to_string(),
        imported_symbol: None,
        alias: None,
        is_wildcard: false,
    }]
}

fn parse_generic_import(raw: &str) -> Vec<ImportDirective> {
    vec![ImportDirective {
        raw: raw.to_string(),
        language: None,
        target_module_or_path: raw.trim().to_string(),
        imported_symbol: None,
        alias: None,
        is_wildcard: false,
    }]
}

pub type AliasBinding = (String, Option<String>);
pub type FileAliasMap = BTreeMap<String, Vec<AliasBinding>>;

/// Registry storing imports and aliases per file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportTable {
    /// File path -> list of parsed directives
    directives_by_file: BTreeMap<String, Vec<ImportDirective>>,
    /// File path -> local alias -> list of (target_file_or_module, Option<remote_symbol>)
    aliases_by_file: BTreeMap<String, FileAliasMap>,
    /// File path -> list of resolved target file paths
    resolved_files_by_file: BTreeMap<String, Vec<String>>,
}

impl ImportTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register imports for a file.
    pub fn register_file_imports(
        &mut self,
        file: &str,
        language: Option<Language>,
        imports: &[Import],
    ) {
        let mut directives = Vec::new();
        let mut aliases: BTreeMap<String, Vec<(String, Option<String>)>> = BTreeMap::new();

        for imp in imports {
            let parsed = parse_import_directive(imp, language);
            for d in &parsed {
                if let Some(alias) = &d.alias {
                    aliases
                        .entry(alias.clone())
                        .or_default()
                        .push((d.target_module_or_path.clone(), d.imported_symbol.clone()));
                } else if let Some(sym) = &d.imported_symbol
                    && !d.is_wildcard
                {
                    aliases
                        .entry(sym.clone())
                        .or_default()
                        .push((d.target_module_or_path.clone(), Some(sym.clone())));
                }
            }
            directives.extend(parsed);
        }

        self.directives_by_file.insert(file.to_string(), directives);
        self.aliases_by_file.insert(file.to_string(), aliases);
    }

    /// Set resolved target files for a file.
    pub fn set_resolved_files(&mut self, file: &str, target_files: Vec<String>) {
        self.resolved_files_by_file
            .insert(file.to_string(), target_files);
    }

    /// Get directives for a file.
    #[must_use]
    pub fn directives(&self, file: &str) -> Option<&[ImportDirective]> {
        self.directives_by_file.get(file).map(|v| v.as_slice())
    }

    /// Get alias bindings for a symbol in a file if they exist.
    #[must_use]
    pub fn resolve_alias(&self, file: &str, name: &str) -> Option<&[(String, Option<String>)]> {
        self.aliases_by_file
            .get(file)?
            .get(name)
            .map(|v| v.as_slice())
    }

    /// Get resolved imported files for a given file.
    #[must_use]
    pub fn resolved_files(&self, file: &str) -> Option<&[String]> {
        self.resolved_files_by_file.get(file).map(|v| v.as_slice())
    }
}
