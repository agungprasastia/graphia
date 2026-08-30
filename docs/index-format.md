# Graphia Binary Index Format

M0.3 stores graph data in `.graphia/index.bin`. Format is explicit, little-endian, versioned, and independent of Rust memory layout. Graph nodes and edges are separate contiguous sections.

## Header

Header size is 96 bytes:

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 4 | Magic `GRPH` |
| 4 | 4 | Format version, currently `2` |
| 8 | 4 | Endian marker `0x01020304` |
| 12 | 4 | Header size, currently `96` |
| 16 | 8 | Node section offset |
| 24 | 8 | Node section byte length |
| 32 | 8 | Edge section offset |
| 40 | 8 | Edge section byte length |
| 48 | 8 | Node record count |
| 56 | 8 | Edge record count |
| 64 | 32 | SHA-256 of bytes from offset 96 to EOF |

All integer fields use little-endian encoding. Loader rejects invalid magic, unsupported version, endian marker, header size, offsets, section lengths, counts, checksum, and trailing bytes.

## Node Section

Each node record is variable-length but uses fixed-width scalar fields:

1. `u64` node ID
2. `u8` node kind code
3. `u8` language code, or `0` for no language
4. length-prefixed UTF-8 name (`u32` byte length followed by bytes)
5. length-prefixed UTF-8 qualified name
6. length-prefixed UTF-8 file
7. source location: length-prefixed UTF-8 file, then four `u32` values for start line, start column, end line, and end column

## Edge Section

Each edge record contains:

1. `u64` edge ID
2. `u8` edge kind code
3. `u8` confidence code
4. `u64` source node ID
5. `u64` target node ID
6. optional label marker (`u8` `0` or `1`), followed by a length-prefixed UTF-8 label when present

Record lengths are derived from their fields. The loader consumes every byte in each section and then validates graph invariants, including edge endpoints.

## Write Guarantees

Writers canonicalize records before encoding and write through a temporary file, `sync_all`, and atomic replacement. Existing destination remains intact if encoding or replacement fails.

JSON remains the human-readable export format. Binary storage carries only logical graph values; it never persists raw Rust layout or serde representation.
