# Knowledge Base

Orca doesn't build its own knowledge-graph system. It integrates graphify — a mature, battle-tested tool that already supports Claude Code, Codex, Gemini CLI, and others. Orca's job is to make graphify discoverable, install it into the right agent instruction files, run it on schedule, and expose its MCP server to agents that support MCP.

## What graphify does (recap)

[graphify](https://github.com/safishamsi/graphify) turns a folder of code, docs, and papers into a queryable knowledge graph. It's a skill/plugin that agents can invoke. Outputs:

- `graph.json` — the knowledge graph (nodes + edges + confidence tags)
- `GRAPH_REPORT.md` — a one-page summary with god nodes, communities, and suggested questions
- `graph.html` — an interactive visualization

It can be queried as an MCP server (`python -m graphify.serve graphify-out/graph.json`), exposing tools like `query_graph`, `get_node`, `get_neighbors`, `shortest_path`.

## Integration points

### 1. Installation

`orca kb init` checks if graphify is installed (`which graphify`). If not:

```
? graphify is not installed. Install now? (Y/n)
  [Y] pip install graphifyy        (recommended)
  [n] skip — you can install later and run `orca kb init` again
```

On install, also runs the platform-specific `graphify <platform> install` for each enabled Orca agent:
- `graphify claude install` (writes CLAUDE.md section + PreToolUse hook)
- `graphify codex install` (writes AGENTS.md + hook)
- `graphify gemini install` (writes GEMINI.md + hook)
- `graphify opencode install` (writes AGENTS.md + plugin)

Pi is handled separately — Orca registers graphify as a Pi skill via Pi's RPC (`{"type":"skill_install","skill":"graphify"}`).

### 2. Running the extraction

```
orca kb init
```

Executes `graphify . --mode normal` on the project root, writing to `.orca/kb/`. Orca's version of the output dir is `.orca/kb/` instead of graphify's default `graphify-out/`, set via `--out-dir`.

The daemon also offers:
- `orca kb update` — incremental update (only changed files)
- Auto-update on a schedule: config `[kb] auto_update = "daily"` or `"on_commit"`

### 3. MCP server lifecycle

If any enabled agent supports MCP (Claude Code does; Codex can via opt-in; Pi via extension), Orca can start the graphify MCP server and register it with those agents:

```
orca kb mcp start
```

This:
1. Spawns `python -m graphify.serve .orca/kb/graph.json` as a child process
2. Writes an MCP registration entry for each agent that supports it
3. Tracks the server lifecycle (restart on crash, stop on `orca daemon stop`)

For Claude Code, registration goes in `.mcp.json`:
```json
{
  "mcpServers": {
    "graphify": {
      "type": "stdio",
      "command": "python",
      "args": ["-m", "graphify.serve", ".orca/kb/graph.json"]
    }
  }
}
```

Orca writes this file if not present, or adds the graphify entry if present.

### 4. Task-level injection

When spawning an agent for a task, Orca's spawn context includes a KB preamble *if* KB is enabled for that task:

```markdown
# Orca injection

You are working on task T-007. Your workspace is an isolated git worktree.

## Knowledge base

A knowledge graph of this codebase is available at .orca/kb/GRAPH_REPORT.md.
Before searching files, read that report to orient yourself.

Query the graph directly with:
  graphify query "<natural language question>" --graph .orca/kb/graph.json

Or use the MCP tool `graphify` if available in your environment.

## Task

(task.toml contents rendered here)

## Context files

(files from task.context_files attached)
```

For agents with PreToolUse hooks (Claude Code, Codex, Gemini CLI, OpenCode), graphify's installed hook will *also* fire before file operations and nudge toward graph-first navigation. The preamble + hook are complementary.

### 5. Orca-level KB commands

From the TUI (`k` keybind) or CLI:

```bash
orca kb query "what connects train.py to model.py?"
orca kb path DigestAuth Response
orca kb explain SwinTransformer
```

These all shell out to `graphify <subcmd>`. Orca adds value only by:
- Knowing where the graph lives (`.orca/kb/graph.json`)
- Logging the query+response to `events.jsonl` (so you can see which KB queries ran during a task)
- Rendering the output in the TUI overlay if invoked from there

## Failure modes

| Problem | Behavior |
|---|---|
| graphify not installed | `orca kb` commands emit a helpful error; task execution continues without KB preamble |
| graphify extraction fails | `orca kb init` reports the error with traceback; state is left untouched |
| MCP server crashes | Daemon restarts it (up to 3 times in 60s); if still failing, marks MCP unavailable and disables the preamble's MCP mention |
| Graph out of date (commits since last update) | TUI shows a `kb: stale` indicator; one-key `U` to update |

## What Orca does NOT do

- Does not re-implement graph extraction
- Does not ship its own vector DB or retrieval layer
- Does not modify graphify's output format
- Does not proxy between agents and graphify — they talk to graphify directly via the skill or MCP

This is deliberate. Graphify is better at its job than we'll ever be. Our job is integration and lifecycle management.

## Alternative KB backends (future)

The config schema leaves room:

```toml
[skills]
graphify = { enabled = true, mcp = true }
# Future alternatives:
# understand-anything = { enabled = false }
# custom-rag = { enabled = false, mcp_command = "..." }
```

In v0.3 or later, we can add adapters for Understand-Anything, a vector-DB RAG, or user-custom backends. The Orca-side contract is small: install/install-per-agent, run-extraction, query-text, provide-mcp-server-optionally. Anything that implements that contract is a valid KB backend.

## Storage budget

Graph files are small (typically <10MB for a repo with a few hundred files, per graphify's benchmarks). Cache and transcripts can grow — limit `.orca/kb/cache/` to a user-configurable cap (default 1GB). On hit, Orca warns and offers to prune oldest entries.
