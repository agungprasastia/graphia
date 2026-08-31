use std::collections::BTreeMap;

use crate::model::{Language, NodeId, NodeKind, SourceLocation};

/// The kind of scope in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScopeKind {
    Project,
    Package,
    Module,
    Namespace,
    File,
    Class,
    Struct,
    Trait,
    Interface,
    Function,
    Method,
    Block,
}

impl ScopeKind {
    #[must_use]
    pub fn from_node_kind(kind: NodeKind) -> Self {
        match kind {
            NodeKind::File => Self::File,
            NodeKind::Module => Self::Module,
            NodeKind::Package => Self::Package,
            NodeKind::Namespace => Self::Namespace,
            NodeKind::Function => Self::Function,
            NodeKind::Method => Self::Method,
            NodeKind::Constructor => Self::Method,
            NodeKind::Class => Self::Class,
            NodeKind::Struct => Self::Struct,
            NodeKind::Trait => Self::Trait,
            NodeKind::Interface => Self::Interface,
            NodeKind::Enum => Self::Class,
            NodeKind::Variable
            | NodeKind::Constant
            | NodeKind::Field
            | NodeKind::Property
            | NodeKind::TypeAlias => Self::Block,
        }
    }
}

/// A scope node in the lexical/module hierarchy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub id: usize,
    pub parent: Option<usize>,
    pub kind: ScopeKind,
    pub name: String,
    pub file: String,
    pub language: Option<Language>,
    pub location: Option<SourceLocation>,
    /// Maps local symbol name -> list of node IDs (in definition order)
    pub symbols: BTreeMap<String, Vec<NodeId>>,
    /// Child scope IDs
    pub children: Vec<usize>,
}

/// Scope hierarchy arena storing all scopes for a project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeTree {
    scopes: Vec<Scope>,
    file_to_scope: BTreeMap<String, usize>,
    node_to_scope: BTreeMap<NodeId, usize>,
}

impl ScopeTree {
    #[must_use]
    pub fn new() -> Self {
        let mut tree = Self {
            scopes: Vec::new(),
            file_to_scope: BTreeMap::new(),
            node_to_scope: BTreeMap::new(),
        };
        // Root project scope is scope 0
        tree.scopes.push(Scope {
            id: 0,
            parent: None,
            kind: ScopeKind::Project,
            name: "<project>".to_string(),
            file: String::new(),
            language: None,
            location: None,
            symbols: BTreeMap::new(),
            children: Vec::new(),
        });
        tree
    }

    /// Add a child scope under an existing parent scope.
    pub fn add_scope(
        &mut self,
        parent: Option<usize>,
        kind: ScopeKind,
        name: &str,
        file: &str,
        language: Option<Language>,
        location: Option<SourceLocation>,
    ) -> usize {
        let parent_id = parent.unwrap_or(0);
        let id = self.scopes.len();
        let scope = Scope {
            id,
            parent: Some(parent_id),
            kind,
            name: name.to_string(),
            file: file.to_string(),
            language,
            location,
            symbols: BTreeMap::new(),
            children: Vec::new(),
        };
        self.scopes.push(scope);
        if let Some(parent_scope) = self.scopes.get_mut(parent_id) {
            parent_scope.children.push(id);
        }
        if kind == ScopeKind::File && !file.is_empty() {
            self.file_to_scope.insert(file.to_string(), id);
        }
        id
    }

    /// Register a symbol into a scope.
    pub fn define_symbol(&mut self, scope_id: usize, name: &str, node_id: NodeId) {
        if let Some(scope) = self.scopes.get_mut(scope_id) {
            scope
                .symbols
                .entry(name.to_string())
                .or_default()
                .push(node_id);
            self.node_to_scope.insert(node_id, scope_id);
        }
    }

    /// Get scope by ID.
    #[must_use]
    pub fn get(&self, scope_id: usize) -> Option<&Scope> {
        self.scopes.get(scope_id)
    }

    /// Total number of scopes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }

    /// Find file scope for a file path.
    #[must_use]
    pub fn file_scope(&self, file: &str) -> Option<usize> {
        self.file_to_scope.get(file).copied()
    }

    /// Find the scope enclosing a node ID.
    #[must_use]
    pub fn node_scope(&self, node_id: NodeId) -> Option<usize> {
        self.node_to_scope.get(&node_id).copied()
    }

    /// Resolve a symbol name starting from a scope and walking up parent scopes (lexical shadowing).
    /// Returns the first matching symbol list found in the closest scope.
    #[must_use]
    pub fn lookup_lexical(&self, start_scope: usize, name: &str) -> Option<(usize, &[NodeId])> {
        let mut curr = Some(start_scope);
        while let Some(scope_id) = curr {
            if let Some(scope) = self.scopes.get(scope_id) {
                if let Some(symbols) = scope.symbols.get(name) {
                    return Some((scope_id, symbols.as_slice()));
                }
                curr = scope.parent;
            } else {
                break;
            }
        }
        None
    }

    /// Collect all ancestor scope IDs from inner to outer (including start_scope).
    #[must_use]
    pub fn ancestor_chain(&self, start_scope: usize) -> Vec<usize> {
        let mut chain = Vec::new();
        let mut curr = Some(start_scope);
        while let Some(scope_id) = curr {
            chain.push(scope_id);
            curr = self.scopes.get(scope_id).and_then(|s| s.parent);
        }
        chain
    }
}
