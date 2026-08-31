# Daemon Protocol Specification

The `graphia daemon` provides event-driven, real-time synchronization between filesystem codebases and Graphia's native in-memory graph index.

## Architecture

1. **Watcher (`src/daemon/watcher.rs`)**:
   - Uses `notify` recursive watcher rooted at the repository directory.
   - Respects exclusion policy: ignores `.git`, `.graphia`, `target`, `node_modules`, `dist`, `build`, `__pycache__`, `venv`, temp files (`.tmp-*`, `*~`, `*.swp`), etc.
   - Filters events based on supported language extensions (Rust, Python, TS/JS, Go, C/C++, Java, C#, Kotlin, PHP, Ruby, Zig, Swift).

2. **Debouncing & Coalescing (`src/daemon/debounce.rs`)**:
   - Windowed debouncer (default 100ms) buffering high-frequency filesystem events.
   - Merges atomic editor saves (e.g. temporary file writes, moves, renames) into clean semantic operations:
     - `Created(PathBuf)`
     - `Modified(PathBuf)`
     - `Removed(PathBuf)`
     - `Renamed { from: PathBuf, to: PathBuf }`

3. **Bounded Update Queue (`src/daemon/update.rs`)**:
   - Fixed-capacity FIFO queue (default 1,000 actions).
   - If incoming rate or burst exceeds queue capacity, queue transitions to `Dirty` state and flushes pending actions, triggering full safe incremental reconciliation without data loss.

4. **Snapshot Isolation & Generation Management (`src/daemon/state.rs`)**:
   - Uses `Arc<RwLock<LiveSnapshot>>` for thread-safe concurrent reads.
   - Monotonic `GraphGeneration(u64)` counter incremented on each successful graph update.
   - Readers access fully valid immutable `Arc<Graph>` instances that remain uncorrupted even while background updates occur.

5. **Daemon Status & Coordination File**:
   - Stored at `.graphia/daemon.json` inside the repository.
   - Schema:
     ```json
     {
       "running": true,
       "pid": 12345,
       "repo_root": "/path/to/repo",
       "generation": 42,
       "node_count": 150,
       "edge_count": 320,
       "last_update_ms": 1725100000000,
       "dirty": false,
       "pending_events": 0
     }
     ```
   - Automatically cleaned up on graceful shutdown.

## CLI Usage

- Start daemon:
  ```bash
  graphia daemon --repo /path/to/repo [--debounce-ms 100]
  ```
- Inspect status:
  ```bash
  graphia daemon status --repo /path/to/repo [--format human|json]
  # or
  graphia daemon-status --repo /path/to/repo [--format human|json]
  ```
