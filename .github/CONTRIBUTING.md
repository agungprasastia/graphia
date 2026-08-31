# Contributing to Graphia

Thank you for your interest in contributing to **Graphia**!

## Core Principles

Graphia is built as a high-performance, deterministic repository intelligence engine with strict architectural boundaries:
- **Zero LLMs in Core**: Graphia generates deterministic graph intelligence consumed by humans and AI agents.
- **Zero Runtime Dependencies**: No runtime Python, no SQLite, no cloud/API keys.
- **Zero Warnings Policy**: All code must compile with `0 compiler warnings` and `0 clippy warnings` (`-D warnings`).

---

## Development Setup

1. **Prerequisites**:
   - Rust toolchain 1.85+ (2024 edition).

2. **Clone & Test**:
   ```bash
   git clone https://github.com/agungprasastia/graphia.git
   cd graphia

   # Run test suite
   cargo test --all-targets --all-features

   # Check formatting and strict lints
   cargo fmt --check
   cargo clippy --all-targets --all-features -- -D warnings
   ```

3. **M4.1.3 verification gates**:
   - No `#[allow(...)]` warning suppressions in production, tests, or benchmarks.
   - Selective incremental updates must preserve canonical clean-build equivalence.
   - `graphia flow` must use value-flow edges; structural `Calls`, `Contains`, and `Imports` alone are not data flow.
   - MCP cancellation and benchmark child-stage behavior require focused regression coverage.

---

## Pull Request Guidelines

1. Ensure all changes include focused deterministic fixtures/tests.
2. New features must not weaken the zero-warning or bounded resource policies.
3. Keep pull requests focused on a single responsibility.
