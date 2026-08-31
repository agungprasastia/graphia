use std::io::{BufRead, BufReader, Read, Write};

use super::error::{McpError, Result};
use super::protocol::{JsonRpcRequest, JsonRpcResponse};

/// Stdio transport reader reading line-delimited JSON-RPC messages from a buffered input stream.
pub struct StdioReader<R> {
    reader: BufReader<R>,
    buffer: String,
}

impl<R: Read> StdioReader<R> {
    pub fn new(read: R) -> Self {
        Self {
            reader: BufReader::new(read),
            buffer: String::new(),
        }
    }

    /// Read next JSON-RPC request/notification from stream.
    /// Returns `None` on EOF.
    pub fn read_message(&mut self) -> Result<Option<JsonRpcRequest>> {
        self.buffer.clear();
        let bytes_read = self.reader.read_line(&mut self.buffer)?;
        if bytes_read == 0 {
            return Ok(None);
        }

        let trimmed = self.buffer.trim();
        if trimmed.is_empty() {
            // Recurse to skip empty lines if any
            return self.read_message();
        }

        let request: JsonRpcRequest = serde_json::from_str(trimmed)
            .map_err(|e| McpError::Parse(format!("Failed to parse JSON-RPC line: {e}")))?;
        Ok(Some(request))
    }
}

/// Stdio transport writer writing newline-delimited JSON-RPC responses to an output stream.
/// Ensures that each message is written on a single line and flushed immediately.
pub struct StdioWriter<W> {
    writer: W,
}

impl<W: Write> StdioWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    /// Write JSON-RPC response as a single newline-terminated line and flush.
    pub fn write_response(&mut self, response: &JsonRpcResponse) -> Result<()> {
        let json = serde_json::to_string(response).map_err(|e| {
            McpError::Internal(format!("Failed to serialize JSON-RPC response: {e}"))
        })?;
        self.writer.write_all(json.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Write raw JSON line and flush.
    pub fn write_raw_json(&mut self, value: &serde_json::Value) -> Result<()> {
        let json = serde_json::to_string(value)
            .map_err(|e| McpError::Internal(format!("Failed to serialize JSON: {e}")))?;
        self.writer.write_all(json.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stdio_reader_parses_lines() {
        let input = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\"}\n";
        let cursor = Cursor::new(input.as_bytes());
        let mut reader = StdioReader::new(cursor);

        let msg1 = reader.read_message().unwrap().unwrap();
        assert_eq!(msg1.method, "tools/list");

        let msg2 = reader.read_message().unwrap().unwrap();
        assert_eq!(msg2.method, "tools/call");

        let eof = reader.read_message().unwrap();
        assert!(eof.is_none());
    }

    #[test]
    fn stdio_writer_writes_clean_lines() {
        let mut buf = Vec::new();
        {
            let mut writer = StdioWriter::new(&mut buf);
            let resp = JsonRpcResponse::success(
                super::super::protocol::RequestId::Number(1),
                serde_json::json!({"status": "ok"}),
            );
            writer.write_response(&resp).unwrap();
        }

        let written = String::from_utf8(buf).unwrap();
        assert!(written.ends_with('\n'));
        assert_eq!(written.lines().count(), 1);
        let parsed: serde_json::Value = serde_json::from_str(written.trim()).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["result"]["status"], "ok");
    }
}
