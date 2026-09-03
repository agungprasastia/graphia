use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::analysis::AnalysisLevel;
use crate::analysis::cycles::{CycleConfig, find_cycles};
use crate::analysis::hotspots::compute_hotspots;
use crate::analysis::projection::project_graph;
use crate::graph::Graph;
use crate::intelligence::explore::explore_symbol;
use crate::intelligence::neighborhood::{NeighborhoodOptions, get_neighborhood};
use crate::intelligence::search::{SearchOptions, search_graph};

pub const INDEX_HTML: &str = include_str!("assets/index.html");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub file_count: usize,
    pub languages: Vec<(String, usize)>,
    pub hotspots_count: usize,
    pub cycles_count: usize,
    pub initial_symbol: Option<String>,
}

pub struct UiServer {
    graph: Arc<Graph>,
    repo_root: PathBuf,
    port: u16,
    running: Arc<AtomicBool>,
}

impl UiServer {
    pub fn new(graph: Graph, repo_root: PathBuf, port: u16) -> Self {
        Self {
            graph: Arc::new(graph),
            repo_root,
            port,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn start(&self) -> std::io::Result<u16> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", self.port))?;
        let actual_port = listener.local_addr()?.port();

        let graph = Arc::clone(&self.graph);
        let repo_root = self.repo_root.clone();
        let running = Arc::clone(&self.running);

        // Precompute stats once
        let stats = compute_stats(&graph);
        let stats = Arc::new(stats);

        std::thread::spawn(move || {
            // Set listener non-blocking or short timeout to check running flag
            listener.set_nonblocking(true).ok();

            while running.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let g = Arc::clone(&graph);
                        let root = repo_root.clone();
                        let s = Arc::clone(&stats);
                        std::thread::spawn(move || {
                            handle_connection(stream, &g, &root, &s);
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(15));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(actual_port)
    }
}

fn compute_stats(graph: &Graph) -> UiStats {
    let mut files = HashSet::new();
    let mut lang_counts: HashMap<String, usize> = HashMap::new();

    for node in &graph.nodes {
        files.insert(node.file.clone());
        let lang = node
            .language
            .map(|l| l.as_str().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        *lang_counts.entry(lang).or_insert(0) += 1;
    }

    let mut languages: Vec<(String, usize)> = lang_counts.into_iter().collect();
    languages.sort_by_key(|a| std::cmp::Reverse(a.1));

    // Compute basic cycles and hotspots
    let projected = project_graph(graph, AnalysisLevel::Symbol, None);
    let adj = projected.to_adjacency();
    let cycles = find_cycles(&adj, CycleConfig::default());
    let hotspots = compute_hotspots(&adj);

    let initial_symbol = hotspots
        .first()
        .map(|h| h.id.clone())
        .or_else(|| graph.nodes.first().map(|n| n.qualified_name.clone()));

    UiStats {
        node_count: graph.node_count(),
        edge_count: graph.edge_count(),
        file_count: files.len(),
        languages,
        hotspots_count: hotspots.len(),
        cycles_count: cycles.len(),
        initial_symbol,
    }
}

fn handle_connection(mut stream: TcpStream, graph: &Graph, repo_root: &Path, stats: &UiStats) {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();

    let mut reader = BufReader::new(&stream);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return;
    }

    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        return;
    }

    let method = parts[0];
    let full_path = parts[1];

    if method != "GET" {
        respond(
            &mut stream,
            "405 Method Not Allowed",
            "text/plain",
            b"Method Not Allowed",
        );
        return;
    }

    let (path, query_params) = parse_url(full_path);

    match path.as_str() {
        "/" | "/index.html" => {
            respond(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                INDEX_HTML.as_bytes(),
            );
        }
        "/api/stats" => {
            if let Ok(json) = serde_json::to_string(stats) {
                respond(&mut stream, "200 OK", "application/json", json.as_bytes());
            } else {
                respond(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain",
                    b"Serialization error",
                );
            }
        }
        "/api/search" => {
            let q = query_params.get("q").cloned().unwrap_or_default();
            let limit = query_params
                .get("limit")
                .and_then(|l| l.parse::<usize>().ok())
                .unwrap_or(20);

            let results = search_graph(
                graph,
                &SearchOptions {
                    query: q,
                    limit: Some(limit),
                    ..Default::default()
                },
            );
            if let Ok(json) = serde_json::to_string(&results) {
                respond(&mut stream, "200 OK", "application/json", json.as_bytes());
            } else {
                respond(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain",
                    b"Serialization error",
                );
            }
        }
        "/api/explore" => {
            let symbol = query_params.get("symbol").cloned().unwrap_or_default();
            let depth = query_params
                .get("depth")
                .and_then(|d| d.parse::<usize>().ok())
                .unwrap_or(2);

            match explore_symbol(graph, &symbol, depth, Some(repo_root)) {
                Some(result) => {
                    if let Ok(json) = serde_json::to_string(&result) {
                        respond(&mut stream, "200 OK", "application/json", json.as_bytes());
                    } else {
                        respond(
                            &mut stream,
                            "500 Internal Server Error",
                            "text/plain",
                            b"Serialization error",
                        );
                    }
                }
                None => {
                    respond(
                        &mut stream,
                        "404 Not Found",
                        "application/json",
                        b"{\"error\":\"symbol not found\"}",
                    );
                }
            }
        }
        "/api/neighborhood" => {
            let symbol = query_params.get("symbol").cloned().unwrap_or_default();
            let depth = query_params
                .get("depth")
                .and_then(|d| d.parse::<usize>().ok())
                .unwrap_or(1);

            let options = NeighborhoodOptions {
                target: symbol,
                depth,
                limit: 50,
            };

            match get_neighborhood(graph, &options) {
                Some(neighborhood) => {
                    if let Ok(json) = serde_json::to_string(&neighborhood) {
                        respond(&mut stream, "200 OK", "application/json", json.as_bytes());
                    } else {
                        respond(
                            &mut stream,
                            "500 Internal Server Error",
                            "text/plain",
                            b"Serialization error",
                        );
                    }
                }
                None => {
                    respond(
                        &mut stream,
                        "404 Not Found",
                        "application/json",
                        b"{\"error\":\"neighborhood not found\"}",
                    );
                }
            }
        }
        "/api/hotspots" => {
            let projected = project_graph(graph, AnalysisLevel::Symbol, None);
            let adj = projected.to_adjacency();
            let mut hotspots = compute_hotspots(&adj);
            hotspots.truncate(20);
            if let Ok(json) = serde_json::to_string(&hotspots) {
                respond(&mut stream, "200 OK", "application/json", json.as_bytes());
            } else {
                respond(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain",
                    b"Serialization error",
                );
            }
        }
        "/api/cycles" => {
            let projected = project_graph(graph, AnalysisLevel::Symbol, None);
            let adj = projected.to_adjacency();
            let cycles = find_cycles(&adj, CycleConfig::default());
            if let Ok(json) = serde_json::to_string(&cycles) {
                respond(&mut stream, "200 OK", "application/json", json.as_bytes());
            } else {
                respond(
                    &mut stream,
                    "500 Internal Server Error",
                    "text/plain",
                    b"Serialization error",
                );
            }
        }
        _ => {
            respond(&mut stream, "404 Not Found", "text/plain", b"Not Found");
        }
    }
}

fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn parse_url(raw: &str) -> (String, HashMap<String, String>) {
    let mut parts = raw.splitn(2, '?');
    let path = parts.next().unwrap_or("").to_string();
    let mut params = HashMap::new();

    if let Some(query) = parts.next() {
        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut kv = pair.splitn(2, '=');
            let k = kv.next().unwrap_or("");
            let v = kv.next().unwrap_or("");
            params.insert(url_decode(k), url_decode(v));
        }
    }

    (path, params)
}

fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let h1 = bytes.next().unwrap_or(0);
            let h2 = bytes.next().unwrap_or(0);
            let hex = [h1, h2];
            if let Ok(s) = std::str::from_utf8(&hex)
                && let Ok(val) = u8::from_str_radix(s, 16)
            {
                result.push(val as char);
            } else {
                result.push('%');
                if h1 != 0 {
                    result.push(h1 as char);
                }
                if h2 != 0 {
                    result.push(h2 as char);
                }
            }
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_decode() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("foo+bar"), "foo bar");
        assert_eq!(url_decode("symbol%3A%3Atest"), "symbol::test");
    }

    #[test]
    fn test_parse_url() {
        let (path, params) = parse_url("/api/search?q=foo%20bar&limit=10");
        assert_eq!(path, "/api/search");
        assert_eq!(params.get("q"), Some(&"foo bar".to_string()));
        assert_eq!(params.get("limit"), Some(&"10".to_string()));
    }
}
