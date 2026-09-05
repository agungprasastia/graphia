use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{GraphiaError, Result};
use crate::graph::{Graph, build_graph};
use crate::model::{Confidence, Edge, EdgeKind, Language, Node, NodeKind, SourceLocation};
use crate::parser::parse_bytes;
use crate::scan::{ScannedFile, scan_repo};

const MAGIC: &[u8; 4] = b"GRPH";
const VERSION: u32 = 3;
const ENDIAN_MARKER: u32 = 0x0102_0304;
const HEADER_SIZE: usize = 96;
const MIN_NODE_RECORD_SIZE: usize = 42;
const MIN_EDGE_RECORD_SIZE: usize = 27;
pub(crate) const FILE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializedGraph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub path: String,
    pub size: u64,
    pub modified_ns: Option<u128>,
    pub hash: [u8; 32],
    pub language: Option<Language>,
    pub parser_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub schema_version: u32,
    pub files: Vec<FileMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileChange {
    Added,
    Modified,
    Deleted,
    Unchanged,
}

#[derive(Debug, Clone)]
pub struct FileChangeRecord {
    pub path: String,
    pub change: FileChange,
}

/// Build graph from supported repository files.
///
/// # Errors
///
/// Returns an error when repository scanning fails.
pub fn build_graph_from_repo(root: &Path) -> Result<Graph> {
    let files = scan_repo(root)?;
    let parsed_files = parse_scanned_files(&files)?;
    let mut graph = build_graph(parsed_files);
    graph.set_source_root(root.to_path_buf());
    graph.resolve_cross_file()?;
    graph.canonicalize()?;
    Ok(graph)
}

fn parse_scanned_files(
    files: &[ScannedFile],
) -> Result<Vec<(String, Option<Language>, crate::parser::ParsedFile)>> {
    let mut parsed_files = Vec::with_capacity(files.len());
    for scanned in files {
        let Some(language) = scanned.language else {
            continue;
        };
        let content = match fs::read(&scanned.absolute_path) {
            Ok(content) => content,
            Err(error) => {
                return Err(GraphiaError::Io {
                    path: scanned.absolute_path.clone(),
                    message: error.to_string(),
                });
            }
        };
        parsed_files.push((
            scanned.relative_path.clone(),
            Some(language),
            parse_bytes(&scanned.relative_path, language, &content)?,
        ));
    }
    Ok(parsed_files)
}

fn canonical_graph(graph: &Graph) -> SerializedGraph {
    let mut nodes = graph.nodes.clone();
    let mut edges = graph.edges.clone();
    nodes.sort_by(|a, b| {
        a.qualified_name
            .cmp(&b.qualified_name)
            .then(a.kind.code().cmp(&b.kind.code()))
            .then(a.file.cmp(&b.file))
            .then(a.id.0.cmp(&b.id.0))
    });
    edges.sort_by(|a, b| {
        a.kind
            .code()
            .cmp(&b.kind.code())
            .then(a.from.0.cmp(&b.from.0))
            .then(a.to.0.cmp(&b.to.0))
            .then(a.confidence.code().cmp(&b.confidence.code()))
            .then(a.label.cmp(&b.label))
            .then(a.id.0.cmp(&b.id.0))
    });
    SerializedGraph { nodes, edges }
}

/// Render graph as canonical JSON string.
pub fn graph_to_json_string(graph: &Graph) -> Result<String> {
    let mut graph = graph.clone();
    graph.canonicalize()?;
    serde_json::to_string_pretty(&canonical_graph(&graph)).map_err(|error| GraphiaError::Storage {
        message: error.to_string(),
    })
}

/// Save graph as canonical JSON using an atomic replacement.
pub fn save_graph_json(graph: &Graph, output: &Path) -> Result<()> {
    let json = graph_to_json_string(graph)?;
    atomic_write(output, json.as_bytes())
}

/// Load graph from canonical JSON.
pub fn load_graph_json(path: &Path) -> Result<Graph> {
    let data = fs::read_to_string(path).map_err(|error| GraphiaError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let serialized: SerializedGraph =
        serde_json::from_str(&data).map_err(|error| GraphiaError::Storage {
            message: error.to_string(),
        })?;
    let graph = Graph::new(serialized.nodes, serialized.edges);
    graph.validate()?;
    Ok(graph)
}

/// Save explicit versioned native graph index.
pub fn save_graph_binary(graph: &Graph, output: &Path) -> Result<()> {
    graph.validate()?;
    let canonical = canonical_graph(graph);
    let mut nodes = Vec::new();
    for node in &canonical.nodes {
        write_node(&mut nodes, node)?;
    }
    let mut edges = Vec::new();
    for edge in &canonical.edges {
        write_edge(&mut edges, edge)?;
    }
    let node_offset = HEADER_SIZE as u64;
    let edge_offset = node_offset
        .checked_add(nodes.len() as u64)
        .ok_or_else(|| storage_error("graph index size overflow"))?;
    let mut sections = Vec::with_capacity(nodes.len() + edges.len());
    sections.extend_from_slice(&nodes);
    sections.extend_from_slice(&edges);
    let checksum = Sha256::digest(&sections);
    let mut data = Vec::with_capacity(HEADER_SIZE + sections.len());
    data.extend_from_slice(MAGIC);
    data.extend_from_slice(&VERSION.to_le_bytes());
    data.extend_from_slice(&ENDIAN_MARKER.to_le_bytes());
    data.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    data.extend_from_slice(&node_offset.to_le_bytes());
    data.extend_from_slice(&(nodes.len() as u64).to_le_bytes());
    data.extend_from_slice(&edge_offset.to_le_bytes());
    data.extend_from_slice(&(edges.len() as u64).to_le_bytes());
    data.extend_from_slice(&(canonical.nodes.len() as u64).to_le_bytes());
    data.extend_from_slice(&(canonical.edges.len() as u64).to_le_bytes());
    data.extend_from_slice(&checksum);
    data.extend_from_slice(&sections);
    atomic_write(output, &data)
}

/// Load native index and reject invalid header, version, counts, or checksum.
pub fn load_graph_binary(path: &Path) -> Result<Graph> {
    let data = fs::read(path).map_err(|error| GraphiaError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if data.len() < HEADER_SIZE || &data[..4] != MAGIC {
        return Err(storage_error("invalid graph index header"));
    }
    let version = read_u32(&data[4..8])?;
    if version != VERSION {
        return Err(storage_error("unsupported graph index version"));
    }
    if read_u32(&data[8..12])? != ENDIAN_MARKER || read_u32(&data[12..16])? as usize != HEADER_SIZE
    {
        return Err(storage_error(
            "invalid graph index byte order or header size",
        ));
    }
    let node_offset = usize::try_from(read_u64(&data[16..24])?)
        .map_err(|_| storage_error("graph index offset exceeds platform limits"))?;
    let node_len = usize::try_from(read_u64(&data[24..32])?)
        .map_err(|_| storage_error("graph index section exceeds platform limits"))?;
    let edge_offset = usize::try_from(read_u64(&data[32..40])?)
        .map_err(|_| storage_error("graph index offset exceeds platform limits"))?;
    let edge_len = usize::try_from(read_u64(&data[40..48])?)
        .map_err(|_| storage_error("graph index section exceeds platform limits"))?;
    let node_count = usize::try_from(read_u64(&data[48..56])?)
        .map_err(|_| storage_error("graph index node count exceeds platform limits"))?;
    let edge_count = usize::try_from(read_u64(&data[56..64])?)
        .map_err(|_| storage_error("graph index edge count exceeds platform limits"))?;
    let node_end = node_offset
        .checked_add(node_len)
        .ok_or_else(|| storage_error("graph index offset overflow"))?;
    let edge_end = edge_offset
        .checked_add(edge_len)
        .ok_or_else(|| storage_error("graph index offset overflow"))?;
    if node_offset != HEADER_SIZE || node_end != edge_offset || edge_end != data.len() {
        return Err(storage_error("invalid graph index section offsets"));
    }
    let sections = &data[HEADER_SIZE..];
    if Sha256::digest(sections).as_slice() != &data[64..96] {
        return Err(storage_error("graph index checksum mismatch"));
    }
    if node_count > node_len / MIN_NODE_RECORD_SIZE || edge_count > edge_len / MIN_EDGE_RECORD_SIZE
    {
        return Err(storage_error("graph index counts exceed section lengths"));
    }
    let mut cursor = Cursor::new(&data[node_offset..node_end]);
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(read_node(&mut cursor)?);
    }
    let mut edge_cursor = Cursor::new(&data[edge_offset..edge_end]);
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        edges.push(read_edge(&mut edge_cursor)?);
    }
    if cursor.position() != node_len as u64 || edge_cursor.position() != edge_len as u64 {
        return Err(storage_error("trailing graph index section data"));
    }
    let graph = Graph::new(nodes, edges);
    graph.validate()?;
    Ok(graph)
}

pub fn save_metadata(root: &Path, metadata: &Metadata) -> Result<()> {
    let data = serde_json::to_vec_pretty(metadata).map_err(|error| GraphiaError::Storage {
        message: error.to_string(),
    })?;
    atomic_write(&root.join(".graphia/metadata.json"), &data)
}

pub fn load_metadata(root: &Path) -> Result<Option<Metadata>> {
    let path = root.join(".graphia/metadata.json");
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path).map_err(|error| GraphiaError::Io {
        path: path.clone(),
        message: error.to_string(),
    })?;
    let metadata: Metadata =
        serde_json::from_str(&data).map_err(|error| GraphiaError::Storage {
            message: error.to_string(),
        })?;
    if metadata.schema_version != FILE_SCHEMA_VERSION {
        return Ok(None);
    }
    Ok(Some(metadata))
}

pub fn metadata_for_files(files: &[ScannedFile]) -> Result<Metadata> {
    let mut records = Vec::with_capacity(files.len());
    for file in files {
        let metadata = fs::metadata(&file.absolute_path).map_err(|error| GraphiaError::Io {
            path: file.absolute_path.clone(),
            message: error.to_string(),
        })?;
        let content = fs::read(&file.absolute_path).map_err(|error| GraphiaError::Io {
            path: file.absolute_path.clone(),
            message: error.to_string(),
        })?;
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos());
        records.push(FileMetadata {
            path: file.relative_path.clone(),
            size: metadata.len(),
            modified_ns,
            hash: Sha256::digest(content).into(),
            language: file.language,
            parser_version: FILE_SCHEMA_VERSION,
        });
    }
    records.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Metadata {
        schema_version: FILE_SCHEMA_VERSION,
        files: records,
    })
}

pub fn compare_metadata(previous: Option<&Metadata>, current: &Metadata) -> Vec<FileChangeRecord> {
    let old = previous.map_or(&[][..], |metadata| metadata.files.as_slice());
    let old_by_path: std::collections::BTreeMap<_, _> =
        old.iter().map(|file| (file.path.as_str(), file)).collect();
    let current_by_path: std::collections::BTreeMap<_, _> = current
        .files
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();
    let mut changes = Vec::new();
    for file in &current.files {
        let change = match old_by_path.get(file.path.as_str()) {
            None => FileChange::Added,
            Some(previous)
                if previous.size != file.size
                    || previous.hash != file.hash
                    || previous.language != file.language
                    || previous.parser_version != file.parser_version =>
            {
                FileChange::Modified
            }
            Some(_) => FileChange::Unchanged,
        };
        changes.push(FileChangeRecord {
            path: file.path.clone(),
            change,
        });
    }
    for file in old {
        if !current_by_path.contains_key(file.path.as_str()) {
            changes.push(FileChangeRecord {
                path: file.path.clone(),
                change: FileChange::Deleted,
            });
        }
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes
}

pub fn build_or_update(root: &Path, clean: bool) -> Result<(Graph, Vec<FileChangeRecord>)> {
    let scanned = scan_repo(root)?;
    let current = metadata_for_files(&scanned)?;
    if clean {
        match fs::remove_file(root.join(".graphia/parsed.json")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(GraphiaError::Io {
                    path: root.join(".graphia/parsed.json"),
                    message: error.to_string(),
                });
            }
        }
    }
    let previous = if clean { None } else { load_metadata(root)? };
    if !clean && previous.is_none() && root.join(".graphia/metadata.json").exists() {
        match fs::remove_file(root.join(".graphia/parsed.json")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(GraphiaError::Io {
                    path: root.join(".graphia/parsed.json"),
                    message: error.to_string(),
                });
            }
        }
    }
    let changes = compare_metadata(previous.as_ref(), &current);
    let graph = crate::incremental::update_repository(root)?;
    Ok((graph, changes))
}

/// Ensure `.graphia/.gitignore` exists to keep local graph data and caches out of git.
pub fn ensure_graphia_gitignore(dir: &Path) {
    let mut current = Some(dir);
    while let Some(d) = current {
        if d.file_name().and_then(|n| n.to_str()) == Some(".graphia") {
            let gitignore = d.join(".gitignore");
            if !gitignore.exists() {
                let content = b"# Graphia data files \xe2\x80\x94 local to each machine, not for committing.\n# Ignore everything in .graphia/ except this file itself.\n*\n!.gitignore\n";
                let _ = fs::write(&gitignore, content);
            }
            break;
        }
        current = d.parent();
    }
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|error| GraphiaError::Io {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
        ensure_graphia_gitignore(parent);
    }
    let (tmp, mut file) = temporary_file(path)?;
    let cleanup = TempCleanup(tmp.clone());
    file.write_all(bytes).map_err(|error| GraphiaError::Io {
        path: tmp.clone(),
        message: error.to_string(),
    })?;
    file.sync_all().map_err(|error| GraphiaError::Io {
        path: tmp.clone(),
        message: error.to_string(),
    })?;
    drop(file);
    let replacement = replace_file(&tmp, path);
    if let Err(error) = replacement {
        let _ = fs::remove_file(&tmp);
        return Err(GraphiaError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        });
    }
    std::mem::forget(cleanup);
    Ok(())
}

fn temporary_path(path: &Path, attempt: u64) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    tmp.push(format!(
        ".tmp-{}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed),
        attempt
    ));
    PathBuf::from(tmp)
}

fn temporary_file(path: &Path) -> Result<(PathBuf, fs::File)> {
    for attempt in 0..100 {
        let tmp = temporary_path(path, attempt);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => return Ok((tmp, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(GraphiaError::Io {
                    path: tmp,
                    message: error.to_string(),
                });
            }
        }
    }
    Err(storage_error("temporary file name collision"))
}

struct TempCleanup(PathBuf);

impl Drop for TempCleanup {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    unsafe extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let replaced: Vec<u16> = destination.as_os_str().encode_wide().chain([0]).collect();
    let replacement: Vec<u16> = source.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: buffers are NUL-terminated UTF-16 paths valid for this call; null optional pointers are documented.
    let result = unsafe {
        ReplaceFileW(
            replaced.as_ptr(),
            replacement.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            fs::rename(source, destination)
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}

fn storage_error(message: &str) -> GraphiaError {
    GraphiaError::Storage {
        message: message.to_string(),
    }
}

fn write_u32(buffer: &mut Vec<u8>, value: usize) -> Result<()> {
    let value =
        u32::try_from(value).map_err(|_| storage_error("graph exceeds binary format limits"))?;
    buffer.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_u64(buffer: &mut Vec<u8>, value: u64) -> Result<()> {
    buffer.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_node(buffer: &mut Vec<u8>, node: &Node) -> Result<()> {
    write_u64(buffer, node.id.0)?;
    buffer.push(node.kind.code());
    buffer.push(node.language.map_or(0, Language::code));
    buffer.push(node.visibility.code());
    write_string(buffer, &node.name)?;
    write_string(buffer, &node.qualified_name)?;
    write_string(buffer, &node.file)?;
    write_optional_string(buffer, node.signature.as_deref())?;
    write_optional_string(buffer, node.container.as_deref())?;
    write_location(buffer, &node.location)
}

fn write_edge(buffer: &mut Vec<u8>, edge: &Edge) -> Result<()> {
    write_u64(buffer, edge.id.0)?;
    buffer.push(edge.kind.code());
    buffer.push(edge.confidence.code());
    write_u64(buffer, edge.from.0)?;
    write_u64(buffer, edge.to.0)?;
    write_optional_string(buffer, edge.label.as_deref())
}

fn write_string(buffer: &mut Vec<u8>, value: &str) -> Result<()> {
    write_u32(buffer, value.len())?;
    buffer.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_optional_string(buffer: &mut Vec<u8>, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => {
            buffer.push(1);
            write_string(buffer, value)?;
        }
        None => buffer.push(0),
    }
    Ok(())
}

fn write_location(buffer: &mut Vec<u8>, location: &SourceLocation) -> Result<()> {
    write_string(buffer, &location.file)?;
    for value in [
        location.start_line,
        location.start_col,
        location.end_line,
        location.end_col,
    ] {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn read_u32(bytes: &[u8]) -> Result<u32> {
    let array: [u8; 4] = bytes
        .try_into()
        .map_err(|_| storage_error("short binary field"))?;
    Ok(u32::from_le_bytes(array))
}

fn read_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes
        .try_into()
        .map_err(|_| storage_error("short binary field"))?;
    Ok(u64::from_le_bytes(array))
}

fn read_u32_from(cursor: &mut Cursor<&[u8]>) -> Result<u32> {
    let mut bytes = [0; 4];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| storage_error("truncated graph index"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64_from(cursor: &mut Cursor<&[u8]>) -> Result<u64> {
    let mut bytes = [0; 8];
    cursor
        .read_exact(&mut bytes)
        .map_err(|_| storage_error("truncated graph index"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_byte(cursor: &mut Cursor<&[u8]>) -> Result<u8> {
    let mut byte = [0];
    cursor
        .read_exact(&mut byte)
        .map_err(|_| storage_error("truncated graph index"))?;
    Ok(byte[0])
}

fn read_string(cursor: &mut Cursor<&[u8]>) -> Result<String> {
    let length = read_u32_from(cursor)? as usize;
    let end = cursor
        .position()
        .checked_add(length as u64)
        .ok_or_else(|| storage_error("string length overflow"))?;
    if end > cursor.get_ref().len() as u64 {
        return Err(storage_error("truncated graph index string"));
    }
    let start = cursor.position() as usize;
    let value = String::from_utf8(cursor.get_ref()[start..end as usize].to_vec())
        .map_err(|_| storage_error("invalid UTF-8 in graph index"))?;
    cursor.set_position(end);
    Ok(value)
}

fn read_optional_string(cursor: &mut Cursor<&[u8]>) -> Result<Option<String>> {
    match read_byte(cursor)? {
        0 => Ok(None),
        1 => Ok(Some(read_string(cursor)?)),
        _ => Err(storage_error("invalid optional string marker")),
    }
}

fn read_location(cursor: &mut Cursor<&[u8]>) -> Result<SourceLocation> {
    Ok(SourceLocation {
        file: read_string(cursor)?,
        start_line: read_u32_from(cursor)?,
        start_col: read_u32_from(cursor)?,
        end_line: read_u32_from(cursor)?,
        end_col: read_u32_from(cursor)?,
    })
}

fn read_node(cursor: &mut Cursor<&[u8]>) -> Result<Node> {
    let id = crate::model::NodeId(read_u64_from(cursor)?);
    let kind = NodeKind::from_code(read_byte(cursor)?)
        .ok_or_else(|| storage_error("invalid node kind"))?;
    let language_code = read_byte(cursor)?;
    let language = if language_code == 0 {
        None
    } else {
        Some(Language::from_code(language_code).ok_or_else(|| storage_error("invalid language"))?)
    };
    let visibility = crate::model::Visibility::from_code(read_byte(cursor)?)
        .ok_or_else(|| storage_error("invalid visibility"))?;
    let name = read_string(cursor)?;
    let qualified_name = read_string(cursor)?;
    let file = read_string(cursor)?;
    let signature = read_optional_string(cursor)?;
    let container = read_optional_string(cursor)?;
    let location = read_location(cursor)?;
    Ok(Node {
        id,
        kind,
        language,
        visibility,
        name,
        qualified_name,
        file,
        signature,
        container,
        location,
    })
}

fn read_edge(cursor: &mut Cursor<&[u8]>) -> Result<Edge> {
    let id = crate::model::EdgeId(read_u64_from(cursor)?);
    let kind = EdgeKind::from_code(read_byte(cursor)?)
        .ok_or_else(|| storage_error("invalid edge kind"))?;
    let confidence = Confidence::from_code(read_byte(cursor)?)
        .ok_or_else(|| storage_error("invalid confidence"))?;
    Ok(Edge {
        id,
        kind,
        confidence,
        from: crate::model::NodeId(read_u64_from(cursor)?),
        to: crate::model::NodeId(read_u64_from(cursor)?),
        label: read_optional_string(cursor)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EdgeId, NodeId};
    use tempfile::tempdir;

    #[test]
    fn binary_round_trip_and_corruption_detection() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("index.bin");
        let graph = Graph::new(Vec::new(), Vec::new());
        save_graph_binary(&graph, &path).expect("write index");
        assert_eq!(load_graph_binary(&path).expect("read index"), graph);
        let mut bytes = fs::read(&path).expect("read bytes");
        bytes[0] ^= 1;
        fs::write(&path, bytes).expect("corrupt index");
        assert!(load_graph_binary(&path).is_err());
    }

    #[test]
    fn binary_rejects_trailing_bytes_and_bad_section_offsets() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("index.bin");
        save_graph_binary(&Graph::new(Vec::new(), Vec::new()), &path).expect("write index");
        let mut bytes = fs::read(&path).expect("read bytes");
        bytes.push(1);
        fs::write(&path, &bytes).expect("append index");
        assert!(load_graph_binary(&path).is_err());

        save_graph_binary(&Graph::new(Vec::new(), Vec::new()), &path).expect("rewrite index");
        let mut bytes = fs::read(&path).expect("read bytes");
        bytes[16] = 1;
        fs::write(&path, bytes).expect("corrupt offset");
        assert!(load_graph_binary(&path).is_err());
    }

    fn sample_graph() -> Graph {
        Graph::new(
            vec![Node {
                id: crate::graph::stable_node_id(&crate::model::NodeIdentity::new(
                    Some(Language::Rust),
                    "lib.rs",
                    NodeKind::Function,
                    "lib.rs::run",
                    None,
                    None,
                )),
                kind: NodeKind::Function,
                language: Some(Language::Rust),
                visibility: crate::model::Visibility::Public,
                name: "run".to_string(),
                qualified_name: "lib.rs::run".to_string(),
                file: "lib.rs".to_string(),
                signature: None,
                container: None,
                location: SourceLocation {
                    file: "lib.rs".to_string(),
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 4,
                },
            }],
            Vec::new(),
        )
    }

    fn rewrite_checksum(bytes: &mut [u8]) {
        let checksum: [u8; 32] = Sha256::digest(&bytes[HEADER_SIZE..]).into();
        bytes[64..96].copy_from_slice(&checksum);
    }

    #[test]
    fn binary_rejects_invalid_header_fields() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("index.bin");
        save_graph_binary(&Graph::new(Vec::new(), Vec::new()), &path).expect("write index");
        for (offset, value) in [(4, 999_u32), (8, 0_u32), (12, 95_u32)] {
            let mut bytes = fs::read(&path).expect("read index");
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            rewrite_checksum(&mut bytes);
            fs::write(&path, bytes).expect("corrupt header");
            assert!(matches!(
                load_graph_binary(&path),
                Err(GraphiaError::Storage { .. })
            ));
        }
    }

    #[test]
    fn binary_rejects_truncation_and_count_overflow_without_allocating() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("index.bin");
        save_graph_binary(&Graph::new(Vec::new(), Vec::new()), &path).expect("write index");
        let mut bytes = fs::read(&path).expect("read index");
        bytes[48..56].copy_from_slice(&u64::MAX.to_le_bytes());
        rewrite_checksum(&mut bytes);
        fs::write(&path, bytes).expect("corrupt count");
        assert!(matches!(
            load_graph_binary(&path),
            Err(GraphiaError::Storage { .. })
        ));
        let mut bytes = fs::read(&path).expect("read index");
        bytes.pop();
        fs::write(&path, bytes).expect("truncate index");
        assert!(matches!(
            load_graph_binary(&path),
            Err(GraphiaError::Storage { .. })
        ));
    }

    #[test]
    fn binary_rejects_enum_utf8_optional_and_dangling_edge_corruption() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("index.bin");
        save_graph_binary(&sample_graph(), &path).expect("write index");
        for offset in [104, 105, 106] {
            save_graph_binary(&sample_graph(), &path).expect("rewrite index");
            let mut bytes = fs::read(&path).expect("read index");
            bytes[offset] = 255;
            rewrite_checksum(&mut bytes);
            fs::write(&path, bytes).expect("corrupt record");
            assert!(matches!(
                load_graph_binary(&path),
                Err(GraphiaError::Storage { .. })
            ));
        }
        save_graph_binary(&sample_graph(), &path).expect("rewrite index");
        let mut bytes = fs::read(&path).expect("read index");
        bytes[111] = 255;
        rewrite_checksum(&mut bytes);
        fs::write(&path, bytes).expect("corrupt utf8");
        assert!(matches!(
            load_graph_binary(&path),
            Err(GraphiaError::Storage { .. })
        ));

        let node_id = sample_graph().nodes[0].id;
        let edge_identity = crate::model::EdgeIdentity::new(
            node_id,
            node_id,
            EdgeKind::Calls,
            Confidence::Extracted,
            Some("label".to_string()),
        );
        let edge = Edge {
            id: crate::graph::stable_edge_id(&edge_identity),
            kind: EdgeKind::Calls,
            from: node_id,
            to: node_id,
            confidence: Confidence::Extracted,
            label: Some("label".to_string()),
        };
        let valid_edge = Edge {
            to: node_id,
            ..edge
        };
        let graph = Graph::new(sample_graph().nodes, vec![valid_edge]);
        save_graph_binary(&graph, &path).expect("write edge");
        let mut bytes = fs::read(&path).expect("read index");
        let edge_offset = usize::try_from(read_u64(&bytes[32..40]).expect("edge offset"))
            .expect("edge offset fits");
        bytes[edge_offset + 26] = 255;
        rewrite_checksum(&mut bytes);
        fs::write(&path, bytes).expect("corrupt optional marker");
        assert!(matches!(
            load_graph_binary(&path),
            Err(GraphiaError::Storage { .. })
        ));
        save_graph_binary(&graph, &path).expect("rewrite edge");
        let mut bytes = fs::read(&path).expect("read index");
        let edge_offset = usize::try_from(read_u64(&bytes[32..40]).expect("edge offset"))
            .expect("edge offset fits");
        bytes[edge_offset + 16..edge_offset + 24].copy_from_slice(&999_u64.to_le_bytes());
        rewrite_checksum(&mut bytes);
        fs::write(&path, bytes).expect("corrupt dangling edge");
        assert!(matches!(
            load_graph_binary(&path),
            Err(GraphiaError::GraphInvariant { .. })
        ));
    }

    #[test]
    fn metadata_detects_content_change_and_delete() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a.rs");
        fs::write(&path, "fn a() {}").expect("write file");
        let files = scan_repo(dir.path()).expect("scan");
        let first = metadata_for_files(&files).expect("metadata");
        fs::write(&path, "fn changed() {}").expect("modify file");
        let files = scan_repo(dir.path()).expect("scan");
        let second = metadata_for_files(&files).expect("metadata");
        assert_eq!(
            compare_metadata(Some(&first), &second)[0].change,
            FileChange::Modified
        );
        fs::remove_file(path).expect("delete file");
        let files = scan_repo(dir.path()).expect("scan");
        let third = metadata_for_files(&files).expect("metadata");
        assert_eq!(
            compare_metadata(Some(&second), &third)[0].change,
            FileChange::Deleted
        );
    }

    #[test]
    fn graph_ids_use_fixed_width_values() {
        assert_eq!(NodeId(7).0, 7);
        assert_eq!(EdgeId(9).0, 9);
    }
}
