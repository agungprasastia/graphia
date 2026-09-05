use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use super::error::{McpError, Result};
#[cfg(test)]
use super::protocol::TestInstrumentation;
use super::protocol::{
    CallToolParams, CancellationToken, Implementation, InitializeParams, InitializeResult,
    JsonRpcRequest, JsonRpcResponse, ListToolsResult, RequestId, ServerCapabilities,
    ToolsCapability,
};
use super::tools::{call_tool_with_cancellation, get_tool_definitions};
use super::transport::{StdioReader, StdioWriter};
use crate::graph::Graph;
use crate::scan::scan_repo;
use crate::storage::{compare_metadata, load_metadata, metadata_for_files};

pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "graphia-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_IN_FLIGHT_REQUESTS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexState {
    Current,
    Stale {
        index_mtime: u64,
        repo_mtime: u64,
    },
    Missing,
    Corrupt(String),
    VersionMismatch {
        index_version: u32,
        expected_version: u32,
    },
}

enum StreamEvent {
    Request(JsonRpcRequest),
    Cancel(RequestId),
    Parse(String),
    Fatal(McpError),
    Eof,
}

struct StreamJob {
    sequence: u64,
    request: JsonRpcRequest,
    id: RequestId,
    repo_root: PathBuf,
    graph: Option<Arc<Graph>>,
    initialized: bool,
    cancellations: Arc<Mutex<BTreeMap<RequestId, CancellationToken>>>,
    token: Option<CancellationToken>,
    #[cfg(test)]
    instrumentation: Option<Arc<TestInstrumentation>>,
}

struct StreamResponse {
    sequence: u64,
    response: JsonRpcResponse,
}

struct ActiveRequestGuard {
    cancellations: Arc<Mutex<BTreeMap<RequestId, CancellationToken>>>,
    id: RequestId,
}

impl ActiveRequestGuard {
    fn new(
        cancellations: &Arc<Mutex<BTreeMap<RequestId, CancellationToken>>>,
        id: &RequestId,
    ) -> Self {
        Self {
            cancellations: Arc::clone(cancellations),
            id: id.clone(),
        }
    }
}

impl Drop for ActiveRequestGuard {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.cancellations.lock() {
            registry.remove(&self.id);
        }
    }
}

/// MCP Server handling JSON-RPC requests, session state, and tool execution.
pub struct McpServer {
    repo_root: PathBuf,
    graph: Option<Arc<Graph>>,
    initialized: bool,
    auto_index: bool,
    cancellations: Arc<Mutex<BTreeMap<RequestId, CancellationToken>>>,
    request_token: Option<CancellationToken>,
    #[cfg(test)]
    instrumentation: Option<Arc<TestInstrumentation>>,
}

impl McpServer {
    /// Create a new MCP server instance bound to an optional repository root.
    #[must_use]
    pub fn new(repo_root: Option<PathBuf>) -> Self {
        Self::new_with_auto_index(repo_root, false)
    }

    #[must_use]
    pub fn new_with_auto_index(repo_root: Option<PathBuf>, auto_index: bool) -> Self {
        let repo_root = repo_root
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self {
            repo_root,
            graph: None,
            initialized: false,
            auto_index,
            cancellations: Arc::new(Mutex::new(BTreeMap::new())),
            request_token: None,
            #[cfg(test)]
            instrumentation: None,
        }
    }

    /// Load or build graph for the repository root.
    pub fn ensure_graph_loaded(&mut self) -> Result<()> {
        if self.graph.is_none() {
            let root = &self.repo_root;
            if !root.exists() {
                return Err(McpError::RepoNotFound(format!(
                    "Repository path '{}' does not exist",
                    root.display()
                )));
            }

            let binary_index = root.join(".graphia/index.bin");
            let state = self.index_state()?;
            let graph = if !binary_index.exists() {
                if self.auto_index {
                    crate::storage::build_or_update(root, false)
                        .map_err(|e| McpError::Internal(format!("Failed to build graph: {e}")))?
                        .0
                } else {
                    return Err(McpError::RepositoryNotIndexed(
                        "Repository index is missing. Run 'graphia build <repo>' or start MCP with '--auto-index'.".to_string(),
                    ));
                }
            } else {
                match state {
                    IndexState::VersionMismatch {
                        index_version,
                        expected_version,
                    } => {
                        if self.auto_index {
                            crate::storage::build_or_update(root, true)
                                .map_err(|e| McpError::Internal(e.to_string()))?
                                .0
                        } else {
                            return Err(McpError::VersionMismatch(format!(
                                "found {index_version}, expected {expected_version}"
                            )));
                        }
                    }
                    IndexState::Corrupt(message) => {
                        if self.auto_index {
                            crate::storage::build_or_update(root, true)
                                .map_err(|e| McpError::Internal(e.to_string()))?
                                .0
                        } else {
                            return Err(McpError::CorruptIndex(message));
                        }
                    }
                    IndexState::Stale {
                        index_mtime,
                        repo_mtime,
                    } => {
                        if self.auto_index {
                            crate::storage::build_or_update(root, false)
                                .map_err(|e| McpError::Internal(e.to_string()))?
                                .0
                        } else {
                            return Err(McpError::StaleIndex(format!(
                                "index mtime {index_mtime}, repository mtime {repo_mtime}"
                            )));
                        }
                    }
                    IndexState::Missing => unreachable!(),
                    IndexState::Current => crate::storage::load_graph_binary(&binary_index)
                        .map_err(|e| McpError::CorruptIndex(e.to_string()))?,
                }
            };

            self.graph = Some(Arc::new(graph));
        }

        Ok(())
    }

    /// Load or build graph for the repository root and return a reference.
    pub fn load_graph(&mut self) -> Result<&Graph> {
        self.ensure_graph_loaded()?;
        self.graph
            .as_deref()
            .ok_or_else(|| McpError::Internal("Graph not initialized".to_string()))
    }

    /// Set an explicit pre-built graph (useful for tests and in-memory execution).
    pub fn with_graph(mut self, graph: Graph) -> Self {
        self.graph = Some(Arc::new(graph));
        self
    }

    #[cfg(test)]
    fn with_instrumentation(mut self, instrumentation: Arc<TestInstrumentation>) -> Self {
        self.instrumentation = Some(instrumentation);
        self
    }

    /// Run the server on the provided input and output streams.
    /// Standard error can be used for server logging without contaminating stdout.
    pub fn run_stream<R: Read + Send, W: Write>(&mut self, input: R, output: W) -> Result<()> {
        let mut writer = StdioWriter::new(output);
        let (event_tx, event_rx) = mpsc::channel();
        let (job_tx, job_rx) = mpsc::sync_channel::<StreamJob>(MAX_IN_FLIGHT_REQUESTS);
        let (response_tx, response_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            scope.spawn(move || {
                let mut reader = StdioReader::new(input);
                loop {
                    match reader.read_message() {
                        Ok(Some(request)) if request.is_notification() => {
                            if matches!(
                                request.method.as_str(),
                                "$/cancelRequest" | "notifications/cancelled"
                            ) && let Some(params) = request.params
                                && let Ok(params) = serde_json::from_value::<
                                    super::protocol::CancelRequestParams,
                                >(params)
                                && event_tx.send(StreamEvent::Cancel(params.id)).is_err()
                            {
                                return;
                            }
                        }
                        Ok(Some(request)) => {
                            if event_tx.send(StreamEvent::Request(request)).is_err() {
                                return;
                            }
                        }
                        Ok(None) => {
                            let _ = event_tx.send(StreamEvent::Eof);
                            return;
                        }
                        Err(McpError::Parse(message)) => {
                            if event_tx.send(StreamEvent::Parse(message)).is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = event_tx.send(StreamEvent::Fatal(error));
                            return;
                        }
                    }
                }
            });

            let job_rx = Arc::new(Mutex::new(job_rx));
            for _ in 0..MAX_IN_FLIGHT_REQUESTS {
                let job_rx = Arc::clone(&job_rx);
                let response_tx = response_tx.clone();
                scope.spawn(move || {
                    loop {
                        let job = match job_rx.lock().ok().and_then(|rx| rx.recv().ok()) {
                            Some(job) => job,
                            None => return,
                        };
                        #[cfg(test)]
                        let instrumentation = job.instrumentation.clone();
                        #[cfg(test)]
                        if job.request.method == "tools/call"
                            && let Some(instrumentation) = &instrumentation
                        {
                            instrumentation.record_worker_started();
                        }
                        #[cfg(test)]
                        let is_tool_call = job.request.method == "tools/call";
                        let mut server = McpServer {
                            repo_root: job.repo_root,
                            graph: job.graph,
                            initialized: job.initialized,
                            auto_index: false,
                            cancellations: Arc::clone(&job.cancellations),
                            request_token: job.token,
                            #[cfg(test)]
                            instrumentation: instrumentation.clone(),
                        };
                        let response = server.handle_request(job.request, job.id);
                        #[cfg(test)]
                        if is_tool_call && let Some(instrumentation) = &instrumentation {
                            instrumentation.record_worker_finished();
                        }
                        let _ = response_tx.send(StreamResponse {
                            sequence: job.sequence,
                            response,
                        });
                    }
                });
            }
            drop(response_tx);

            let mut eof = false;
            let mut pending = 0usize;
            let mut next_sequence = 0u64;
            let mut response_order = VecDeque::new();
            let mut completed = BTreeMap::new();
            loop {
                while let Ok(response) = response_rx.try_recv() {
                    completed.insert(response.sequence, response.response);
                }
                while let Some(sequence) = response_order.front() {
                    let Some(response) = completed.remove(sequence) else {
                        break;
                    };
                    response_order.pop_front();
                    pending = pending.saturating_sub(1);
                    if writer.write_response(&response).is_err() {
                        return;
                    }
                }
                if eof && pending == 0 {
                    break;
                }
                match event_rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(StreamEvent::Request(request)) => {
                        let id = request.id.clone().unwrap_or(RequestId::Null);
                        let graph = if request.method == "tools/call" {
                            match self.ensure_graph_loaded() {
                                Ok(()) => self.graph.clone(),
                                Err(error) => {
                                    let _ = writer.write_response(&JsonRpcResponse::error(
                                        id,
                                        error.to_jsonrpc_error(),
                                    ));
                                    continue;
                                }
                            }
                        } else {
                            self.graph.clone()
                        };
                        let token = if request.method == "tools/call" {
                            #[cfg(test)]
                            {
                                Some(self.instrumentation.as_ref().map_or_else(
                                    CancellationToken::new,
                                    |instrumentation| {
                                        CancellationToken::with_instrumentation(Arc::clone(
                                            instrumentation,
                                        ))
                                    },
                                ))
                            }
                            #[cfg(not(test))]
                            {
                                Some(CancellationToken::new())
                            }
                        } else {
                            None
                        };
                        if let Some(token) = &token
                            && let Ok(mut registry) = self.cancellations.lock()
                        {
                            registry.insert(id.clone(), token.clone());
                        }
                        let job = StreamJob {
                            sequence: next_sequence,
                            request,
                            id: id.clone(),
                            repo_root: self.repo_root.clone(),
                            graph,
                            initialized: self.initialized,
                            cancellations: Arc::clone(&self.cancellations),
                            token,
                            #[cfg(test)]
                            instrumentation: self.instrumentation.clone(),
                        };
                        match job_tx.try_send(job) {
                            Ok(()) => {
                                response_order.push_back(next_sequence);
                                next_sequence = next_sequence.wrapping_add(1);
                                pending += 1;
                            }
                            Err(mpsc::TrySendError::Full(_)) => {
                                if let Ok(mut registry) = self.cancellations.lock() {
                                    registry.remove(&id);
                                }
                                let _ = writer.write_response(&JsonRpcResponse::error(
                                    id,
                                    McpError::Internal("MCP request capacity reached".into())
                                        .to_jsonrpc_error(),
                                ));
                            }
                            Err(mpsc::TrySendError::Disconnected(job)) => {
                                if let Ok(mut registry) = self.cancellations.lock() {
                                    registry.remove(&job.id);
                                }
                                break;
                            }
                        }
                    }
                    Ok(StreamEvent::Cancel(id)) => self.cancel_request(&id),
                    Ok(StreamEvent::Parse(message)) => {
                        let _ = writer.write_response(&JsonRpcResponse::error(
                            RequestId::Null,
                            McpError::Parse(message).to_jsonrpc_error(),
                        ));
                    }
                    Ok(StreamEvent::Fatal(error)) => {
                        let _ = writer.write_response(&JsonRpcResponse::error(
                            RequestId::Null,
                            error.to_jsonrpc_error(),
                        ));
                        eof = true;
                    }
                    Ok(StreamEvent::Eof) | Err(mpsc::RecvTimeoutError::Disconnected) => eof = true,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            drop(job_tx);
        });
        Ok(())
    }

    /// Handle a JSON-RPC request and produce a response.
    #[must_use]
    pub fn handle_request(&mut self, request: JsonRpcRequest, id: RequestId) -> JsonRpcResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request.params, id),
            "tools/list" => self.handle_tools_list(id),
            "tools/call" => self.handle_tools_call(request.params, id),
            "ping" => JsonRpcResponse::success(id, serde_json::json!({})),
            _ => {
                let err = McpError::MethodNotFound(format!("Unknown method: {}", request.method));
                JsonRpcResponse::error(id, err.to_jsonrpc_error())
            }
        }
    }

    /// Handle a JSON-RPC notification (no response sent).
    pub fn handle_notification(&mut self, request: &JsonRpcRequest) -> Result<()> {
        match request.method.as_str() {
            "notifications/initialized" => {
                self.initialized = true;
                eprintln!("[graphia-mcp] Client initialized notification received");
            }
            "$/cancelRequest" | "notifications/cancelled" => {
                let Some(params) = request.params.clone() else {
                    return Ok(());
                };
                if let Ok(params) =
                    serde_json::from_value::<super::protocol::CancelRequestParams>(params)
                {
                    self.cancel_request(&params.id);
                }
            }
            _ => {
                eprintln!("[graphia-mcp] Received notification: {}", request.method);
            }
        }
        Ok(())
    }

    pub fn cancel_request(&self, id: &RequestId) {
        if let Ok(registry) = self.cancellations.lock()
            && let Some(token) = registry.get(id)
        {
            token.cancel();
        }
    }

    fn handle_initialize(
        &mut self,
        params: Option<serde_json::Value>,
        id: RequestId,
    ) -> JsonRpcResponse {
        if let Some(val) = params
            && let Ok(init_params) = serde_json::from_value::<InitializeParams>(val)
        {
            eprintln!(
                "[graphia-mcp] Initializing MCP session with protocol version: {}",
                init_params.protocol_version
            );
        }

        self.initialized = true;

        let result = InitializeResult {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: Some(false),
                }),
            },
            server_info: Implementation {
                name: SERVER_NAME.to_string(),
                version: SERVER_VERSION.to_string(),
            },
            instructions: Some(
                "Graphia Code Graph MCP Server provides read-only structural code navigation, search, impact analysis, and context generation.".to_string(),
            ),
        };

        match serde_json::to_value(result) {
            Ok(v) => JsonRpcResponse::success(id, v),
            Err(e) => JsonRpcResponse::error(
                id,
                McpError::Internal(format!("Serialization error: {e}")).to_jsonrpc_error(),
            ),
        }
    }

    fn handle_tools_list(&self, id: RequestId) -> JsonRpcResponse {
        let tools = get_tool_definitions();
        let result = ListToolsResult {
            tools,
            next_cursor: None,
        };

        match serde_json::to_value(result) {
            Ok(v) => JsonRpcResponse::success(id, v),
            Err(e) => JsonRpcResponse::error(
                id,
                McpError::Internal(format!("Serialization error: {e}")).to_jsonrpc_error(),
            ),
        }
    }

    fn handle_tools_call(
        &mut self,
        params: Option<serde_json::Value>,
        id: RequestId,
    ) -> JsonRpcResponse {
        let _active_request = ActiveRequestGuard::new(&self.cancellations, &id);
        let Some(param_val) = params else {
            return JsonRpcResponse::error(
                id,
                McpError::InvalidParams("Missing params for tools/call".to_string())
                    .to_jsonrpc_error(),
            );
        };

        let call_params: CallToolParams = match serde_json::from_value(param_val) {
            Ok(p) => p,
            Err(e) => {
                return JsonRpcResponse::error(
                    id,
                    McpError::InvalidParams(format!("Invalid tools/call params: {e}"))
                        .to_jsonrpc_error(),
                );
            }
        };

        // Path sandbox validation: inspect arguments for any disallowed path traversal
        if let Some(ref args) = call_params.arguments {
            for (key, val) in args {
                if let Some(path_str) = val.as_str()
                    && (key == "file" || key.contains("path"))
                    && self.is_path_traversal(path_str)
                {
                    return JsonRpcResponse::error(
                        id,
                        McpError::PathTraversal(format!(
                            "Path traversal attempted in parameter '{key}': '{path_str}'"
                        ))
                        .to_jsonrpc_error(),
                    );
                }
            }
        }

        if let Err(e) = self.ensure_graph_loaded() {
            return JsonRpcResponse::error(id, e.to_jsonrpc_error());
        }

        let Some(graph) = self.graph.as_ref() else {
            return JsonRpcResponse::error(
                id,
                McpError::Internal("Graph not initialized".to_string()).to_jsonrpc_error(),
            );
        };
        let root_ref = Some(self.repo_root.as_path());
        let token = self.request_token.take().unwrap_or_default();
        if let Ok(mut registry) = self.cancellations.lock() {
            registry.insert(id.clone(), token.clone());
        }
        let result = call_tool_with_cancellation(
            graph,
            root_ref,
            &call_params.name,
            call_params.arguments.as_ref(),
            &token,
        );
        match result {
            Ok(tool_result) => match serde_json::to_value(tool_result) {
                Ok(val) => JsonRpcResponse::success(id, val),
                Err(e) => JsonRpcResponse::error(
                    id,
                    McpError::Internal(format!("Serialization error: {e}")).to_jsonrpc_error(),
                ),
            },
            Err(e) => JsonRpcResponse::error(id, e.to_jsonrpc_error()),
        }
    }

    fn index_state(&self) -> Result<IndexState> {
        let path = self.repo_root.join(".graphia/index.bin");
        if !path.exists() {
            return Ok(IndexState::Missing);
        }
        let data = std::fs::read(&path).map_err(McpError::from)?;
        if data.len() < 8 || &data[..4] != b"GRPH" {
            return Ok(IndexState::Corrupt("invalid graph index header".into()));
        }
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if version != 3 {
            return Ok(IndexState::VersionMismatch {
                index_version: version,
                expected_version: 3,
            });
        }
        if let Err(error) = crate::storage::load_graph_binary(&path) {
            return Ok(IndexState::Corrupt(error.to_string()));
        }
        let index_mtime = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let scanned = scan_repo(&self.repo_root).map_err(McpError::from)?;
        let current = metadata_for_files(&scanned).map_err(McpError::from)?;
        let previous = load_metadata(&self.repo_root).map_err(McpError::from)?;
        let changed = !compare_metadata(previous.as_ref(), &current)
            .iter()
            .all(|c| c.change == crate::storage::FileChange::Unchanged);
        let repo_mtime = current
            .files
            .iter()
            .filter_map(|f| f.modified_ns)
            .max()
            .map_or(0, |n| (n / 1_000_000_000) as u64);
        Ok(if changed || repo_mtime > index_mtime {
            IndexState::Stale {
                index_mtime,
                repo_mtime,
            }
        } else {
            IndexState::Current
        })
    }

    pub fn classify_index_state(&self) -> Result<IndexState> {
        self.index_state()
    }

    /// Check whether a path traverses outside of the repository boundary.
    fn is_path_traversal(&self, path_str: &str) -> bool {
        let normalized = path_str.replace('\\', "/");
        if normalized.starts_with('/')
            || normalized.contains("/../")
            || normalized.starts_with("../")
            || normalized.ends_with("/..")
            || normalized == ".."
            || (normalized.len() >= 2 && normalized.chars().nth(1) == Some(':'))
        {
            return true;
        }

        let p = Path::new(path_str);
        if p.is_absolute() {
            if let Ok(canonical_repo) = self.repo_root.canonicalize()
                && let Ok(canonical_p) = p.canonicalize()
            {
                return !canonical_p.starts_with(canonical_repo);
            }
            return true;
        }

        for comp in p.components() {
            if comp == std::path::Component::ParentDir {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Condvar;
    use std::sync::atomic::Ordering;

    use crate::model::{
        Confidence, Edge, EdgeId, EdgeKind, Language, Node, NodeId, NodeKind, SourceLocation,
        Visibility,
    };

    struct ChannelReader {
        receiver: mpsc::Receiver<Vec<u8>>,
        chunk: Cursor<Vec<u8>>,
    }

    impl ChannelReader {
        fn new(receiver: mpsc::Receiver<Vec<u8>>) -> Self {
            Self {
                receiver,
                chunk: Cursor::new(Vec::new()),
            }
        }
    }

    impl Read for ChannelReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            loop {
                if self.chunk.position() < self.chunk.get_ref().len() as u64 {
                    return self.chunk.read(buffer);
                }
                match self.receiver.recv() {
                    Ok(chunk) => self.chunk = Cursor::new(chunk),
                    Err(_) => return Ok(0),
                }
            }
        }
    }

    #[derive(Clone, Default)]
    struct SynchronizedOutput(Arc<(Mutex<Vec<u8>>, Condvar)>);

    impl Write for SynchronizedOutput {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            let mut bytes = self.0.0.lock().expect("output lock");
            bytes.extend_from_slice(buffer);
            self.0.1.notify_all();
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl SynchronizedOutput {
        fn response(bytes: &[u8], id: &RequestId) -> Option<JsonRpcResponse> {
            String::from_utf8_lossy(bytes).lines().find_map(|line| {
                serde_json::from_str::<JsonRpcResponse>(line)
                    .ok()
                    .filter(|response| &response.id == id)
            })
        }

        fn wait_response(&self, id: RequestId) -> JsonRpcResponse {
            let bytes = self.0.0.lock().expect("output lock");
            let (bytes, timeout) = self
                .0
                .1
                .wait_timeout_while(bytes, Duration::from_secs(5), |bytes| {
                    Self::response(bytes, &id).is_none()
                })
                .expect("output wait");
            assert!(!timeout.timed_out(), "response {id:?} timed out");
            Self::response(&bytes, &id).expect("response present")
        }

        fn response_count(bytes: &[u8], id: &RequestId) -> usize {
            String::from_utf8_lossy(bytes)
                .lines()
                .filter_map(|line| serde_json::from_str::<JsonRpcResponse>(line).ok())
                .filter(|response| &response.id == id)
                .count()
        }

        fn wait_response_count(&self, id: RequestId, expected: usize) {
            let bytes = self.0.0.lock().expect("output lock");
            let (bytes, timeout) = self
                .0
                .1
                .wait_timeout_while(bytes, Duration::from_secs(5), |bytes| {
                    Self::response_count(bytes, &id) < expected
                })
                .expect("output wait");
            assert!(
                !timeout.timed_out(),
                "{expected} responses for {id:?} timed out"
            );
            assert_eq!(Self::response_count(&bytes, &id), expected);
        }
    }

    fn send_line(sender: &mpsc::Sender<Vec<u8>>, line: &str) {
        sender
            .send(format!("{line}\n").into_bytes())
            .expect("send input");
    }

    fn chain_graph(total_work: u64) -> Graph {
        let nodes = (0..total_work)
            .map(|id| Node {
                id: NodeId(id),
                kind: NodeKind::Function,
                name: format!("n{id}"),
                qualified_name: format!("chain::n{id}"),
                file: "chain.rs".into(),
                location: SourceLocation {
                    file: "chain.rs".into(),
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 1,
                },
                language: Some(Language::Rust),
                visibility: Visibility::Public,
                signature: None,
                container: None,
            })
            .collect();
        let edges = (0..total_work - 1)
            .map(|id| Edge {
                id: EdgeId(id),
                kind: EdgeKind::Calls,
                from: NodeId(id),
                to: NodeId(id + 1),
                confidence: Confidence::Resolved,
                label: None,
            })
            .collect();
        Graph::new(nodes, edges)
    }

    fn wide_graph(total_work: u64) -> Graph {
        let mut graph = chain_graph(total_work);
        graph.edges = (1..total_work)
            .map(|id| Edge {
                id: EdgeId(id - 1),
                kind: EdgeKind::Calls,
                from: NodeId(0),
                to: NodeId(id),
                confidence: Confidence::Resolved,
                label: None,
            })
            .collect();
        graph
    }

    #[test]
    fn server_handles_initialize_and_tools_list() {
        let mut server = McpServer::new(None).with_graph(Graph::new(vec![], vec![]));

        let init_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(1)),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {}
            })),
        };

        let resp = server.handle_request(init_req, RequestId::Number(1));
        assert_eq!(resp.id, RequestId::Number(1));
        assert!(resp.error.is_none());
        assert_eq!(
            resp.result.as_ref().unwrap()["serverInfo"]["name"],
            "graphia-mcp"
        );

        let list_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(2)),
            method: "tools/list".to_string(),
            params: None,
        };

        let list_resp = server.handle_request(list_req, RequestId::Number(2));
        assert_eq!(list_resp.id, RequestId::Number(2));
        let tools = list_resp.result.unwrap()["tools"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(tools.len(), 12);
    }

    #[test]
    fn server_blocks_path_traversal() {
        let mut server = McpServer::new(None).with_graph(Graph::new(vec![], vec![]));

        let call_req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(RequestId::Number(3)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "graphia_find_tests",
                "arguments": {
                    "file": "../../etc/passwd"
                }
            })),
        };

        let resp = server.handle_request(call_req, RequestId::Number(3));
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(
            err.code,
            super::super::error::error_codes::PATH_TRAVERSAL_DETECTED
        );
    }

    #[test]
    fn invalid_tool_request_cleans_active_registry() {
        let id = RequestId::Number(4);
        let mut server = McpServer::new(None).with_graph(Graph::new(vec![], vec![]));
        server
            .cancellations
            .lock()
            .expect("active registry")
            .insert(id.clone(), CancellationToken::default());

        let response = server.handle_request(
            JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: Some(id.clone()),
                method: "tools/call".to_string(),
                params: None,
            },
            id.clone(),
        );

        assert!(response.error.is_some());
        assert!(
            !server
                .cancellations
                .lock()
                .expect("active registry")
                .contains_key(&id)
        );
    }

    #[test]
    fn server_runs_stream() {
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2024-11-05\"}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
        let mut output = Vec::new();

        let mut server = McpServer::new(None).with_graph(Graph::new(vec![], vec![]));
        server
            .run_stream(Cursor::new(input.as_bytes()), &mut output)
            .unwrap();

        let out_str = String::from_utf8(output).unwrap();
        let lines: Vec<&str> = out_str.lines().collect();
        assert_eq!(lines.len(), 2);

        let res1: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(res1["id"], 1);

        let res2: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(res2["id"], 2);
    }

    #[test]
    fn stream_preserves_duplicate_request_id_occurrences() {
        let instrumentation = Arc::new(TestInstrumentation::default());
        instrumentation.hold_workers.store(true, Ordering::Release);
        let (input_sender, input_receiver) = mpsc::channel();
        let output = SynchronizedOutput::default();
        let mut server = McpServer::new(None)
            .with_graph(chain_graph(4))
            .with_instrumentation(Arc::clone(&instrumentation));
        let server_output = output.clone();
        let server_thread = std::thread::spawn(move || {
            server.run_stream(ChannelReader::new(input_receiver), server_output)
        });

        let request = r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"graphia_search_symbol","arguments":{"query":"n1"}}}"#;
        send_line(&input_sender, request);
        send_line(&input_sender, request);
        assert!(
            instrumentation
                .wait_until(|| { instrumentation.workers_started.load(Ordering::Acquire) == 2 })
        );
        instrumentation.release_workers();

        output.wait_response_count(RequestId::Number(7), 2);
        drop(input_sender);
        server_thread
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn stream_cancels_only_after_worker_starts_real_work() {
        const TOTAL_WORK: u64 = 10_000;
        let graph = wide_graph(TOTAL_WORK);
        let baseline = Arc::new(TestInstrumentation::default());
        let baseline_token = CancellationToken::with_instrumentation(Arc::clone(&baseline));
        let baseline_arguments = serde_json::json!({
            "from": "n0",
            "to": "n9999",
            "max_depth": 1
        })
        .as_object()
        .expect("arguments")
        .clone();
        call_tool_with_cancellation(
            &graph,
            None,
            "graphia_dependency_path",
            Some(&baseline_arguments),
            &baseline_token,
        )
        .expect("baseline traversal");
        assert_eq!(
            baseline.work_units.load(Ordering::Acquire),
            TOTAL_WORK as usize
        );

        let instrumentation = Arc::new(TestInstrumentation::default());
        instrumentation.pause_work.store(true, Ordering::Release);
        let (input_sender, input_receiver) = mpsc::channel();
        let output = SynchronizedOutput::default();
        let mut server = McpServer::new(None)
            .with_graph(graph)
            .with_instrumentation(Arc::clone(&instrumentation));
        let active_registry = Arc::clone(&server.cancellations);
        let server_output = output.clone();
        let server_thread = std::thread::spawn(move || {
            server.run_stream(ChannelReader::new(input_receiver), server_output)
        });

        send_line(
            &input_sender,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#,
        );
        assert!(output.wait_response(RequestId::Number(1)).error.is_none());
        send_line(
            &input_sender,
            r#"{"jsonrpc":"2.0","id":100,"method":"tools/call","params":{"name":"graphia_dependency_path","arguments":{"from":"n0","to":"n9999","max_depth":1}}}"#,
        );
        assert!(instrumentation.wait_until(|| {
            instrumentation.worker_started.load(Ordering::Acquire)
                && instrumentation.work_units.load(Ordering::Acquire) > 0
        }));

        send_line(
            &input_sender,
            r#"{"jsonrpc":"2.0","method":"$/cancelRequest","params":{"id":100}}"#,
        );
        let cancelled = output.wait_response(RequestId::Number(100));
        assert_eq!(
            cancelled.error.as_ref().map(|error| error.code),
            Some(super::super::error::error_codes::CANCELLED)
        );
        assert!(instrumentation.wait_until(|| {
            instrumentation.cancel_observed.load(Ordering::Acquire)
                && instrumentation.worker_finished.load(Ordering::Acquire)
        }));
        let work_units = instrumentation.work_units.load(Ordering::Acquire);
        assert!(work_units > 0);
        assert!(work_units < TOTAL_WORK as usize);
        assert!(instrumentation.cancel_observed.load(Ordering::Acquire));
        assert!(
            !active_registry
                .lock()
                .expect("active registry")
                .contains_key(&RequestId::Number(100))
        );

        send_line(
            &input_sender,
            r#"{"jsonrpc":"2.0","id":101,"method":"tools/call","params":{"name":"graphia_search_symbol","arguments":{"query":"n1"}}}"#,
        );
        assert!(output.wait_response(RequestId::Number(101)).error.is_none());
        drop(input_sender);
        server_thread
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn stream_worker_pool_never_exceeds_configured_bound() {
        let instrumentation = Arc::new(TestInstrumentation::default());
        instrumentation.hold_workers.store(true, Ordering::Release);
        let (input_sender, input_receiver) = mpsc::channel();
        let output = SynchronizedOutput::default();
        let mut server = McpServer::new(None)
            .with_graph(chain_graph(16))
            .with_instrumentation(Arc::clone(&instrumentation));
        let server_output = output.clone();
        let server_thread = std::thread::spawn(move || {
            server.run_stream(ChannelReader::new(input_receiver), server_output)
        });

        for offset in 0..MAX_IN_FLIGHT_REQUESTS {
            let id = 200 + offset as i64;
            send_line(
                &input_sender,
                &format!(
                    r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"graphia_search_symbol","arguments":{{"query":"n1"}}}}}}"#
                ),
            );
            assert!(instrumentation.wait_until(|| {
                instrumentation.workers_started.load(Ordering::Acquire) > offset
            }));
        }
        assert_eq!(
            instrumentation.active_workers.load(Ordering::Acquire),
            MAX_IN_FLIGHT_REQUESTS
        );
        send_line(
            &input_sender,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"graphia_search_symbol","arguments":{{"query":"n2"}}}}}}"#,
                200 + MAX_IN_FLIGHT_REQUESTS
            ),
        );
        assert!(
            instrumentation.max_active_workers.load(Ordering::Acquire) <= MAX_IN_FLIGHT_REQUESTS
        );
        instrumentation.release_workers();

        for offset in 0..=MAX_IN_FLIGHT_REQUESTS {
            assert!(
                output
                    .wait_response(RequestId::Number(200 + offset as i64))
                    .error
                    .is_none()
            );
        }
        assert!(instrumentation.wait_until(|| {
            instrumentation.workers_started.load(Ordering::Acquire) == MAX_IN_FLIGHT_REQUESTS + 1
                && instrumentation.active_workers.load(Ordering::Acquire) == 0
        }));
        assert!(
            instrumentation.max_active_workers.load(Ordering::Acquire) <= MAX_IN_FLIGHT_REQUESTS
        );
        drop(input_sender);
        server_thread
            .join()
            .expect("server thread")
            .expect("server result");
    }
}
