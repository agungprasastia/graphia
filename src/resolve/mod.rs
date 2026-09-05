pub mod candidate;
pub mod import;
pub mod scope;

use std::collections::{BTreeMap, BTreeSet};

pub use candidate::{Candidate, CandidateSelector, Resolution, ResolutionReason, ResolutionState};
pub use import::{ImportDirective, ImportTable, parse_import_directive};
pub use scope::{Scope, ScopeKind, ScopeTree};

use crate::model::{
    Confidence, Edge, EdgeIdentity, EdgeKind, Language, Node, NodeId, NodeKind, SourceLocation,
};
use crate::parser::{Call, Export, Import, ParsedFile};

#[derive(Debug, Clone, Default)]
pub(crate) struct ResolutionFileContext {
    pub language: Option<Language>,
    pub imports: Vec<Import>,
    pub exports: Vec<Export>,
}

impl ResolutionFileContext {
    pub(crate) fn from_parsed(language: Option<Language>, parsed: &ParsedFile) -> Self {
        Self {
            language,
            imports: parsed.imports.clone(),
            exports: parsed.exports.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallReferenceIdentity {
    pub file: String,
    pub caller: String,
    pub callee: String,
    pub location: SourceLocation,
}

impl CallReferenceIdentity {
    pub(crate) fn new(file: &str, call: &Call) -> Self {
        Self {
            file: file.to_string(),
            caller: call.caller.clone(),
            callee: call.callee.clone(),
            location: call.location.clone(),
        }
    }
}

/// Resolution statistics and outcomes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolutionSummary {
    pub resolved_calls: usize,
    pub unresolved_calls: usize,
    pub ambiguous_calls: usize,
    /// Stable call-reference identity -> resolution state.
    pub call_states: BTreeMap<CallReferenceIdentity, ResolutionState>,
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
    /// Maps simple symbol name -> Vec<SymbolEntry>
    pub name_index: BTreeMap<String, Vec<SymbolEntry>>,
    /// Maps class/struct/trait name -> list of child methods/functions (name -> NodeId)
    pub type_members: BTreeMap<String, BTreeMap<String, Vec<NodeId>>>,
    /// Maps (file, exported_name) -> target_symbol_or_path
    pub export_table: BTreeMap<(String, String), String>,
    /// Node map by id for quick inspection
    pub node_by_id: BTreeMap<NodeId, Node>,
}

/// Short-lived memoization, borrowing the engine so its public indexes cannot
/// change underneath cached results. Released when the graph pass completes.
pub(crate) struct ResolutionSession<'a> {
    engine: &'a ResolutionEngine,
    imported_files: std::cell::RefCell<BTreeMap<String, std::rc::Rc<BTreeSet<String>>>>,
    reexports: std::cell::RefCell<ReexportCache>,
}

type ReexportCache = BTreeMap<(String, String), Option<(String, String)>>;

impl std::ops::Deref for ResolutionSession<'_> {
    type Target = ResolutionEngine;

    fn deref(&self) -> &Self::Target {
        self.engine
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    pub name: String,
    pub qualified_name: String,
    pub node_id: NodeId,
    pub file: String,
    pub parent_type: Option<String>,
    pub kind: NodeKind,
    pub language: Option<Language>,
    pub signature: Option<String>,
    pub container: Option<String>,
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
            export_table: BTreeMap::new(),
            node_by_id: BTreeMap::new(),
        }
    }

    /// Index parsed files and nodes before resolution.
    pub fn index_files(
        &mut self,
        nodes: &[Node],
        files: &[(String, Option<Language>, ParsedFile)],
    ) {
        let contexts = files
            .iter()
            .map(|(path, language, parsed)| {
                (
                    path.clone(),
                    ResolutionFileContext::from_parsed(*language, parsed),
                )
            })
            .collect();
        self.index_contexts(nodes, &contexts);
    }

    pub(crate) fn index_contexts(
        &mut self,
        nodes: &[Node],
        files: &BTreeMap<String, ResolutionFileContext>,
    ) {
        for node in nodes {
            self.node_by_id.insert(node.id, node.clone());
        }

        // 1. Build file scopes, register file nodes and exports
        for (path, context) in files {
            let file_node_id = nodes
                .iter()
                .find(|n| n.kind == NodeKind::File && n.file == *path)
                .map(|n| n.id);

            let file_scope_id = self.scope_tree.add_scope(
                Some(0),
                ScopeKind::File,
                path,
                path,
                context.language,
                None,
            );

            if let Some(fid) = file_node_id {
                self.file_nodes.insert(path.clone(), fid);
                self.scope_tree.define_symbol(file_scope_id, path, fid);
            }

            // Register import directives
            self.import_table
                .register_file_imports(path, context.language, &context.imports);

            // Register export directives / re-exports
            for exp in &context.exports {
                if let Some(target) = &exp.target {
                    self.export_table
                        .insert((path.clone(), exp.name.clone()), target.clone());
                }
            }
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
            let (scope_for_symbol, parsed_parent) = if let Some(parent) = node.container.clone() {
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
            self.scope_tree.bind_node_scope(node.id, scope_for_symbol);

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
                    name: node.name.clone(),
                    qualified_name: node.qualified_name.clone(),
                    node_id: node.id,
                    file: node.file.clone(),
                    parent_type: parent_type_name.clone(),
                    kind: node.kind,
                    language: node.language,
                    signature: node.signature.clone(),
                    container: node.container.clone().or(parent_type_name),
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
                if let Some(child) = self.scope_tree.get(child_id)
                    && child.name == type_name
                {
                    return child_id;
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
        let session = self.session();

        for (file, call) in calls {
            let caller_node = nodes
                .iter()
                .find(|n| n.qualified_name == call.caller && n.file == *file)
                .or_else(|| nodes.iter().find(|n| n.qualified_name == call.caller))
                .or_else(|| {
                    nodes
                        .iter()
                        .find(|n| n.name == call.caller && n.file == *file)
                })
                .or_else(|| {
                    let simple_caller = call.caller.rsplit("::").next().unwrap_or(&call.caller);
                    nodes
                        .iter()
                        .find(|n| n.name == simple_caller && n.file == *file)
                })
                .or_else(|| {
                    let simple_caller = call.caller.rsplit('.').next().unwrap_or(&call.caller);
                    nodes
                        .iter()
                        .find(|n| n.name == simple_caller && n.file == *file)
                });
            let Some(caller) = caller_node else {
                summary.unresolved_calls += 1;
                summary.call_states.insert(
                    CallReferenceIdentity::new(file, call),
                    ResolutionState::Unresolved,
                );
                continue;
            };

            let state = session.resolve_single_call(file, caller, &call.callee, existing_imports);

            match &state {
                ResolutionState::Resolved { target, reason } => {
                    summary.resolved_calls += 1;
                    let conf = match reason {
                        ResolutionReason::LexicalScope
                        | ResolutionReason::SameFile
                        | ResolutionReason::ExplicitImport
                        | ResolutionReason::ExplicitAlias => Confidence::Resolved,
                        ResolutionReason::ReceiverMethod
                        | ResolutionReason::InheritedMethod
                        | ResolutionReason::WildcardImport => Confidence::Inferred,
                    };
                    let identity =
                        EdgeIdentity::new(caller.id, *target, EdgeKind::Calls, conf, None);
                    resolved_edges.push(Edge {
                        id: crate::graph::stable_edge_id(&identity),
                        kind: EdgeKind::Calls,
                        from: caller.id,
                        to: *target,
                        confidence: conf,
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
                .insert(CallReferenceIdentity::new(file, call), state);
        }

        (resolved_edges, summary)
    }

    pub(crate) fn session(&self) -> ResolutionSession<'_> {
        ResolutionSession {
            engine: self,
            imported_files: Default::default(),
            reexports: Default::default(),
        }
    }

    #[must_use]
    pub fn resolve_reference(
        &self,
        file: &str,
        caller_id: Option<NodeId>,
        target_name: &str,
        kind: EdgeKind,
        param_count: Option<usize>,
    ) -> Resolution {
        self.session()
            .resolve_reference(file, caller_id, target_name, kind, param_count)
    }

    #[must_use]
    pub fn resolve_type_reference(&self, file: &str, type_name: &str) -> Resolution {
        self.session().resolve_type_reference(file, type_name)
    }

    #[must_use]
    pub fn resolve_instantiation(
        &self,
        file: &str,
        caller_id: Option<NodeId>,
        type_name: &str,
    ) -> Resolution {
        self.session()
            .resolve_instantiation(file, caller_id, type_name)
    }

    #[must_use]
    pub fn resolve_inheritance(
        &self,
        file: &str,
        derived_type: &str,
        base_name: &str,
    ) -> Resolution {
        self.session()
            .resolve_inheritance(file, derived_type, base_name)
    }

    #[must_use]
    pub fn resolve_implementation(
        &self,
        file: &str,
        implementing_type: &str,
        trait_or_interface: &str,
    ) -> Resolution {
        self.session()
            .resolve_implementation(file, implementing_type, trait_or_interface)
    }

    #[must_use]
    pub fn resolve_single_call(
        &self,
        caller_file: &str,
        caller: &Node,
        callee: &str,
        existing_imports: &[(NodeId, NodeId)],
    ) -> ResolutionState {
        self.session()
            .resolve_single_call(caller_file, caller, callee, existing_imports)
    }
}

impl ResolutionSession<'_> {
    /// Resolve a general reference or call.
    #[must_use]
    pub fn resolve_reference(
        &self,
        file: &str,
        caller_id: Option<NodeId>,
        target_name: &str,
        kind: EdgeKind,
        param_count: Option<usize>,
    ) -> Resolution {
        let caller_node = caller_id.and_then(|id| self.node_by_id.get(&id));
        self.resolve_symbol_with_context(file, caller_node, target_name, kind, param_count, None)
    }

    /// Resolve a type reference (struct, class, interface, trait, enum, type alias).
    #[must_use]
    pub fn resolve_type_reference(&self, file: &str, type_name: &str) -> Resolution {
        self.resolve_symbol_with_context(
            file,
            None,
            type_name,
            EdgeKind::TypeReferences,
            None,
            None,
        )
    }

    /// Resolve an instantiation (constructor, class, struct instantiated).
    #[must_use]
    pub fn resolve_instantiation(
        &self,
        file: &str,
        caller_id: Option<NodeId>,
        type_name: &str,
    ) -> Resolution {
        let caller_node = caller_id.and_then(|id| self.node_by_id.get(&id));
        self.resolve_symbol_with_context(
            file,
            caller_node,
            type_name,
            EdgeKind::Instantiates,
            None,
            None,
        )
    }

    /// Resolve inheritance base class.
    #[must_use]
    pub fn resolve_inheritance(
        &self,
        file: &str,
        derived_type: &str,
        base_name: &str,
    ) -> Resolution {
        self.resolve_symbol_with_context(
            file,
            None,
            base_name,
            EdgeKind::Inherits,
            None,
            Some(derived_type),
        )
    }

    /// Resolve trait or interface implementation.
    #[must_use]
    pub fn resolve_implementation(
        &self,
        file: &str,
        implementing_type: &str,
        trait_or_interface: &str,
    ) -> Resolution {
        self.resolve_symbol_with_context(
            file,
            None,
            trait_or_interface,
            EdgeKind::Implements,
            None,
            Some(implementing_type),
        )
    }

    /// Core semantic resolution engine method handling scope, aliases, re-exports, arity, and containers.
    fn resolve_symbol_with_context(
        &self,
        file: &str,
        caller: Option<&Node>,
        target_name: &str,
        edge_kind: EdgeKind,
        param_count: Option<usize>,
        container_context: Option<&str>,
    ) -> Resolution {
        let mut selector = CandidateSelector::new();
        let imported_files = self.collect_all_imported_files(file);

        // 1. Lexical Scope Lookup (if caller node is provided)
        if let Some(c) = caller
            && let Some(caller_scope) = self.scope_tree.node_scope(c.id)
            && let Some((scope_id, ids)) = self.scope_tree.lookup_lexical(caller_scope, target_name)
        {
            let is_local =
                scope_id != 0 && scope_id != self.scope_tree.file_scope(file).unwrap_or(0);
            let priority = if is_local { 1 } else { 2 };
            let reason = if is_local {
                ResolutionReason::LexicalScope
            } else {
                ResolutionReason::SameFile
            };
            for &id in ids {
                if self.candidate_matches_filter(id, edge_kind, param_count, container_context) {
                    selector.add(id, priority, reason.clone());
                }
            }
        }

        // 2. Same-file candidate matches
        if let Some(candidates) = self.name_index.get(target_name) {
            for c in candidates {
                if c.file == file
                    && self.candidate_matches_filter(
                        c.node_id,
                        edge_kind,
                        param_count,
                        container_context,
                    )
                {
                    let priority = if let Some(ctx) = container_context {
                        if c.container.as_deref() == Some(ctx)
                            || c.parent_type.as_deref() == Some(ctx)
                        {
                            1
                        } else {
                            2
                        }
                    } else {
                        2
                    };
                    selector.add(c.node_id, priority, ResolutionReason::SameFile);
                }
            }
        }

        if let Some(alias_bindings) = self.import_table.resolve_alias(file, target_name) {
            for (target_path, remote_sym) in alias_bindings {
                let symbol_to_find = remote_sym.as_deref().unwrap_or(target_name);
                let (final_sym, final_target_file) = self
                    .trace_reexport_path(target_path, symbol_to_find)
                    .unwrap_or_else(|| (symbol_to_find.to_string(), target_path.to_string()));

                if let Some(candidates) = self.name_index.get(&final_sym) {
                    for c in candidates {
                        let is_imported = imported_files.contains(&c.file);
                        let m1 = matches_import_target(&c.file, &final_target_file);
                        let m2 = matches_import_target(&c.file, target_path);
                        let is_same_lang = caller.is_none_or(|cl| {
                            cl.language == c.language
                                || cl.language.is_none()
                                || c.language.is_none()
                        });
                        let filter_pass = self.candidate_matches_filter(
                            c.node_id,
                            edge_kind,
                            param_count,
                            container_context,
                        );
                        if (m1 || m2 || is_imported) && is_same_lang && filter_pass {
                            let is_explicit_alias =
                                remote_sym.is_some() && remote_sym.as_deref() != Some(target_name);
                            let (priority, reason) = if is_explicit_alias {
                                (3, ResolutionReason::ExplicitAlias)
                            } else if m1 {
                                (3, ResolutionReason::ExplicitImport)
                            } else {
                                (4, ResolutionReason::ExplicitImport)
                            };
                            selector.add(c.node_id, priority, reason);
                        }
                    }
                }
            }
        }

        if let Some(dirs) = self.import_table.directives(file) {
            for d in dirs {
                let sym = d.imported_symbol.as_deref().unwrap_or(target_name);
                if (sym == target_name || sym == "*")
                    && let Some((final_sym, final_target_file)) =
                        self.trace_reexport_path(&d.target_module_or_path, sym)
                    && let Some(candidates) = self.name_index.get(&final_sym)
                {
                    for c in candidates {
                        if matches_import_target(&c.file, &final_target_file)
                            && self.candidate_matches_filter(
                                c.node_id,
                                edge_kind,
                                param_count,
                                container_context,
                            )
                        {
                            selector.add(c.node_id, 4, ResolutionReason::ExplicitImport);
                        }
                    }
                }
            }
        }

        // 4. Receiver-aware resolution / container-aware resolution
        // Check if target_name is a method in an imported class or caller's class or any imported container
        if let Some(candidates) = self.name_index.get(target_name) {
            for c in candidates {
                if let Some(parent) = &c.parent_type {
                    let in_caller_type = caller.is_some_and(|caller_node| {
                        caller_node.qualified_name.contains(parent)
                            || caller_node.container.as_deref() == Some(parent)
                    });

                    if in_caller_type {
                        if self.candidate_matches_filter(
                            c.node_id,
                            edge_kind,
                            param_count,
                            container_context,
                        ) {
                            selector.add(c.node_id, 2, ResolutionReason::ReceiverMethod);
                        }
                    } else {
                        // Check if parent type is imported or parent type's file is imported
                        let parent_imported =
                            self.is_type_imported(file, parent) || imported_files.contains(&c.file);
                        if parent_imported
                            && self.candidate_matches_filter(
                                c.node_id,
                                edge_kind,
                                param_count,
                                container_context,
                            )
                        {
                            selector.add(c.node_id, 5, ResolutionReason::ReceiverMethod);
                        }
                    }
                }
            }
        }

        // 5. Cross-file imported candidates (via wildcard or module import)
        if let Some(candidates) = self.name_index.get(target_name) {
            for c in candidates {
                if c.file != file
                    && imported_files.contains(&c.file)
                    && self.candidate_matches_filter(
                        c.node_id,
                        edge_kind,
                        param_count,
                        container_context,
                    )
                {
                    selector.add(c.node_id, 6, ResolutionReason::WildcardImport);
                }
            }
        }

        selector.resolve_unified()
    }

    fn is_type_imported(&self, file: &str, type_name: &str) -> bool {
        if let Some(alias_bindings) = self.import_table.resolve_alias(file, type_name) {
            return !alias_bindings.is_empty();
        }
        if let Some(dirs) = self.import_table.directives(file) {
            for d in dirs {
                if d.imported_symbol.as_deref() == Some(type_name)
                    || d.alias.as_deref() == Some(type_name)
                {
                    return true;
                }
            }
        }
        false
    }

    fn collect_all_imported_files(&self, file: &str) -> std::rc::Rc<BTreeSet<String>> {
        if let Some(imported) = self.imported_files.borrow().get(file) {
            return std::rc::Rc::clone(imported);
        }
        let mut imported = BTreeSet::new();
        if let Some(dirs) = self.import_table.directives(file) {
            for d in dirs {
                for file_path in self.file_nodes.keys() {
                    if matches_import_target(file_path, &d.target_module_or_path) {
                        imported.insert(file_path.clone());
                    }
                }
            }
        }
        let imported = std::rc::Rc::new(imported);
        self.imported_files
            .borrow_mut()
            .insert(file.to_string(), std::rc::Rc::clone(&imported));
        imported
    }

    /// Trace multi-hop re-exports: A defines Foo, B re-exports Foo from A, C imports from B.
    fn trace_reexport(&self, file: &str, sym_name: &str) -> Option<(String, String)> {
        let mut visited = BTreeSet::new();
        let mut curr_file = file.to_string();
        let mut curr_sym = sym_name.to_string();

        while visited.insert((curr_file.clone(), curr_sym.clone())) {
            // Check if current file has an export for curr_sym
            if let Some(target) = self
                .export_table
                .get(&(curr_file.clone(), curr_sym.clone()))
                && let Some((target_f, target_s)) = target.rsplit_once("::")
            {
                curr_file = target_f.to_string();
                curr_sym = target_s.to_string();
                continue;
            }
            // Check if curr_file imports curr_sym from another file that re-exports it
            if let Some(alias_bindings) = self.import_table.resolve_alias(&curr_file, &curr_sym) {
                let mut found_next = false;
                for (target_path, remote_sym) in alias_bindings {
                    let remote_name = remote_sym.as_deref().unwrap_or(&curr_sym);
                    for f in self.file_nodes.keys() {
                        if matches_import_target(f, target_path) {
                            curr_file = f.clone();
                            curr_sym = remote_name.to_string();
                            found_next = true;
                            break;
                        }
                    }
                    if found_next {
                        break;
                    }
                }
                if !found_next {
                    break;
                }
            } else {
                break;
            }
        }

        if curr_file != file || curr_sym != sym_name {
            Some((curr_file, curr_sym))
        } else {
            None
        }
    }

    fn trace_reexport_path(&self, target_path: &str, sym_name: &str) -> Option<(String, String)> {
        let key = (target_path.to_string(), sym_name.to_string());
        if let Some(result) = self.reexports.borrow().get(&key) {
            return result.clone();
        }
        let mut visited = BTreeSet::new();
        let result = self.trace_reexport_path_internal(target_path, sym_name, &mut visited);
        self.reexports.borrow_mut().insert(key, result.clone());
        result
    }

    fn trace_reexport_path_internal(
        &self,
        target_path: &str,
        sym_name: &str,
        visited: &mut BTreeSet<(String, String)>,
    ) -> Option<(String, String)> {
        let mut matching_files: Vec<String> = self
            .file_nodes
            .keys()
            .filter(|f| matches_import_target(f, target_path))
            .cloned()
            .collect();

        if matching_files.is_empty() {
            let direct_files: Vec<String> = self
                .file_nodes
                .keys()
                .filter(|f| {
                    f.ends_with(&format!("{target_path}.ts"))
                        || f.ends_with(&format!("{target_path}.rs"))
                        || f.ends_with(&format!("{target_path}.py"))
                        || f.ends_with(&format!("{target_path}.js"))
                })
                .cloned()
                .collect();
            if !direct_files.is_empty() {
                matching_files = direct_files;
            } else if let Some((target_f, target_s)) = target_path.rsplit_once("::") {
                return self.trace_reexport_path_internal(target_f, target_s, visited);
            }
        }

        for f in matching_files {
            if !visited.insert((f.clone(), sym_name.to_string())) {
                continue;
            }
            if let Some(target) = self.export_table.get(&(f.clone(), sym_name.to_string()))
                && let Some((target_f, target_s)) = target.rsplit_once("::")
            {
                let mut resolved_target_file = target_f.to_string();
                for real_f in self.file_nodes.keys() {
                    if matches_import_target(real_f, target_f) {
                        resolved_target_file = real_f.clone();
                        break;
                    }
                }
                if f != resolved_target_file {
                    if let Some((deep_s, deep_f)) =
                        self.trace_reexport_path_internal(&resolved_target_file, target_s, visited)
                    {
                        return Some((deep_s, deep_f));
                    }
                    return Some((target_s.to_string(), resolved_target_file));
                }
            }
            if let Some(dirs) = self.import_table.directives(&f) {
                for d in dirs {
                    let imported_s = d.imported_symbol.as_deref().unwrap_or(sym_name);
                    if (imported_s == sym_name || imported_s == "*")
                        && let Some((deep_s, deep_f)) = self.trace_reexport_path_internal(
                            &d.target_module_or_path,
                            sym_name,
                            visited,
                        )
                    {
                        return Some((deep_s, deep_f));
                    }
                }
            }
            if let Some(alias_bindings) = self.import_table.resolve_alias(&f, sym_name) {
                for (next_target, remote_sym) in alias_bindings {
                    let remote_name = remote_sym.as_deref().unwrap_or(sym_name);
                    if let Some((deep_s, deep_f)) =
                        self.trace_reexport_path_internal(next_target, remote_name, visited)
                    {
                        return Some((deep_s, deep_f));
                    }
                }
            }
            if let Some((orig_f, orig_s)) = self.trace_reexport(&f, sym_name) {
                for real_f in self.file_nodes.keys() {
                    if matches_import_target(real_f, &orig_f) {
                        return Some((orig_s, real_f.clone()));
                    }
                }
                return Some((orig_s, orig_f));
            }
            if let Some(candidates) = self.name_index.get(sym_name)
                && candidates.iter().any(|c| c.file == f)
            {
                return Some((sym_name.to_string(), f.clone()));
            }
        }
        None
    }

    fn candidate_matches_filter(
        &self,
        node_id: NodeId,
        edge_kind: EdgeKind,
        param_count: Option<usize>,
        container_context: Option<&str>,
    ) -> bool {
        let Some(node) = self.node_by_id.get(&node_id) else {
            return true;
        };

        match edge_kind {
            EdgeKind::TypeReferences => {
                if !matches!(
                    node.kind,
                    NodeKind::Class
                        | NodeKind::Struct
                        | NodeKind::Trait
                        | NodeKind::Interface
                        | NodeKind::Enum
                        | NodeKind::TypeAlias
                ) {
                    return false;
                }
            }
            EdgeKind::Instantiates => {
                if !matches!(
                    node.kind,
                    NodeKind::Class | NodeKind::Struct | NodeKind::Constructor | NodeKind::Enum
                ) {
                    return false;
                }
            }
            EdgeKind::Inherits => {
                if !matches!(
                    node.kind,
                    NodeKind::Class | NodeKind::Struct | NodeKind::Interface | NodeKind::Trait
                ) {
                    return false;
                }
            }
            EdgeKind::Implements => {
                if !matches!(
                    node.kind,
                    NodeKind::Trait | NodeKind::Interface | NodeKind::Class | NodeKind::Struct
                ) {
                    return false;
                }
            }
            EdgeKind::Calls
            | EdgeKind::References
            | EdgeKind::Contains
            | EdgeKind::Imports
            | EdgeKind::Exports => {}
        }

        if let Some(expected_params) = param_count
            && let Some(sig) = &node.signature
            && let Some(actual_count) = extract_param_count_from_signature(sig)
            && actual_count != expected_params
        {
            return false;
        }

        if let Some(expected_container) = container_context
            && let Some(c) = &node.container
            && c != expected_container
            && !c.ends_with(expected_container)
        {
            return false;
        }

        true
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
        let mut imported_file_ids: Vec<NodeId> = existing_imports
            .iter()
            .filter_map(|(from, to)| (*from == caller_file_node_id).then_some(*to))
            .collect();
        let imported_paths = self.collect_all_imported_files(caller_file);
        for p in imported_paths.iter() {
            if let Some(id) = self.file_nodes.get(p)
                && !imported_file_ids.contains(id)
            {
                imported_file_ids.push(*id);
            }
        }

        // 1. Lexical Scope Lookup (Local scope in caller function/class/file)
        if let Some(caller_scope) = self.scope_tree.node_scope(caller.id)
            && let Some((scope_id, ids)) = self.scope_tree.lookup_lexical(caller_scope, callee)
        {
            let is_local =
                scope_id != 0 && scope_id != self.scope_tree.file_scope(caller_file).unwrap_or(0);
            let priority = if is_local { 1 } else { 2 };
            let reason = if is_local {
                ResolutionReason::LexicalScope
            } else {
                ResolutionReason::SameFile
            };
            for &id in ids {
                if self.candidate_matches_filter(id, EdgeKind::Calls, None, None) {
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
                        let target_f_id =
                            self.file_nodes.get(&c.file).copied().unwrap_or(NodeId(0));
                        let is_imported = imported_file_ids.contains(&target_f_id);
                        let m1 = matches_import_target(&c.file, target_path);
                        let m2 = matches_import_target(target_path, &c.file);
                        let is_same_lang = caller.language == c.language
                            || c.language.is_none()
                            || caller.language.is_none();
                        if is_same_lang && (m1 || m2 || is_imported) {
                            let is_explicit_alias =
                                remote_sym.is_some() && remote_sym.as_deref() != Some(callee);
                            let (priority, reason) = if is_explicit_alias {
                                (3, ResolutionReason::ExplicitAlias)
                            } else if m1 || m2 {
                                (3, ResolutionReason::ExplicitImport)
                            } else {
                                (4, ResolutionReason::ExplicitImport)
                            };
                            selector.add(c.node_id, priority, reason);
                        }
                    }
                }
            }
        }

        if let Some(dirs) = self.import_table.directives(caller_file) {
            for d in dirs {
                if let Some(sym) = &d.imported_symbol {
                    if (sym == callee || sym == "*")
                        && let Some(candidates) = self.name_index.get(callee)
                    {
                        for c in candidates {
                            let is_same_lang = caller.language == c.language
                                || c.language.is_none()
                                || caller.language.is_none();
                            if is_same_lang
                                && matches_import_target(&c.file, &d.target_module_or_path)
                            {
                                selector.add(c.node_id, 4, ResolutionReason::ExplicitImport);
                            }
                        }
                    }
                } else if (matches_import_target(&d.target_module_or_path, callee)
                    || d.target_module_or_path.ends_with(&format!("::{callee}"))
                    || d.target_module_or_path.ends_with(&format!("/{callee}"))
                    || d.target_module_or_path.ends_with(callee))
                    && let Some(candidates) = self.name_index.get(callee)
                {
                    for c in candidates {
                        let is_same_lang = caller.language == c.language
                            || c.language.is_none()
                            || caller.language.is_none();
                        if is_same_lang
                            && (matches_import_target(&c.file, &d.target_module_or_path)
                                || matches_import_target(&d.target_module_or_path, &c.file)
                                || c.file.ends_with(&format!("{}.rs", d.target_module_or_path))
                                || c.file.ends_with(&format!("{}.py", d.target_module_or_path))
                                || c.file.ends_with(&format!("{}.ts", d.target_module_or_path)))
                        {
                            selector.add(c.node_id, 4, ResolutionReason::ExplicitImport);
                        }
                    }
                }
            }
        }

        // 4. Receiver-aware resolution (e.g. `obj.method` or caller is method of class, or method of imported type)
        // Check if callee is a known method name
        if let Some(candidates) = self.name_index.get(callee) {
            for c in candidates {
                if c.kind == NodeKind::Method
                    && let Some(parent) = &c.parent_type
                {
                    // If caller is in same type
                    if caller.qualified_name.contains(parent) {
                        selector.add(c.node_id, 2, ResolutionReason::ReceiverMethod);
                    } else if let Some(target_file_id) = self.file_nodes.get(&c.file) {
                        if imported_file_ids.contains(target_file_id) {
                            selector.add(c.node_id, 5, ResolutionReason::ReceiverMethod);
                        }
                    } else if imported_file_ids.iter().any(|id| {
                        self.file_nodes.iter().any(|(path, fid)| {
                            fid == id
                                && (matches_import_target(path, &c.file)
                                    || matches_import_target(&c.file, path))
                        })
                    }) {
                        selector.add(c.node_id, 5, ResolutionReason::ReceiverMethod);
                    } else if let Some(dirs) = self.import_table.directives(caller_file) {
                        for d in dirs {
                            if matches_import_target(&c.file, &d.target_module_or_path)
                                || d.imported_symbol.as_deref() == Some(parent)
                                || d.imported_symbol.as_deref() == Some(&c.name)
                                || d.target_module_or_path.ends_with(parent)
                                || d.target_module_or_path.ends_with(&c.name)
                            {
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
                if c.file != caller_file
                    && let Some(target_file_id) = self.file_nodes.get(&c.file)
                    && imported_file_ids.contains(target_file_id)
                {
                    selector.add(c.node_id, 4, ResolutionReason::ExplicitImport);
                }
            }
        }

        selector.resolve()
    }
}

fn matches_import_target(file_path: &str, target_path: &str) -> bool {
    fn normalize(path: &str) -> std::borrow::Cow<'_, str> {
        if path.contains('\\') || path.contains("::") {
            std::borrow::Cow::Owned(path.replace('\\', "/").replace("::", "/"))
        } else {
            std::borrow::Cow::Borrowed(path)
        }
    }
    fn trim(path: &str) -> &str {
        path.trim_start_matches("./")
            .trim_start_matches('/')
            .trim_end_matches("/mod")
            .trim_end_matches("/index")
    }
    let file = normalize(file_path);
    let target = normalize(target_path);
    let file = trim(&file);
    let target = trim(&target);
    if target.is_empty() {
        return false;
    }

    fn without_extension(path: &str) -> &str {
        match path.rsplit_once('.') {
            Some((
                stem,
                "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "c" | "h" | "cpp" | "java"
                | "cs" | "kt" | "kts" | "zig" | "php" | "rb" | "swift",
            )) => stem,
            _ => path,
        }
    }
    let file = without_extension(file);
    let target = without_extension(target);
    file == target
        || file
            .strip_suffix(target)
            .is_some_and(|prefix| prefix.ends_with('/'))
}

fn extract_param_count_from_signature(signature: &str) -> Option<usize> {
    let start = signature.find('(')?;
    let end = signature.rfind(')')?;
    if end <= start {
        return None;
    }
    let inner = signature[start + 1..end].trim();
    if inner.is_empty() {
        return Some(0);
    }
    let mut count = 0;
    let mut depth = 0;
    let mut current_has_chars = false;
    for ch in inner.chars() {
        match ch {
            '(' | '<' | '[' | '{' => {
                depth += 1;
                current_has_chars = true;
            }
            ')' | '>' | ']' | '}' => {
                if depth > 0 {
                    depth -= 1;
                }
                current_has_chars = true;
            }
            ',' if depth == 0 => {
                if current_has_chars {
                    count += 1;
                    current_has_chars = false;
                }
            }
            c if !c.is_whitespace() => {
                current_has_chars = true;
            }
            _ => {}
        }
    }
    if current_has_chars {
        count += 1;
    }
    Some(count)
}

#[cfg(test)]
mod performance_tests {
    use super::*;

    #[test]
    fn path_matching_preserves_normalization_and_boundaries() {
        let paths = [
            "",
            "/",
            "./src/foo.rs",
            "src/foo.rs",
            "src/foo",
            "foo",
            "oo",
            "src\\foo.rs",
            "src::foo",
            "././foo",
            "/foo",
            "foo/mod",
            "foo/index",
            "foo/mod.rs",
            "foo/index.ts",
            "foo.json",
            "foo.tsx",
            "foo.cpp",
            "foo.swift",
            "nested/foo.py",
            "nested/food.rs",
            "foo/index/mod",
            "é/foo.js",
        ];
        fn previous(file: &str, target: &str) -> bool {
            fn normalize(path: &str) -> String {
                path.replace('\\', "/")
                    .replace("::", "/")
                    .trim_start_matches("./")
                    .trim_start_matches('/')
                    .trim_end_matches("/mod")
                    .trim_end_matches("/index")
                    .to_string()
            }
            fn strip(path: &str) -> &str {
                [
                    ".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go", ".c", ".h", ".cpp", ".java",
                    ".cs", ".kt", ".kts", ".zig", ".php", ".rb", ".swift",
                ]
                .iter()
                .find_map(|ext| path.strip_suffix(ext))
                .unwrap_or(path)
            }
            let file = normalize(file);
            let target = normalize(target);
            if target.is_empty() {
                return false;
            }
            strip(&file) == strip(&target)
                || strip(&file).ends_with(&format!("/{}", strip(&target)))
        }
        for file in paths {
            for target in paths {
                assert_eq!(
                    matches_import_target(file, target),
                    previous(file, target),
                    "{file} -> {target}"
                );
            }
        }
    }

    #[test]
    fn session_reuses_results_and_does_not_survive_index_changes() {
        let mut engine = ResolutionEngine::new();
        engine.file_nodes.insert("a.rs".into(), NodeId(1));
        engine.file_nodes.insert("b.rs".into(), NodeId(2));
        engine
            .export_table
            .insert(("a.rs".into(), "Foo".into()), "b::Bar".into());
        {
            let session = engine.session();
            let first = session.collect_all_imported_files("a.rs");
            assert!(std::rc::Rc::ptr_eq(
                &first,
                &session.collect_all_imported_files("a.rs")
            ));
            for _ in 0..3 {
                assert_eq!(
                    session.trace_reexport_path("a", "Foo"),
                    Some(("Bar".into(), "b.rs".into()))
                );
                assert_eq!(session.trace_reexport_path("a", "Missing"), None);
            }
            assert_eq!(session.reexports.borrow().len(), 2);
        }
        engine
            .export_table
            .insert(("a.rs".into(), "Foo".into()), "b::New".into());
        assert_eq!(
            engine.session().trace_reexport_path("a", "Foo"),
            Some(("New".into(), "b.rs".into()))
        );
    }

    #[test]
    fn recursive_import_preserves_matching_file_traversal_order() {
        let mut engine = ResolutionEngine::new();
        for (id, file) in [(1, "a/foo.rs"), (2, "b/foo.rs")] {
            engine.file_nodes.insert(file.into(), NodeId(id));
            engine
                .name_index
                .entry("Foo".into())
                .or_default()
                .push(SymbolEntry {
                    name: "Foo".into(),
                    qualified_name: format!("{file}::Foo"),
                    node_id: NodeId(id + 2),
                    file: file.into(),
                    parent_type: None,
                    kind: NodeKind::Struct,
                    language: Some(Language::Rust),
                    signature: None,
                    container: None,
                });
        }
        engine.import_table.register_file_imports(
            "a/foo.rs",
            Some(Language::Rust),
            &[Import {
                path: "use foo::Foo;".into(),
                location: SourceLocation {
                    file: "a/foo.rs".into(),
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 14,
                },
            }],
        );
        let session = engine.session();
        for _ in 0..2 {
            assert_eq!(
                session.trace_reexport_path("foo", "Foo"),
                Some(("Foo".into(), "b/foo.rs".into()))
            );
        }
    }
}
