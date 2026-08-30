use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FileId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeIdentity {
    pub file: String,
    pub kind: NodeKind,
    pub qualified_name: String,
    pub location: SourceLocation,
}

impl NodeIdentity {
    #[must_use]
    pub fn new(
        file: &str,
        kind: NodeKind,
        qualified_name: &str,
        location: &SourceLocation,
    ) -> Self {
        Self {
            file: normalize_identity_path(file),
            kind,
            qualified_name: qualified_name.to_string(),
            location: SourceLocation {
                file: normalize_identity_path(&location.file),
                ..location.clone()
            },
        }
    }

    #[must_use]
    pub fn from_node(node: &Node) -> Self {
        Self::new(&node.file, node.kind, &node.qualified_name, &node.location)
    }
}

fn normalize_identity_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.split_once("/repo/") {
        Some((_, relative)) => relative.to_string(),
        None => normalized,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeIdentity {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub confidence: Confidence,
    pub label: Option<String>,
}

impl EdgeIdentity {
    #[must_use]
    pub fn new(
        from: NodeId,
        to: NodeId,
        kind: EdgeKind,
        confidence: Confidence,
        label: Option<String>,
    ) -> Self {
        Self {
            from,
            to,
            kind,
            confidence,
            label,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Language {
    Rust,
    Python,
    TypeScript,
}

impl Language {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::TypeScript => "typescript",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Rust => 1,
            Self::Python => 2,
            Self::TypeScript => 3,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Rust),
            2 => Some(Self::Python),
            3 => Some(Self::TypeScript),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Module,
    Function,
    Method,
    Class,
    Struct,
    Trait,
    Interface,
}

impl NodeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Module => "Module",
            Self::Function => "Function",
            Self::Method => "Method",
            Self::Class => "Class",
            Self::Struct => "Struct",
            Self::Trait => "Trait",
            Self::Interface => "Interface",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::File => 1,
            Self::Module => 2,
            Self::Function => 3,
            Self::Method => 4,
            Self::Class => 5,
            Self::Struct => 6,
            Self::Trait => 7,
            Self::Interface => 8,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::File),
            2 => Some(Self::Module),
            3 => Some(Self::Function),
            4 => Some(Self::Method),
            5 => Some(Self::Class),
            6 => Some(Self::Struct),
            7 => Some(Self::Trait),
            8 => Some(Self::Interface),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains,
    Imports,
    Calls,
    Inherits,
    Implements,
}

impl EdgeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "Contains",
            Self::Imports => "Imports",
            Self::Calls => "Calls",
            Self::Inherits => "Inherits",
            Self::Implements => "Implements",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Contains => 1,
            Self::Imports => 2,
            Self::Calls => 3,
            Self::Inherits => 4,
            Self::Implements => 5,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Contains),
            2 => Some(Self::Imports),
            3 => Some(Self::Calls),
            4 => Some(Self::Inherits),
            5 => Some(Self::Implements),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Confidence {
    Extracted,
    Inferred,
}

impl Confidence {
    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Extracted => 1,
            Self::Inferred => 2,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Extracted),
            2 => Some(Self::Inferred),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file: String,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub qualified_name: String,
    pub file: String,
    pub location: SourceLocation,
    pub language: Option<Language>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub id: EdgeId,
    pub kind: EdgeKind,
    pub from: NodeId,
    pub to: NodeId,
    pub confidence: Confidence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}
