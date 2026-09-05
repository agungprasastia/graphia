# 0.2.5 verification — 2026-09-05

Version metadata and the CLI report 0.2.5. No release tag or publication performed.

## Fixes from this audit

- Init propagates unreadable `.gitignore` errors without overwriting its content.
- Init distinguishes actual ignore rules from comments and negations.
- Graph construction computes imports once per file and reuses identical reference resolutions within that file/caller.
- Agent configuration rejects paths resolving outside the repository.
- OpenCode detects `opencode.jsonc` and accepts JSONC comments and trailing commas. Saving normalizes JSONC to formatted JSON, removing comments; settings are retained.
- Added a real binary stdin/stdout E2E test invoking all 12 MCP tools with semantic result assertions and sequential requests.

## Performance verification

Before this performance pass, self-indexing this repository did not finish within bounded attempts (over 120 seconds), including an optimized release binary. Scanning took approximately 0.09 seconds; independent parsing took approximately 0.5–0.9 seconds for 160 supported source files. The expensive stage was graph construction/resolution.

Resolver sessions now borrow the indexed engine immutably and memoize imported-file sets and completed top-level re-export results, including misses. Sessions are dropped after each graph pass, preventing stale results after index changes and avoiding persistent cache growth. Path matching borrows already-normalized paths and avoids allocating a suffix for every comparison. Public resolver APIs remain unchanged.

Independent review identified a traversal-order regression in an attempted recursive-request shortcut. That shortcut was removed; the original per-file visitation order is preserved and covered by a regression test.

Final Windows release measurements using `target/release/graphia.exe build . --clean`:

- Three clean runs: **3.598, 2.840, 2.765 seconds** (median **2.840 seconds**).
- Result: **1,613 nodes / 4,281 edges**, 277 scanned files; identical counts across all three runs.
- Unchanged-repository `build .`: **2.197 seconds**.
- Separate clean run: **3.049 seconds**, approximately **95.03 MiB peak working set**, sampled from the process's Windows peak-working-set counter every 20 ms.

These are local wall-clock measurements, not a universal latency or memory guarantee. The earlier attempts were interrupted rather than completed baseline measurements, and the source tree gained regression tests during the work; no exact before/after speedup or memory-reduction ratio is claimed. Cache memory scales with distinct imports and re-export queries within a pass. Larger repositories still need separate profiling.

## Coverage and limits

### Explicit export and init consent follow-up

CLI exports now require `--output`; direct CLI dispatch also rejects an absent destination before loading or building an index. Init lists possible repository/configuration and skill destinations before any writes and requires explicit interactive acceptance (default no). Non-interactive init rejects calls without `--yes`; `--yes --no-skill` allows unattended setup without skill writes. Existing unrelated configuration settings remain covered by regression tests.

Validation: **272 tests passed across 32 suites**, strict Clippy and formatting passed, and the release binary was rebuilt. Subprocess regressions verify that missing export destinations and unconfirmed init leave existing files untouched. Release smoke checks returned exit 2 for missing `--output` and exit 1 for unconfirmed non-interactive init. Interactive response parsing is unit-tested; a live terminal prompt was not manually exercised.

### Internal index location follow-up

Automatic JSON persistence now writes `.graphia/graph.json`; CLI build/update and init reuse the single persistence step instead of saving twice. CLI and UI readers prefer the binary index, then internal JSON, then legacy root JSON. Root files are not automatically deleted: they may be explicit exports. Explicit export destinations are unchanged.

Validation after this change: **270 tests passed across 32 suites**, including new-index priority, legacy fallback, root-export preservation, init/build/update placement, incremental JSON parity, and existing explicit-export tests. Release build and strict Clippy passed.

The all-targets suite includes parser/resolution regression tests, index persistence, incremental updates, daemon lifecycle, MCP protocol and tool behavior, exports, CLI initialization, and live HTTP UI endpoints. The new binary MCP test checks all 12 tools against parsed source rather than only an in-memory mock graph.

Final performance-pass validation: **268 tests passed across 32 suites**, `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean, and `git diff --check` clean. New resolver tests cover normalization equivalence, cache reuse/lifetime, and recursive multi-file traversal order.

Local validation runs on Windows. Unix symlink regressions and Unix installer syntax require Unix CI. `cargo-audit` is unavailable locally, so no current dependency-advisory clearance is claimed. Passing these tests does not establish that every repository size or agent configuration is bug-free.
