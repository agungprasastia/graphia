use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use super::error::{McpError, Result};
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
    request: JsonRpcRequest,
    id: RequestId,
    repo_root: PathBuf,
    graph: Option<Arc<Graph>>,
    initialized: bool,
    cancellations: Arc<Mutex<BTreeMap<RequestId, CancellationToken>>>,
    token: Option<CancellationToken>,
}

/// MCP Server handling JSON-RPC requests, session state, and tool execution.
pub struct McpServer {
    repo_root: PathBuf,
    graph: Option<Arc<Graph>>,
    initialized: bool,
    auto_index: bool,
    cancellations: Arc<Mutex<BTreeMap<RequestId, CancellationToken>>>,
    request_token: Option<CancellationToken>,
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
                        let mut server = McpServer {
                            repo_root: job.repo_root,
                            graph: job.graph,
                            initialized: job.initialized,
                            auto_index: false,
                            cancellations: Arc::clone(&job.cancellations),
                            request_token: job.token,
                        };
                        let response = server.handle_request(job.request, job.id);
                        let _ = response_tx.send(response);
                    }
                });
            }
            drop(response_tx);

            let mut eof = false;
            let mut pending = 0usize;
            let mut response_order = VecDeque::new();
            let mut completed = BTreeMap::new();
            loop {
                while let Ok(response) = response_rx.try_recv() {
                    completed.insert(response.id.clone(), response);
                }
                while let Some(id) = response_order.front() {
                    let Some(response) = completed.remove(id) else {
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
                        let token = (request.method == "tools/call").then(CancellationToken::new);
                        if let Some(token) = &token
                            && let Ok(mut registry) = self.cancellations.lock()
                        {
                            registry.insert(id.clone(), token.clone());
                        }
                        let job = StreamJob {
                            request,
                            id: id.clone(),
                            repo_root: self.repo_root.clone(),
                            graph,
                            initialized: self.initialized,
                            cancellations: Arc::clone(&self.cancellations),
                            token,
                        };
                        match job_tx.try_send(job) {
                            Ok(()) => {
                                response_order.push_back(id);
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
                            Err(mpsc::TrySendError::Disconnected(_)) => break,
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
        if let Some(val) = params {
            if let Ok(init_params) = serde_json::from_value::<InitializeParams>(val) {
                eprintln!(
                    "[graphia-mcp] Initializing MCP session with protocol version: {}",
                    init_params.protocol_version
                );
            }
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
                if let Some(path_str) = val.as_str() {
                    if (key == "file" || key.contains("path")) && self.is_path_traversal(path_str) {
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
        if let Ok(mut registry) = self.cancellations.lock() {
            registry.remove(&id);
        }
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
            if let Ok(canonical_repo) = self.repo_root.canonicalize() {
                if let Ok(canonical_p) = p.canonicalize() {
                    return !canonical_p.starts_with(canonical_repo);
                }
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
        assert_eq!(tools.len(), 11);
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
}
