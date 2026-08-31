use std::fmt;

use super::protocol::JsonRpcErrorObject;

/// Standard JSON-RPC 2.0 and MCP error codes.
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // Custom / MCP error codes
    pub const REPO_NOT_FOUND: i32 = -32000;
    pub const GRAPH_NOT_BUILT: i32 = -32001;
    pub const PATH_TRAVERSAL_DETECTED: i32 = -32002;
    pub const TOOL_EXECUTION_ERROR: i32 = -32003;
    pub const UNINITIALIZED: i32 = -32004;
}

#[derive(Debug)]
pub enum McpError {
    Parse(String),
    InvalidRequest(String),
    MethodNotFound(String),
    InvalidParams(String),
    Internal(String),
    RepoNotFound(String),
    GraphNotBuilt(String),
    PathTraversal(String),
    ToolExecution(String),
    Uninitialized(String),
    Io(std::io::Error),
}

impl fmt::Display for McpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "Parse error: {msg}"),
            Self::InvalidRequest(msg) => write!(f, "Invalid request: {msg}"),
            Self::MethodNotFound(msg) => write!(f, "Method not found: {msg}"),
            Self::InvalidParams(msg) => write!(f, "Invalid params: {msg}"),
            Self::Internal(msg) => write!(f, "Internal error: {msg}"),
            Self::RepoNotFound(msg) => write!(f, "Repository not found: {msg}"),
            Self::GraphNotBuilt(msg) => write!(f, "Graph not built: {msg}"),
            Self::PathTraversal(msg) => write!(f, "Path traversal forbidden: {msg}"),
            Self::ToolExecution(msg) => write!(f, "Tool execution failed: {msg}"),
            Self::Uninitialized(msg) => write!(f, "Server not initialized: {msg}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for McpError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for McpError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for McpError {
    fn from(err: serde_json::Error) -> Self {
        Self::Parse(err.to_string())
    }
}

impl From<crate::error::GraphiaError> for McpError {
    fn from(err: crate::error::GraphiaError) -> Self {
        Self::Internal(err.to_string())
    }
}

impl McpError {
    #[must_use]
    pub fn to_jsonrpc_error(&self) -> JsonRpcErrorObject {
        match self {
            Self::Parse(msg) => JsonRpcErrorObject::new(error_codes::PARSE_ERROR, msg),
            Self::InvalidRequest(msg) => JsonRpcErrorObject::new(error_codes::INVALID_REQUEST, msg),
            Self::MethodNotFound(msg) => {
                JsonRpcErrorObject::new(error_codes::METHOD_NOT_FOUND, msg)
            }
            Self::InvalidParams(msg) => JsonRpcErrorObject::new(error_codes::INVALID_PARAMS, msg),
            Self::Internal(msg) => JsonRpcErrorObject::new(error_codes::INTERNAL_ERROR, msg),
            Self::RepoNotFound(msg) => JsonRpcErrorObject::new(error_codes::REPO_NOT_FOUND, msg),
            Self::GraphNotBuilt(msg) => JsonRpcErrorObject::new(error_codes::GRAPH_NOT_BUILT, msg),
            Self::PathTraversal(msg) => {
                JsonRpcErrorObject::new(error_codes::PATH_TRAVERSAL_DETECTED, msg)
            }
            Self::ToolExecution(msg) => {
                JsonRpcErrorObject::new(error_codes::TOOL_EXECUTION_ERROR, msg)
            }
            Self::Uninitialized(msg) => JsonRpcErrorObject::new(error_codes::UNINITIALIZED, msg),
            Self::Io(err) => JsonRpcErrorObject::new(error_codes::INTERNAL_ERROR, err.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, McpError>;
