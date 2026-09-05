# Graphia Command Routing

## High-value MCP tools

| Need | MCP tool | Core arguments |
|---|---|---|
| Unified symbol context | `graphia_explore` | `query`, optional `depth` |
| Ranked symbol lookup | `graphia_search_symbol` | `query`, optional `limit`, `kind`, `file` |
| Exact symbol details | `graphia_get_symbol` | `symbol` |
| Inbound calls | `graphia_find_callers` | `symbol`, optional `depth`, `limit` |
| Outbound calls | `graphia_find_callees` | `symbol`, optional `depth`, `limit` |
| All structural references | `graphia_find_references` | `symbol`, optional `limit` |
| Shortest dependency route | `graphia_dependency_path` | `from`, `to`, optional `max_depth` |
| Bounded local graph | `graphia_neighborhood` | `symbol`, optional `depth`, `limit` |
| Blast radius | `graphia_impact` | `symbol`, optional `depth` |
| Relevant tests | `graphia_find_tests` | optional `symbol` or `file` |
| Repository overview | `graphia_architecture` | none |
| Token-budgeted context | `graphia_context` | one of `symbol`, `query`, or changed-file selection plus budget options |

Use `graphia_explore` first for a named symbol. Call specialized tools only when their narrower output is needed.

## CLI lifecycle

```bash
graphia init --yes       # project setup, agent MCP configs, initial index
graphia init --no-skill  # project setup without changing agent skills
graphia init --skill-scope project
graphia skill status     # check embedded skill against installed copies
graphia skill install    # install missing user-global copies
graphia skill update     # repair or refresh user-global copies
graphia build .          # create/update index when missing
graphia update .         # incremental refresh after edits
graphia stats .          # compact index health summary
graphia load .           # load and validate index
```

Run lifecycle commands only when state changes are authorized. Query commands are read-only.

## Analysis routing

| Question | Command |
|---|---|
| Cycles | `graphia cycles . --level file` |
| Hotspots | `graphia hotspots . --limit 10` |
| Communities | `graphia communities . --level module` |
| Entry points | `graphia entrypoints . --format json` |
| Architecture boundaries | `graphia architecture-check . --config architecture.toml --format json` |
| Source-to-sink flow | `graphia flow --source <source> --sink <sink>` |
| Dead code candidates | `graphia deadcode` |
| Git churn | `graphia history --max-commits 100` |
| Co-change | `graphia cochange --min-support 0.1` |
| Index diff | `graphia diff <old-index> <new-index>` |
| Public API diff | `graphia api-diff <old-index> <new-index>` |

Before using optional flags, confirm current syntax with `graphia <command> --help`.

## Token discipline

- Prefer JSON over human reports for agent processing.
- Start at depth 1–2 and limit 10–20.
- Use context budgets proportional to task size; 2,000–8,000 approximate tokens is typical.
- Ask one structural question per command.
- Keep only relevant fields from results when reporting to the user.
- Do not follow Graphia with repository-wide grep unless results show the index is incomplete.
