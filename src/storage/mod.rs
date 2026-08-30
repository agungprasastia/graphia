use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{GraphiaError, Result};
use crate::graph::{Graph, build_graph};
use crate::model::{Confidence, Edge, EdgeKind, Language, Node, NodeKind, SourceLocation};
use crate::parser::parse_file;
use crate::scan::{ScannedFile, scan_repo};

const MAGIC: &[u8; 4] = b"GRPH";
const VERSION: u32 = 2;
const ENDIAN_MARKER: u32 = 0x0102_0304;
const HEADER_SIZE: usize = 64;
const FILE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerializedGraph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileMetadata {
    path: String,
    size: u64,
    modified_ns: Option<u128>,
    hash: [u8; 32],
    language: Option<Language>,
    parser_version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Metadata {
    schema_version: u32,
    files: Vec<FileMetadata>,
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
    Ok(build_graph(parsed_files))
}

fn parse_scanned_files(
    files: &[ScannedFile],
) -> Result<Vec<(String, Option<Language>, crate::parser::ParsedFile)>> {
    let mut parsed_files = Vec::with_capacity(files.len());
    for scanned in files {
        let Some(language) = scanned.language else {
            continue;
        };
        let content = match fs::read_to_string(&scanned.absolute_path) {
            Ok(content) => content,
            Err(error) => {
                eprintln!("warning: skip {}: {error}", scanned.relative_path);
                continue;
            }
        };
        parsed_files.push((
            scanned.relative_path.clone(),
            Some(language),
            parse_file(&scanned.relative_path, language, &content),
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

/// Save graph as canonical JSON using an atomic replacement.
pub fn save_graph_json(graph: &Graph, output: &Path) -> Result<()> {
    let json = serde_json::to_vec_pretty(&canonical_graph(graph)).map_err(|error| {
        GraphiaError::Storage {
            message: error.to_string(),
        }
    })?;
    atomic_write(output, &json)
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
    let mut body = Vec::new();
    write_u32(&mut body, canonical.nodes.len())?;
    write_u32(&mut body, canonical.edges.len())?;
    for node in &canonical.nodes {
        write_u64(&mut body, node.id.0)?;
        body.push(node.kind.code());
        body.push(node.language.map_or(0, Language::code));
        write_string(&mut body, &node.name)?;
        write_string(&mut body, &node.qualified_name)?;
        write_string(&mut body, &node.file)?;
        write_location(&mut body, &node.location)?;
    }
    for edge in &canonical.edges {
        write_u64(&mut body, edge.id.0)?;
        body.push(edge.kind.code());
        body.push(edge.confidence.code());
        write_u64(&mut body, edge.from.0)?;
        write_u64(&mut body, edge.to.0)?;
        write_optional_string(&mut body, edge.label.as_deref())?;
    }
    let checksum = Sha256::digest(&body);
    let mut data = Vec::with_capacity(HEADER_SIZE + body.len());
    data.extend_from_slice(MAGIC);
    data.extend_from_slice(&VERSION.to_le_bytes());
    data.extend_from_slice(&ENDIAN_MARKER.to_le_bytes());
    data.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
    data.extend_from_slice(&(body.len() as u64).to_le_bytes());
    data.extend_from_slice(&(canonical.nodes.len() as u64).to_le_bytes());
    data.extend_from_slice(&(canonical.edges.len() as u64).to_le_bytes());
    data.extend_from_slice(&checksum);
    data.extend_from_slice(&body);
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
    let body_len = read_u64(&data[16..24])? as usize;
    let node_count = read_u64(&data[24..32])? as usize;
    let edge_count = read_u64(&data[32..40])? as usize;
    let expected_end = HEADER_SIZE
        .checked_add(body_len)
        .ok_or_else(|| storage_error("graph index size overflow"))?;
    if expected_end != data.len() {
        return Err(storage_error("graph index body length mismatch"));
    }
    let body = &data[HEADER_SIZE..];
    if Sha256::digest(body).as_slice() != &data[40..64] {
        return Err(storage_error("graph index checksum mismatch"));
    }
    let mut cursor = Cursor::new(body);
    let encoded_nodes = read_u32_from(&mut cursor)? as usize;
    let encoded_edges = read_u32_from(&mut cursor)? as usize;
    if encoded_nodes != node_count || encoded_edges != edge_count {
        return Err(storage_error("graph index count mismatch"));
    }
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        nodes.push(read_node(&mut cursor)?);
    }
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        edges.push(read_edge(&mut cursor)?);
    }
    if cursor.position() != body.len() as u64 {
        return Err(storage_error("trailing graph index data"));
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

fn load_metadata(root: &Path) -> Result<Option<Metadata>> {
    let path = root.join(".graphia/metadata.json");
    if !path.exists() {
        return Ok(None);
    }
    let data = fs::read_to_string(&path).map_err(|error| GraphiaError::Io {
        path: path.clone(),
        message: error.to_string(),
    })?;
    let metadata = serde_json::from_str(&data).map_err(|error| GraphiaError::Storage {
        message: error.to_string(),
    })?;
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
    let previous = if clean { None } else { load_metadata(root)? };
    let changes = compare_metadata(previous.as_ref(), &current);
    let graph = build_graph_from_repo(root)?;
    save_metadata(root, &current)?;
    Ok((graph, changes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|error| GraphiaError::Io {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    let tmp = temporary_path(path);
    let mut file = fs::File::create(&tmp).map_err(|error| GraphiaError::Io {
        path: tmp.clone(),
        message: error.to_string(),
    })?;
    file.write_all(bytes).map_err(|error| GraphiaError::Io {
        path: tmp.clone(),
        message: error.to_string(),
    })?;
    file.sync_all().map_err(|error| GraphiaError::Io {
        path: tmp.clone(),
        message: error.to_string(),
    })?;
    drop(file);
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(GraphiaError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        });
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
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
    Ok(Node {
        id,
        kind,
        language,
        name: read_string(cursor)?,
        qualified_name: read_string(cursor)?,
        file: read_string(cursor)?,
        location: read_location(cursor)?,
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
        bytes[HEADER_SIZE] ^= 1;
        fs::write(&path, bytes).expect("corrupt index");
        assert!(load_graph_binary(&path).is_err());
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
