use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::error::{McpError, Result};
use super::protocol::{
    CallToolParams, Implementation, InitializeParams, InitializeResult, JsonRpcRequest,
    JsonRpcResponse, ListToolsResult, RequestId, ServerCapabilities, ToolsCapability,
};
use super::tools::{call_tool, get_tool_definitions};
use super::transport::{StdioReader, StdioWriter};
use crate::graph::Graph;

pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "graphia-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// MCP Server handling JSON-RPC requests, session state, and tool execution.
pub struct McpServer {
    repo_root: PathBuf,
    graph: Option<Graph>,
    initialized: bool,
}

impl McpServer {
    /// Create a new MCP server instance bound to an optional repository root.
    #[must_use]
    pub fn new(repo_root: Option<PathBuf>) -> Self {
        let repo_root = repo_root
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Self {
            repo_root,
            graph: None,
            initialized: false,
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

            let graph = if root.join(".graphia/index.bin").exists() {
                crate::storage::load_graph_binary(&root.join(".graphia/index.bin"))
                    .map_err(|e| McpError::Internal(format!("Failed to load binary index: {e}")))?
            } else if root.join("graph.json").exists() {
                crate::storage::load_graph_json(&root.join("graph.json"))
                    .map_err(|e| McpError::Internal(format!("Failed to load graph.json: {e}")))?
            } else {
                crate::storage::build_graph_from_repo(root)
                    .map_err(|e| McpError::Internal(format!("Failed to build graph: {e}")))?
            };

            self.graph = Some(graph);
        }

        Ok(())
    }

    /// Load or build graph for the repository root and return a reference.
    pub fn load_graph(&mut self) -> Result<&Graph> {
        self.ensure_graph_loaded()?;
        Ok(self.graph.as_ref().expect("graph initialized"))
    }

    /// Set an explicit pre-built graph (useful for tests and in-memory execution).
    pub fn with_graph(mut self, graph: Graph) -> Self {
        self.graph = Some(graph);
        self
    }

    /// Run the server on the provided input and output streams.
    /// Standard error can be used for server logging without contaminating stdout.
    pub fn run_stream<R: Read, W: Write>(&mut self, input: R, output: W) -> Result<()> {
        let mut reader = StdioReader::new(input);
        let mut writer = StdioWriter::new(output);

        while let Some(request) = reader.read_message()? {
            if request.is_notification() {
                self.handle_notification(&request)?;
            } else {
                let id = request
                    .id
                    .clone()
                    .unwrap_or(RequestId::String("null".to_string()));
                let response = self.handle_request(request, id);
                writer.write_response(&response)?;
            }
        }

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
            "notifications/cancelled" => {
                eprintln!("[graphia-mcp] Client cancelled request");
            }
            _ => {
                eprintln!("[graphia-mcp] Received notification: {}", request.method);
            }
        }
        Ok(())
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

        let graph = self.graph.as_ref().expect("graph loaded");
        let root_ref = Some(self.repo_root.as_path());
        match call_tool(
            graph,
            root_ref,
            &call_params.name,
            call_params.arguments.as_ref(),
        ) {
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
