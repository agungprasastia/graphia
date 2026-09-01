use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(test)]
use std::sync::{Condvar, Mutex, atomic::AtomicUsize};
#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct TestInstrumentation {
    pub(crate) worker_started: AtomicBool,
    pub(crate) work_units: AtomicUsize,
    pub(crate) cancel_observed: AtomicBool,
    pub(crate) worker_finished: AtomicBool,
    pub(crate) active_workers: AtomicUsize,
    pub(crate) max_active_workers: AtomicUsize,
    pub(crate) workers_started: AtomicUsize,
    pub(crate) pause_work: AtomicBool,
    pub(crate) hold_workers: AtomicBool,
    state: Mutex<()>,
    changed: Condvar,
}

#[cfg(test)]
impl TestInstrumentation {
    pub(crate) fn record_worker_started(&self) {
        let mut state = self.state.lock().expect("instrumentation state");
        let active = self.active_workers.fetch_add(1, Ordering::AcqRel) + 1;
        self.max_active_workers.fetch_max(active, Ordering::AcqRel);
        self.workers_started.fetch_add(1, Ordering::AcqRel);
        self.worker_started.store(true, Ordering::Release);
        self.changed.notify_all();
        while self.hold_workers.load(Ordering::Acquire) {
            state = self.changed.wait(state).expect("instrumentation wait");
        }
    }

    pub(crate) fn record_worker_finished(&self) {
        let _state = self.state.lock().expect("instrumentation state");
        self.active_workers.fetch_sub(1, Ordering::AcqRel);
        self.worker_finished.store(true, Ordering::Release);
        self.changed.notify_all();
    }

    fn record_work_unit(&self, cancelled: &AtomicBool) {
        let mut state = self.state.lock().expect("instrumentation state");
        self.work_units.fetch_add(1, Ordering::AcqRel);
        self.changed.notify_all();
        while self.pause_work.load(Ordering::Acquire) && !cancelled.load(Ordering::Acquire) {
            state = self.changed.wait(state).expect("instrumentation wait");
        }
    }

    fn record_cancel_observed(&self) {
        let _state = self.state.lock().expect("instrumentation state");
        self.cancel_observed.store(true, Ordering::Release);
        self.changed.notify_all();
    }

    fn notify(&self) {
        let _state = self.state.lock().expect("instrumentation state");
        self.changed.notify_all();
    }

    pub(crate) fn release_workers(&self) {
        let _state = self.state.lock().expect("instrumentation state");
        self.hold_workers.store(false, Ordering::Release);
        self.changed.notify_all();
    }

    pub(crate) fn wait_until(&self, predicate: impl Fn() -> bool) -> bool {
        let state = self.state.lock().expect("instrumentation state");
        if predicate() {
            return true;
        }
        let (_state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(5), |_| !predicate())
            .expect("instrumentation wait");
        !timeout.timed_out() || predicate()
    }
}

#[derive(Debug, Clone)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    #[cfg(test)]
    instrumentation: Option<Arc<TestInstrumentation>>,
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            instrumentation: None,
        }
    }
}

impl CancellationToken {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(crate) fn with_instrumentation(instrumentation: Arc<TestInstrumentation>) -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            instrumentation: Some(instrumentation),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        #[cfg(test)]
        if let Some(instrumentation) = &self.instrumentation {
            instrumentation.notify();
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        let cancelled = self.cancelled.load(Ordering::Acquire);
        #[cfg(test)]
        if cancelled && let Some(instrumentation) = &self.instrumentation {
            instrumentation.record_cancel_observed();
        }
        cancelled
    }

    #[must_use]
    pub(crate) fn check_work_unit(&self) -> bool {
        #[cfg(test)]
        if let Some(instrumentation) = &self.instrumentation {
            instrumentation.record_work_unit(&self.cancelled);
        }
        self.is_cancelled()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelRequestParams {
    pub id: RequestId,
}

/// JSON-RPC 2.0 Request ID which can be numeric, string, or null.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
    Null,
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "{s}"),
            Self::Null => write!(f, "null"),
        }
    }
}

/// JSON-RPC 2.0 incoming request or notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    #[must_use]
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// JSON-RPC 2.0 outgoing response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcErrorObject>,
}

impl JsonRpcResponse {
    #[must_use]
    pub fn success(id: RequestId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    #[must_use]
    pub fn error(id: RequestId, error: JsonRpcErrorObject) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC 2.0 outgoing notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    #[must_use]
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcErrorObject {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcErrorObject {
    #[must_use]
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    #[must_use]
    pub fn with_data(code: i32, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

/// Implementation metadata for client or server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Implementation {
    pub name: String,
    pub version: String,
}

/// MCP Initialize request parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    #[serde(default)]
    pub capabilities: serde_json::Value,
    #[serde(default)]
    pub client_info: Option<Implementation>,
}

/// MCP Server capabilities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
}

/// MCP Tools capability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// MCP Initialize result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: Implementation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

/// MCP Tool schema definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// MCP tools/list result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// MCP tools/call request parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Option<serde_json::Map<String, serde_json::Value>>,
}

/// MCP content payload in tool execution results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
    Text { text: String },
}

/// MCP tools/call execution result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

impl CallToolResult {
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text { text: text.into() }],
            is_error: None,
        }
    }

    #[must_use]
    pub fn error(error_message: impl Into<String>) -> Self {
        Self {
            content: vec![Content::Text {
                text: error_message.into(),
            }],
            is_error: Some(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_id_serde() {
        let n: RequestId = serde_json::from_str("123").expect("parse num");
        assert_eq!(n, RequestId::Number(123));

        let s: RequestId = serde_json::from_str("\"req-1\"").expect("parse string");
        assert_eq!(s, RequestId::String("req-1".to_string()));

        let serialized = serde_json::to_string(&n).expect("serialize");
        assert_eq!(serialized, "123");
    }

    #[test]
    fn jsonrpc_request_deserialization() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).expect("parse req");
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.id, Some(RequestId::Number(1)));
        assert_eq!(req.method, "initialize");
        assert!(!req.is_notification());
    }

    #[test]
    fn jsonrpc_notification() {
        let json = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).expect("parse notif");
        assert_eq!(req.id, None);
        assert!(req.is_notification());
    }

    #[test]
    fn call_tool_result_serialization() {
        let res = CallToolResult::text("hello world");
        let json = serde_json::to_value(&res).expect("to value");
        assert_eq!(json["content"][0]["type"], "text");
        assert_eq!(json["content"][0]["text"], "hello world");
        assert!(json.get("isError").is_none());

        let err_res = CallToolResult::error("failed");
        let err_json = serde_json::to_value(&err_res).expect("to value");
        assert_eq!(err_json["isError"], true);
    }
}
