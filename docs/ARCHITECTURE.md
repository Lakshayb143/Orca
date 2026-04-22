# Architecture

## System at a glance

Orca is one Rust binary that runs in two modes:

- `orca` (foreground) — launches the ratatui TUI and ensures a daemon is running.
- `orca daemon` (background) — the long-lived process that owns state, watches the filesystem, dispatches tasks, and manages agent lifecycles.

The CLI subcommands (`orca task new`, `orca route`, `orca status`, etc.) are thin — they send requests to the daemon via a Unix socket and print the response.

## Process model

```
      ┌────────────────────────────────────────────────────────────┐
      │                       tmux session                         │
      │                                                            │
      │  ┌────────────────────┐   ┌──────────────┐   ┌──────────┐  │
      │  │ [0] orca control   │   │ [1] claude-  │   │ [2] codex│  │
      │  │     ratatui TUI    │   │     code     │   │          │  │
      │  └──────────┬─────────┘   └──────┬───────┘   └──────┬───┘  │
      │             │                    │                  │      │
      │  ┌──────────┴─────────┐   ┌──────┴───────┐   ┌──────┴───┐  │
      │  │ [4] status tail    │   │ [3] gemini   │   │ [5] pi   │  │
      │  └────────────────────┘   └──────────────┘   └──────────┘  │
      │                                                            │
      └────────────────────────────┬───────────────────────────────┘
                                   │
                                   │ Unix socket (.orca/state/daemon.sock)
                                   │ JSON-RPC over line-delimited JSON
                                   │
                         ┌─────────▼──────────┐
                         │      orcad         │
                         │   (daemon)         │
                         │                    │
                         │  ┌──────────────┐  │
                         │  │ state store  │  │  watches .orca/
                         │  ├──────────────┤  │
                         │  │ dispatcher   │  │  routes tasks
                         │  ├──────────────┤  │
                         │  │ agent pool   │  │  spawn/kill/monitor
                         │  ├──────────────┤  │
                         │  │ event bus    │  │  emits to events.jsonl
                         │  ├──────────────┤  │
                         │  │ KB client    │  │  talks to graphify MCP
                         │  ├──────────────┤  │
                         │  │ usage meter  │  │  tracks tokens/cost
                         │  └──────────────┘  │
                         └──────────┬─────────┘
                                    │
                                    ▼
                         ┌────────────────────┐
                         │    .orca/          │
                         │    filesystem      │
                         └────────────────────┘
```

### Why a daemon

A pure-CLI design would be simpler but can't do:
- Background reviews (review agent runs while user works on next task)
- Live usage metering (token/cost counters ticking in the TUI)
- Auto-retry on agent death (requires a watcher)
- Speculative review (cavekit-proven pattern — review tier N while building tier N+1)
- Cross-agent event ordering

So the daemon owns the world. Everything else is a client.

### Why one binary with subcommands

Easier to distribute (one artifact), easier to keep versions in sync between CLI and daemon (same build, same protocol), easier to test. The Rust pattern is `main.rs` dispatches to subcommand modules; the daemon is just `orca daemon` which calls into the `daemon` module's entry point.

## Component breakdown

### `state store`

- Holds the canonical in-memory representation of `.orca/`.
- Writes through to disk on every mutation (no in-memory-only state).
- On startup, reads disk and rebuilds in-memory model.
- Uses `notify` crate to watch the filesystem and reconcile external edits (someone hand-edits `task.toml` → daemon picks it up).
- Publishes change events to the event bus.

Key types (Rust):
```rust
struct Task {
    id: TaskId,               // e.g., "T-001"
    title: String,
    description: String,
    state: TaskState,
    assigned_to: Option<AgentId>,
    reviewer: Option<AgentId>,
    worktree: PathBuf,
    parent: Option<TaskId>,
    subtasks: Vec<TaskId>,
    created_at: OffsetDateTime,
    updated_at: OffsetDateTime,
    // ... see STATE.md for full schema
}

enum TaskState {
    Drafted, Planning, Planned, Implementing,
    Implemented, Reviewing, Reviewed, Revising,
    Done, Blocked(BlockReason), Parked,
}
```

### `dispatcher`

Decides (suggests, really) which agent should handle a task.

In v0, rules-based. The task's `capabilities` field is set at creation (either by the user or by a template) and the dispatcher matches it to available agents:

```rust
trait DispatchRule {
    fn suggest(&self, task: &Task, agents: &[AgentStatus]) -> Option<Suggestion>;
}

// Built-in rules:
struct LongContextRule;      // needs_long_context → gemini-cli
struct AdversarialReviewRule; // needs_review → codex (if not the builder)
struct MultiFileEditRule;    // default for complex tasks → claude-code
struct RpcUtilityRule;       // needs_rpc → pi
struct LocalModelRule;       // needs_local_model → opencode
```

Rules are ordered by priority and return the first match. If none match, fallback is `claude-code`.

**Crucially: the dispatcher never auto-routes.** It emits a `SuggestionReady` event with a ranked list. The user confirms through the TUI (or `orca route T-001 --accept` accepts the top suggestion).

### `agent pool`

Manages the lifecycle of agent instances. One pool per Orca session; one `AgentInstance` per active agent.

```rust
trait AgentAdapter: Send + Sync {
    fn id(&self) -> AgentId;
    fn capabilities(&self) -> Capabilities;
    async fn spawn(&self, ctx: SpawnContext) -> Result<AgentHandle>;
    async fn send_prompt(&self, h: &AgentHandle, prompt: &str) -> Result<()>;
    async fn status(&self, h: &AgentHandle) -> Result<AgentStatus>;
    async fn kill(&self, h: &AgentHandle) -> Result<()>;
}
```

Two concrete strategies:

- **`TmuxAdapter`** — spawns the agent in a tmux pane, uses `tmux send-keys` for prompts, uses `tmux capture-pane` + heuristics for status. Works for Claude Code, Codex, Gemini CLI, OpenCode.
- **`RpcAdapter`** — spawns the agent with stdin/stdout piped, speaks line-delimited JSONL over the pipes. Works for Pi.

Agent death detection:
- TmuxAdapter polls `tmux list-panes` and the underlying process; missing pane or zombie process = dead.
- RpcAdapter detects closed stdin/stdout or a heartbeat timeout.

On death: emit `AgentDied` event, set any tasks assigned to that agent to `Blocked(AgentDied)`, trigger the recovery flow (see TASK_LIFECYCLE.md).

### `event bus`

In-memory pub/sub for events. Every event is also appended to `.orca/state/events.jsonl` for durability and inspection.

Event types (non-exhaustive):
```
TaskCreated, TaskStateChanged, TaskAssigned, TaskCompleted, TaskBlocked
AgentSpawned, AgentHeartbeat, AgentDied, AgentUsageUpdate
SuggestionReady, UserDecision
KBQueryIssued, KBQueryReturned
```

Clients (the TUI, external observers via `orca watch`) subscribe via the Unix socket.

### `KB client`

Thin wrapper around graphify. On `orca kb init`, shells out to `graphify .` (or runs via MCP if configured). Exposes queries to agents via an injected prompt preamble ("your KB is at `.orca/kb/GRAPH_REPORT.md`, query with `/graphify query <...>`").

See `KB.md` for details.

### `usage meter`

Tracks tokens and cost per agent per task. Fed by:
- TmuxAdapter: parses agent-specific output for usage lines (Claude Code prints `[14.2k tokens · $0.18]`; Codex similar). Each adapter has a `parse_usage_line` method.
- RpcAdapter: Pi reports usage in its JSONL protocol natively.

Stored in `.orca/cache/tokens.db` (SQLite). Queryable for dashboards.

## Data flow: the happy path

User creates a task:

```
User types in TUI: "fix the referee false-positive in dark-kit frames"
     │
     ▼
TUI sends CreateTask RPC to daemon
     │
     ▼
Daemon's state store creates task.toml, emits TaskCreated event
     │
     ▼
Dispatcher sees TaskCreated, applies rules, emits SuggestionReady
     │   (capabilities: multi_file_edit, domain_specific → claude-code)
     ▼
TUI receives SuggestionReady, shows "Assign T-007 to claude-code? [Y/n/other]"
     │
     ▼
User hits Y. TUI sends UserDecision RPC.
     │
     ▼
Daemon transitions state Drafted → Planning, emits TaskAssigned
     │
     ▼
Agent pool spawns claude-code in new tmux pane with isolated git worktree
     │
     ▼
Adapter sends the task description + instructions to claude-code
     │
     ▼
[claude-code works, producing plan.md]
     │
     ▼
Adapter detects completion (heuristic: agent goes idle N seconds, or user hits "done")
     │
     ▼
Daemon transitions state Planning → Planned, waits for user ACK
     │
     ▼
User reviews plan, hits 'i' to implement
     │
     ▼
Daemon transitions Planned → Implementing (same agent continues)
     │
     ▼
[claude-code codes, commits to worktree]
     │
     ▼
Transition Implementing → Implemented
     │
     ▼
User hits 'r' to review. Dispatcher suggests codex (adversarial-review rule).
     │
     ▼
Second agent (codex) spawned in new pane, reviewing the worktree.
     │
     ▼
Codex produces review.md. State → Reviewed.
     │
     ▼
If findings: user decides to revise (state → Revising) or accept.
     │
     ▼
On accept: worktree merged into main, task → Done.
```

Every transition is a file write to `.orca/state/tasks/T-007/` AND an event append to `events.jsonl`. A power user can do any of this from the CLI without touching the TUI.

## Isolation model: git worktrees

Each task gets a git worktree at `.orca/worktrees/T-007/` on a branch `orca/T-007`. The agent's working directory is the worktree, not the repo root. Benefits:
- Changes are isolated until merge
- Multiple tasks can run in parallel (future) without conflicting
- Rolling back is `git worktree remove` + `git branch -D`
- Easy to diff what the agent did (`git diff main...orca/T-007`)

On task close, merge is either:
- Fast-forward merge into main (default for solo work)
- PR creation via `gh` CLI (if configured)
- Kept as a branch for manual merge (if user prefers)

User configurable in `config.toml`.

## Concurrency

v0 runs **one task at a time**. The daemon rejects `orca task start T-002` if T-001 is still active. This is a deliberate simplification:
- Humans can't reasonably watch 3 agents at once anyway
- State machine reasoning is 10× simpler
- Parallelism is a v0.2 feature (cavekit-style wave execution)

Within a single task, the daemon does allow **concurrent agents on different roles** (builder + speculative reviewer, for example). That's a narrower, more controlled form of parallelism.

## Error handling philosophy

Three failure modes, three responses:

1. **Agent process died** → emit event, block task, surface recovery UI.
2. **Agent produced bad output** (user's judgment call) → user hits `revise`, task returns to Revising state with feedback attached.
3. **Daemon crashed** → state on disk survives, restart recovers everything. Emit a `DaemonRestarted` event so the TUI can reconnect.

No try-again-silently. Every failure should be visible.

## Security & trust

- Orca runs agents with the user's own permissions. No sandbox. The agents can do anything the user can do. That's the user's choice.
- API keys are read from the user's env or agent-specific config files (`~/.claude/`, `~/.codex/`, etc.). Orca never stores keys.
- The daemon's Unix socket is chmod 600, owned by the user. No network listening in v0.
- `.orca/config.toml` is committed to git; secrets go in `.orca/secrets.toml` (gitignored) or env vars.

## Tech stack (Rust crates)

| Concern | Crate |
|---|---|
| Async runtime | `tokio` |
| TUI | `ratatui` + `crossterm` |
| Interactive prompts (wizard) | `inquire` |
| Filesystem watching | `notify` |
| Config parsing | `serde` + `toml` + `config` |
| CLI parsing | `clap` with derive |
| Unix sockets & IPC | `tokio::net::UnixListener` |
| Event serialization | `serde_json` |
| SQLite for usage | `rusqlite` or `sqlx` |
| Git operations | `git2` or shell-out to `git` |
| tmux interaction | shell-out to `tmux` (simpler than FFI) |
| Logging | `tracing` + `tracing-subscriber` |
| Error handling | `anyhow` (binaries) + `thiserror` (library types) |

Version pinning via `Cargo.lock` committed; MSRV TBD but likely stable at project start.

## Project layout (proposed)

```
orca/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── AGENTS.md                     # instructions for AI coding agents
├── docs/                         # the spec set
├── crates/
│   ├── orca-core/                # state, events, types — no I/O
│   ├── orca-daemon/              # daemon entrypoint + dispatcher
│   ├── orca-agents/              # agent adapters (one module per agent)
│   ├── orca-tui/                 # ratatui control pane
│   ├── orca-wizard/              # inquire-based setup
│   ├── orca-kb/                  # graphify integration
│   └── orca-cli/                 # binary crate (main + subcommand dispatch)
└── tests/
    ├── integration/
    └── fixtures/
```

Cargo workspace, one binary crate (`orca-cli`) that depends on the rest. Libraries are separated for testability and to keep the daemon's core logic independent of I/O concerns.

See `BUILD_PLAN.md` for the ordered list of crates to build and their milestones.
