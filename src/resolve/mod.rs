pub mod candidate;
pub mod import;
pub mod scope;

use std::collections::BTreeMap;

pub use candidate::{Candidate, CandidateSelector, ResolutionReason, ResolutionState};
pub use import::{ImportDirective, ImportTable, parse_import_directive};
pub use scope::{Scope, ScopeKind, ScopeTree};

use crate::model::{Confidence, Edge, EdgeIdentity, EdgeKind, Language, Node, NodeId, NodeKind};
use crate::parser::{Call, ParsedFile};

/// Resolution statistics and outcomes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolutionSummary {
    pub resolved_calls: usize,
    pub unresolved_calls: usize,
    pub ambiguous_calls: usize,
    /// Call location / description -> Resolution state
    pub call_states: Vec<(String, String, ResolutionState)>,
}

/// Orchestrator for scope-aware, alias-aware, receiver-aware resolution.
#[derive(Debug, Clone)]
pub struct ResolutionEngine {
    pub scope_tree: ScopeTree,
    pub import_table: ImportTable,
    /// Maps file path -> NodeId of File node
    pub file_nodes: BTreeMap<String, NodeId>,
    /// Maps symbol qualified_name -> NodeId
    pub symbol_nodes: BTreeMap<String, Vec<NodeId>>,
    /// Maps simple symbol name -> Vec<(qualified_name, NodeId, file_path, Option<parent_type>, NodeKind)>
    pub name_index: BTreeMap<String, Vec<SymbolEntry>>,
    /// Maps class/struct/trait name -> list of child methods/functions (name -> NodeId)
    pub type_members: BTreeMap<String, BTreeMap<String, Vec<NodeId>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    pub qualified_name: String,
    pub node_id: NodeId,
    pub file: String,
    pub parent_type: Option<String>,
    pub kind: NodeKind,
    pub language: Option<Language>,
}

impl Default for ResolutionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ResolutionEngine {
    #[must_use]
    pub fn new() -> Self {
        Self {
            scope_tree: ScopeTree::new(),
            import_table: ImportTable::new(),
            file_nodes: BTreeMap::new(),
            symbol_nodes: BTreeMap::new(),
            name_index: BTreeMap::new(),
            type_members: BTreeMap::new(),
        }
    }

    /// Index parsed files and nodes before resolution.
    pub fn index_files(
        &mut self,
        nodes: &[Node],
        files: &[(String, Option<Language>, ParsedFile)],
    ) {
        // 1. Build file scopes and register file nodes
        for (path, lang, parsed) in files {
            let file_node_id = nodes
                .iter()
                .find(|n| n.kind == NodeKind::File && n.file == *path)
                .map(|n| n.id);

            let file_scope_id =
                self.scope_tree
                    .add_scope(Some(0), ScopeKind::File, path, path, *lang, None);

            if let Some(fid) = file_node_id {
                self.file_nodes.insert(path.clone(), fid);
                self.scope_tree.define_symbol(file_scope_id, path, fid);
            }

            // Register import directives
            self.import_table
                .register_file_imports(path, *lang, &parsed.imports);
        }

        // 2. Index all symbols into scopes, name_index, and type_members
        for node in nodes {
            if node.kind == NodeKind::File {
                continue;
            }

            self.symbol_nodes
                .entry(node.qualified_name.clone())
                .or_default()
                .push(node.id);

            let file_scope_id = self.scope_tree.file_scope(&node.file).unwrap_or(0);

            // Determine if symbol has parent class/type scope
            let mut parent_type_name: Option<String> = None;

            // Check if node qualified_name or parent indicates class membership
            // Many parsers format qualified_name as "file::parent::method" or "file::method"
            let (scope_for_symbol, parsed_parent) =
                if let Some(parent) = find_symbol_parent(node, files) {
                    parent_type_name = Some(parent.clone());
                    // Find or create scope for parent class
                    let class_scope = self.find_or_create_type_scope(
                        file_scope_id,
                        &parent,
                        &node.file,
                        node.language,
                    );
                    (class_scope, Some(parent))
                } else {
                    (file_scope_id, None)
                };

            self.scope_tree
                .define_symbol(scope_for_symbol, &node.name, node.id);

            if let Some(parent) = &parsed_parent {
                self.type_members
                    .entry(parent.clone())
                    .or_default()
                    .entry(node.name.clone())
                    .or_default()
                    .push(node.id);
            }

            self.name_index
                .entry(node.name.clone())
                .or_default()
                .push(SymbolEntry {
                    qualified_name: node.qualified_name.clone(),
                    node_id: node.id,
                    file: node.file.clone(),
                    parent_type: parent_type_name,
                    kind: node.kind,
                    language: node.language,
                });
        }
    }

    fn find_or_create_type_scope(
        &mut self,
        file_scope_id: usize,
        type_name: &str,
        file: &str,
        lang: Option<Language>,
    ) -> usize {
        if let Some(scope) = self.scope_tree.get(file_scope_id) {
            for &child_id in &scope.children {
                if let Some(child) = self.scope_tree.get(child_id) {
                    if child.name == type_name {
                        return child_id;
                    }
                }
            }
        }
        self.scope_tree.add_scope(
            Some(file_scope_id),
            ScopeKind::Class,
            type_name,
            file,
            lang,
            None,
        )
    }

    /// Resolve all calls and generate inferred Calls edges and resolution summary.
    #[must_use]
    pub fn resolve_calls(
        &self,
        calls: &[(String, Call)],
        nodes: &[Node],
        existing_imports: &[(NodeId, NodeId)],
    ) -> (Vec<Edge>, ResolutionSummary) {
        let mut resolved_edges = Vec::new();
        let mut summary = ResolutionSummary::default();

        for (file, call) in calls {
            let caller_node = nodes.iter().find(|n| n.qualified_name == call.caller);
            let Some(caller) = caller_node else {
                summary.unresolved_calls += 1;
                summary.call_states.push((
                    file.clone(),
                    call.callee.clone(),
                    ResolutionState::Unresolved,
                ));
                continue;
            };

            let state = self.resolve_single_call(file, caller, &call.callee, existing_imports);

            match &state {
                ResolutionState::Resolved { target, .. } => {
                    summary.resolved_calls += 1;
                    let identity = EdgeIdentity::new(
                        caller.id,
                        *target,
                        EdgeKind::Calls,
                        Confidence::Inferred,
                        None,
                    );
                    resolved_edges.push(Edge {
                        id: crate::graph::stable_edge_id(&identity),
                        kind: EdgeKind::Calls,
                        from: caller.id,
                        to: *target,
                        confidence: Confidence::Inferred,
                        label: None,
                    });
                }
                ResolutionState::Ambiguous { .. } => {
                    summary.ambiguous_calls += 1;
                }
                ResolutionState::Unresolved => {
                    summary.unresolved_calls += 1;
                }
            }

            summary
                .call_states
                .push((file.clone(), call.callee.clone(), state));
        }

        (resolved_edges, summary)
    }

    /// Resolve an individual call from caller to callee.
    #[must_use]
    pub fn resolve_single_call(
        &self,
        caller_file: &str,
        caller: &Node,
        callee: &str,
        existing_imports: &[(NodeId, NodeId)],
    ) -> ResolutionState {
        let mut selector = CandidateSelector::new();

        let caller_file_node_id = self
            .file_nodes
            .get(caller_file)
            .copied()
            .unwrap_or(NodeId(0));
        let imported_file_ids: Vec<NodeId> = existing_imports
            .iter()
            .filter_map(|(from, to)| (*from == caller_file_node_id).then_some(*to))
            .collect();

        // 1. Lexical Scope Lookup (Local scope in caller function/class/file)
        if let Some(caller_scope) = self.scope_tree.node_scope(caller.id) {
            if let Some((scope_id, ids)) = self.scope_tree.lookup_lexical(caller_scope, callee) {
                let is_local = scope_id != 0
                    && scope_id != self.scope_tree.file_scope(caller_file).unwrap_or(0);
                let priority = if is_local { 1 } else { 2 };
                let reason = if is_local {
                    ResolutionReason::LexicalScope
                } else {
                    ResolutionReason::SameFile
                };
                for &id in ids {
                    selector.add(id, priority, reason.clone());
                }
            }
        }

        // 2. Same-file candidate match if not found in caller lexical scope
        if let Some(candidates) = self.name_index.get(callee) {
            for c in candidates {
                if c.file == caller_file {
                    selector.add(c.node_id, 2, ResolutionReason::SameFile);
                }
            }
        }

        // 3. Alias / Explicit Import resolution from ImportTable
        if let Some(alias_bindings) = self.import_table.resolve_alias(caller_file, callee) {
            for (target_path, remote_sym) in alias_bindings {
                let symbol_to_find = remote_sym.as_deref().unwrap_or(callee);
                if let Some(candidates) = self.name_index.get(symbol_to_find) {
                    for c in candidates {
                        if matches_import_target(&c.file, target_path) {
                            let is_explicit_alias =
                                remote_sym.is_some() && remote_sym.as_deref() != Some(callee);
                            let (priority, reason) = if is_explicit_alias {
                                (3, ResolutionReason::ExplicitAlias)
                            } else {
                                (4, ResolutionReason::ExplicitImport)
                            };
                            selector.add(c.node_id, priority, reason);
                        }
                    }
                }
            }
        }

        // 4. Receiver-aware resolution (e.g. `obj.method` or caller is method of class, or method of imported type)
        // Check if callee is a known method name
        if let Some(candidates) = self.name_index.get(callee) {
            for c in candidates {
                if c.kind == NodeKind::Method {
                    if let Some(parent) = &c.parent_type {
                        // If caller is in same type
                        if caller.qualified_name.contains(parent) {
                            selector.add(c.node_id, 2, ResolutionReason::ReceiverMethod);
                        } else if let Some(target_file_id) = self.file_nodes.get(&c.file) {
                            if imported_file_ids.contains(target_file_id) {
                                selector.add(c.node_id, 5, ResolutionReason::ReceiverMethod);
                            }
                        }
                    }
                }
            }
        }

        // 5. Cross-file imported candidates (via file-level Imports edge or matching import directives)
        if let Some(candidates) = self.name_index.get(callee) {
            for c in candidates {
                if c.file != caller_file {
                    if let Some(target_file_id) = self.file_nodes.get(&c.file) {
                        if imported_file_ids.contains(target_file_id) {
                            selector.add(c.node_id, 6, ResolutionReason::WildcardImport);
                        }
                    }
                }
            }
        }

        selector.resolve()
    }
}

fn find_symbol_parent(
    node: &Node,
    files: &[(String, Option<Language>, ParsedFile)],
) -> Option<String> {
    for (path, _, parsed) in files {
        if path == &node.file {
            for s in &parsed.symbols {
                if s.qualified_name == node.qualified_name {
                    return s.parent.clone();
                }
            }
        }
    }
    None
}

fn matches_import_target(file_path: &str, target_path: &str) -> bool {
    let norm_target = target_path
        .replace('\\', "/")
        .replace("::", "/")
        .replace('.', "/");
    let norm_file = file_path.replace('\\', "/");

    norm_file == norm_target
        || norm_file.starts_with(&norm_target)
        || norm_file.ends_with(&norm_target)
        || norm_file.contains(&norm_target)
        || norm_file
            .strip_suffix(".rs")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".py")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".ts")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".js")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".go")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".java")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".cs")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".kt")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".php")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".rb")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".swift")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
        || norm_file
            .strip_suffix(".zig")
            .unwrap_or(&norm_file)
            .ends_with(&norm_target)
}
