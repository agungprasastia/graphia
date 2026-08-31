use serde::{Deserialize, Serialize};

use crate::graph::Graph;
use crate::model::{Language, Node, NodeKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntrypointKind {
    MainFunction,
    CliCommand,
    GuardedScript,
    WebHandler,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entrypoint {
    pub node: Node,
    pub kind: EntrypointKind,
    pub description: String,
}

#[must_use]
pub fn detect_entrypoints(graph: &Graph) -> Vec<Entrypoint> {
    let mut entrypoints = Vec::new();

    for node in &graph.nodes {
        if node.kind == NodeKind::File {
            continue;
        }

        let name = node.name.as_str();
        let lang = node.language.or_else(|| infer_language(&node.file));

        match lang {
            Some(Language::Rust) => {
                if name == "main" && node.kind == NodeKind::Function {
                    entrypoints.push(Entrypoint {
                        node: node.clone(),
                        kind: EntrypointKind::MainFunction,
                        description: "Rust fn main() program entrypoint".to_string(),
                    });
                }
            }
            Some(Language::Go) => {
                if name == "main" && node.kind == NodeKind::Function {
                    entrypoints.push(Entrypoint {
                        node: node.clone(),
                        kind: EntrypointKind::MainFunction,
                        description: "Go func main() package main entrypoint".to_string(),
                    });
                }
            }
            Some(Language::C | Language::Cpp) => {
                if name == "main" && node.kind == NodeKind::Function {
                    entrypoints.push(Entrypoint {
                        node: node.clone(),
                        kind: EntrypointKind::MainFunction,
                        description: "C/C++ int main() executable entrypoint".to_string(),
                    });
                }
            }
            Some(Language::Java | Language::CSharp | Language::Kotlin) => {
                if name.eq_ignore_ascii_case("main")
                    && (node.kind == NodeKind::Method || node.kind == NodeKind::Function)
                {
                    entrypoints.push(Entrypoint {
                        node: node.clone(),
                        kind: EntrypointKind::MainFunction,
                        description: "JVM/CLR static main() application entrypoint".to_string(),
                    });
                }
            }
            Some(Language::Python) => {
                if name == "main" {
                    entrypoints.push(Entrypoint {
                        node: node.clone(),
                        kind: EntrypointKind::GuardedScript,
                        description: "Python main entrypoint / script guard".to_string(),
                    });
                }
            }
            _ => {
                if name == "main"
                    && (node.kind == NodeKind::Function || node.kind == NodeKind::Method)
                {
                    entrypoints.push(Entrypoint {
                        node: node.clone(),
                        kind: EntrypointKind::MainFunction,
                        description: "Program main() entrypoint".to_string(),
                    });
                }
            }
        }

        // Detect CLI / Command handlers (e.g. run, execute, cli, handle)
        let is_cli = name.starts_with("cmd_")
            || name.starts_with("command_")
            || name.ends_with("_cmd")
            || name.ends_with("_command")
            || (name == "run" && node.file.contains("cli"));

        if is_cli && (node.kind == NodeKind::Function || node.kind == NodeKind::Method) {
            entrypoints.push(Entrypoint {
                node: node.clone(),
                kind: EntrypointKind::CliCommand,
                description: format!("CLI command handler: {name}"),
            });
        }
    }

    entrypoints.sort_by(|a, b| {
        a.node
            .qualified_name
            .cmp(&b.node.qualified_name)
            .then_with(|| a.node.id.0.cmp(&b.node.id.0))
    });
    entrypoints.dedup_by(|a, b| a.node.id == b.node.id && a.kind == b.kind);

    entrypoints
}

fn infer_language(file: &str) -> Option<Language> {
    let lower = file.to_lowercase();
    if lower.ends_with(".rs") {
        Some(Language::Rust)
    } else if lower.ends_with(".py") {
        Some(Language::Python)
    } else if lower.ends_with(".ts") || lower.ends_with(".mts") || lower.ends_with(".cts") {
        Some(Language::TypeScript)
    } else if lower.ends_with(".tsx") {
        Some(Language::Tsx)
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs") {
        Some(Language::JavaScript)
    } else if lower.ends_with(".jsx") {
        Some(Language::Jsx)
    } else if lower.ends_with(".go") {
        Some(Language::Go)
    } else if lower.ends_with(".c") || lower.ends_with(".h") {
        Some(Language::C)
    } else if lower.ends_with(".cpp")
        || lower.ends_with(".cc")
        || lower.ends_with(".cxx")
        || lower.ends_with(".hpp")
    {
        Some(Language::Cpp)
    } else if lower.ends_with(".java") {
        Some(Language::Java)
    } else if lower.ends_with(".cs") {
        Some(Language::CSharp)
    } else if lower.ends_with(".kt") || lower.ends_with(".kts") {
        Some(Language::Kotlin)
    } else if lower.ends_with(".zig") {
        Some(Language::Zig)
    } else if lower.ends_with(".php") {
        Some(Language::Php)
    } else if lower.ends_with(".rb") {
        Some(Language::Ruby)
    } else if lower.ends_with(".swift") {
        Some(Language::Swift)
    } else {
        None
    }
}
