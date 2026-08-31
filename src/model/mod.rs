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

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum Visibility {
    Public,
    Protected,
    Private,
    Internal,
    Package,
    #[default]
    Unknown,
}

impl Visibility {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Protected => "protected",
            Self::Private => "private",
            Self::Internal => "internal",
            Self::Package => "package",
            Self::Unknown => "unknown",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Public => 1,
            Self::Protected => 2,
            Self::Private => 3,
            Self::Internal => 4,
            Self::Package => 5,
            Self::Unknown => 6,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Public),
            2 => Some(Self::Protected),
            3 => Some(Self::Private),
            4 => Some(Self::Internal),
            5 => Some(Self::Package),
            6 => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemanticNodeKey {
    pub language: Option<Language>,
    pub file: String,
    pub kind: NodeKind,
    pub qualified_name: String,
    pub container: Option<String>,
    pub signature: Option<String>,
}

impl SemanticNodeKey {
    #[must_use]
    pub fn new(
        language: Option<Language>,
        file: &str,
        kind: NodeKind,
        qualified_name: &str,
        container: Option<&str>,
        signature: Option<&str>,
    ) -> Self {
        Self {
            language,
            file: normalize_identity_path(file),
            kind,
            qualified_name: qualified_name.to_string(),
            container: container.map(str::to_string),
            signature: signature.map(str::to_string),
        }
    }

    #[must_use]
    pub fn from_node(node: &Node) -> Self {
        Self::new(
            node.language,
            &node.file,
            node.kind,
            &node.qualified_name,
            node.container.as_deref(),
            node.signature.as_deref(),
        )
    }
}

pub type NodeIdentity = SemanticNodeKey;

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
    JavaScript,
    Tsx,
    Jsx,
    Go,
    C,
    Cpp,
    Java,
    CSharp,
    Kotlin,
    Zig,
    Php,
    Ruby,
    Swift,
}

impl Language {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::JavaScript => "javascript",
            Self::Tsx => "tsx",
            Self::Jsx => "jsx",
            Self::Go => "go",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Java => "java",
            Self::CSharp => "csharp",
            Self::Kotlin => "kotlin",
            Self::Zig => "zig",
            Self::Php => "php",
            Self::Ruby => "ruby",
            Self::Swift => "swift",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Rust => 1,
            Self::Python => 2,
            Self::TypeScript => 3,
            Self::JavaScript => 4,
            Self::Tsx => 5,
            Self::Jsx => 6,
            Self::Go => 7,
            Self::C => 8,
            Self::Cpp => 9,
            Self::Java => 10,
            Self::CSharp => 11,
            Self::Kotlin => 12,
            Self::Zig => 13,
            Self::Php => 14,
            Self::Ruby => 15,
            Self::Swift => 16,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Rust),
            2 => Some(Self::Python),
            3 => Some(Self::TypeScript),
            4 => Some(Self::JavaScript),
            5 => Some(Self::Tsx),
            6 => Some(Self::Jsx),
            7 => Some(Self::Go),
            8 => Some(Self::C),
            9 => Some(Self::Cpp),
            10 => Some(Self::Java),
            11 => Some(Self::CSharp),
            12 => Some(Self::Kotlin),
            13 => Some(Self::Zig),
            14 => Some(Self::Php),
            15 => Some(Self::Ruby),
            16 => Some(Self::Swift),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    File,
    Module,
    Package,
    Namespace,
    Function,
    Method,
    Constructor,
    Class,
    Struct,
    Trait,
    Interface,
    Enum,
    Variable,
    Constant,
    Field,
    Property,
    TypeAlias,
}

impl NodeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Module => "Module",
            Self::Package => "Package",
            Self::Namespace => "Namespace",
            Self::Function => "Function",
            Self::Method => "Method",
            Self::Constructor => "Constructor",
            Self::Class => "Class",
            Self::Struct => "Struct",
            Self::Trait => "Trait",
            Self::Interface => "Interface",
            Self::Enum => "Enum",
            Self::Variable => "Variable",
            Self::Constant => "Constant",
            Self::Field => "Field",
            Self::Property => "Property",
            Self::TypeAlias => "TypeAlias",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::File => 1,
            Self::Module => 2,
            Self::Package => 3,
            Self::Namespace => 4,
            Self::Function => 5,
            Self::Method => 6,
            Self::Constructor => 7,
            Self::Class => 8,
            Self::Struct => 9,
            Self::Trait => 10,
            Self::Interface => 11,
            Self::Enum => 12,
            Self::Variable => 13,
            Self::Constant => 14,
            Self::Field => 15,
            Self::Property => 16,
            Self::TypeAlias => 17,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::File),
            2 => Some(Self::Module),
            3 => Some(Self::Package),
            4 => Some(Self::Namespace),
            5 => Some(Self::Function),
            6 => Some(Self::Method),
            7 => Some(Self::Constructor),
            8 => Some(Self::Class),
            9 => Some(Self::Struct),
            10 => Some(Self::Trait),
            11 => Some(Self::Interface),
            12 => Some(Self::Enum),
            13 => Some(Self::Variable),
            14 => Some(Self::Constant),
            15 => Some(Self::Field),
            16 => Some(Self::Property),
            17 => Some(Self::TypeAlias),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains,
    Imports,
    Exports,
    Calls,
    References,
    TypeReferences,
    Instantiates,
    Inherits,
    Implements,
}

impl EdgeKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Contains => "Contains",
            Self::Imports => "Imports",
            Self::Exports => "Exports",
            Self::Calls => "Calls",
            Self::References => "References",
            Self::TypeReferences => "TypeReferences",
            Self::Instantiates => "Instantiates",
            Self::Inherits => "Inherits",
            Self::Implements => "Implements",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Contains => 1,
            Self::Imports => 2,
            Self::Exports => 3,
            Self::Calls => 4,
            Self::References => 5,
            Self::TypeReferences => 6,
            Self::Instantiates => 7,
            Self::Inherits => 8,
            Self::Implements => 9,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Contains),
            2 => Some(Self::Imports),
            3 => Some(Self::Exports),
            4 => Some(Self::Calls),
            5 => Some(Self::References),
            6 => Some(Self::TypeReferences),
            7 => Some(Self::Instantiates),
            8 => Some(Self::Inherits),
            9 => Some(Self::Implements),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Confidence {
    Extracted,
    Resolved,
    Inferred,
    Possible,
}

impl Confidence {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extracted => "Extracted",
            Self::Resolved => "Resolved",
            Self::Inferred => "Inferred",
            Self::Possible => "Possible",
        }
    }

    #[must_use]
    pub fn code(self) -> u8 {
        match self {
            Self::Extracted => 1,
            Self::Resolved => 2,
            Self::Inferred => 3,
            Self::Possible => 4,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Extracted),
            2 => Some(Self::Resolved),
            3 => Some(Self::Inferred),
            4 => Some(Self::Possible),
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
    #[serde(default)]
    pub visibility: Visibility,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
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
