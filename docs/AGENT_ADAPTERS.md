# Agent Adapters

Orca treats every coding agent as an opaque actor that it can spawn, send prompts to, observe, and kill. The abstraction is the `AgentAdapter` trait in `orca-agents`. This doc covers the trait, the two concrete strategies (`TmuxAdapter` and `RpcAdapter`), and per-agent integration notes for the five built-ins.

## The trait

```rust
// crates/orca-agents/src/adapter.rs

use async_trait::async_trait;
use std::path::PathBuf;

pub struct AgentId(pub String);         // "claude-code", "codex", ...

pub struct Capabilities {
    pub long_context: bool,
    pub multi_file_edit: bool,
    pub surgical_edit: bool,
    pub adversarial_review: bool,
    pub rpc_driven: bool,
    pub local_model: bool,
    pub domain_specific: bool,
    // Future: pub custom: Vec<String>,
}

pub struct SpawnContext {
    pub task_id: TaskId,
    pub role: Role,                     // Planner | Implementer | Reviewer | Utility
    pub workspace: PathBuf,             // git worktree root
    pub model: String,                  // resolved from config per role
    pub context_files: Vec<PathBuf>,
    pub kb_enabled: bool,
    pub extra_args: Vec<String>,
}

pub enum AgentHandle {
    Tmux { session: String, pane: String, pid: u32 },
    Rpc  { pid: u32, stdin: ChildStdin, stdout_rx: mpsc::Receiver<Value> },
}

pub struct AgentStatus {
    pub alive: bool,
    pub idle: bool,                     // no recent output
    pub last_output_at: OffsetDateTime,
    pub last_usage: Option<UsageUpdate>,
    pub current_output_tail: String,    // last ~2KB of stdout/pane
}

pub struct UsageUpdate {
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_usd: f64,
}

#[async_trait]
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> AgentId;
    fn capabilities(&self) -> Capabilities;

    async fn spawn(&self, ctx: SpawnContext) -> Result<AgentHandle>;
    async fn send_prompt(&self, h: &AgentHandle, prompt: &str) -> Result<()>;
    async fn status(&self, h: &AgentHandle) -> Result<AgentStatus>;
    async fn kill(&self, h: &AgentHandle) -> Result<()>;

    /// Adapter-specific completion heuristic. Default: idle > 30s.
    async fn is_complete(&self, h: &AgentHandle) -> Result<bool> { /* default */ }

    /// Parse a line of output for token/cost info. Called for every line.
    fn parse_usage_line(&self, line: &str) -> Option<UsageUpdate> { None }
}
```

The daemon holds `Vec<Box<dyn AgentAdapter>>`, one per configured agent.

## Strategy 1: TmuxAdapter

Used for all agents that only expose a TTY (Claude Code, Codex, Gemini CLI, OpenCode). Works like this:

```rust
pub struct TmuxAdapter {
    pub id: AgentId,
    pub command: String,                // e.g., "claude"
    pub capabilities: Capabilities,
    pub completion_heuristic: CompletionHeuristic,
    pub usage_parser: Box<dyn UsageParser>,
}
```

### spawn

1. Ensure a tmux session named `orca` exists (create if not).
2. Create a new pane in that session: `tmux new-window -t orca -n <agent-id>-<task-id>`.
3. In the new pane, `cd` to `ctx.workspace` and exec `<command> <extra_args>`.
4. Wait for the agent to print its ready prompt (regex match per agent, configurable).
5. Return a handle with the session, pane target (`orca:3.0`), and the PID of the child process (via `tmux display -p '#{pane_pid}'`).

### send_prompt

- `tmux send-keys -t <pane> -l "<prompt>"` for the literal text.
- Followed by `tmux send-keys -t <pane> Enter` to submit.
- For multi-line prompts, use temp file + the agent's file-input mechanism (Claude Code supports drag-drop markdown; Codex accepts stdin redirect). Simpler: write to temp file, send `@<path>` or equivalent per agent.

### status & completion heuristic

Status polling is a loop: `tmux capture-pane -p -t <pane> -S -200` every 2 seconds. The adapter keeps a running hash of the last 200 lines; if the hash doesn't change for `completion_heuristic.idle_seconds` (default 30), and the last line matches the agent's prompt regex, the task is likely complete.

This is heuristic and will occasionally misfire. The TUI always lets the user mark a task complete manually with `d` (done).

### parse_usage_line

Each agent has a regex-based parser. Example for Claude Code:

```rust
fn parse_usage_line(&self, line: &str) -> Option<UsageUpdate> {
    // Claude Code prints: "✢ 14.2k tokens used · $0.18 this turn"
    static RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(\d+\.?\d*)k tokens.*\$([0-9.]+)").unwrap()
    });
    let caps = RE.captures(line)?;
    let tokens = (caps[1].parse::<f64>().ok()? * 1000.0) as u64;
    let cost = caps[2].parse::<f64>().ok()?;
    Some(UsageUpdate { tokens_in: 0, tokens_out: tokens, cost_usd: cost })
}
```

These parsers are brittle by nature — agents change their output. The adapter is expected to be updated alongside agent releases. Include a test fixture of real output for each agent so regressions are caught.

### kill

`tmux kill-pane -t <pane>`. The pane closes, the child process gets SIGHUP, cleanup proceeds.

## Strategy 2: RpcAdapter

Used for Pi, which supports line-delimited JSONL over stdin/stdout. Much cleaner.

```rust
pub struct RpcAdapter {
    pub id: AgentId,
    pub command: String,                // e.g., "pi"
    pub rpc_args: Vec<String>,          // e.g., ["--rpc"]
    pub capabilities: Capabilities,
}
```

### spawn

1. `tokio::process::Command::new(&self.command).args(&self.rpc_args).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()`.
2. Send an `init` message over stdin: `{"type":"init","workspace":"/path","model":"..."}`.
3. Read until Pi responds `{"type":"ready","version":"..."}`.
4. Also spawn Pi into a tmux pane for human observation — the pane is read-only (Pi is driven by RPC, not keystrokes). This gives the user a live view while Orca drives.

### send_prompt

Write `{"type":"prompt","text":"..."}\n` to stdin. Pi streams responses back on stdout as JSONL events.

### status & completion

Pi's RPC protocol includes explicit events:
```json
{"type":"turn_start"}
{"type":"tool_call", "tool":"edit", "args":{...}}
{"type":"tool_result", ...}
{"type":"text", "content":"..."}
{"type":"turn_end", "usage":{"input":1234,"output":567,"cost_usd":0.02}}
```

`is_complete` is simply "did we see a `turn_end` since the last prompt?" Much more reliable than TmuxAdapter's hash heuristic.

### parse_usage

Structured. Just read the `usage` field from `turn_end`.

### kill

Close stdin, wait for graceful exit, SIGTERM, then SIGKILL if still alive after 5s.

## Per-agent integration notes

### claude-code

```toml
[agents.claude-code]
command = "claude"
```

- Spawn args: none by default; the agent opens in interactive mode.
- Ready prompt regex: `^\s*\>\s*$` (the `>` cursor).
- Prompt submission: `send-keys` + `Enter`. Multi-line uses the `/paste` command or `@file` references.
- Skills integration: claude-code reads `CLAUDE.md` at repo root. Orca writes an `CLAUDE.md` addendum section `<orca-injected>…</orca-injected>` on spawn that includes:
  - The current task spec (summary from task.toml)
  - KB hint if enabled: "Your KB is at `.orca/kb/GRAPH_REPORT.md`. Use `/graphify` to query."
  - Effort preset
- Model selection: via `ANTHROPIC_MODEL` env var on spawn.
- Completion heuristic: idle 30s + prompt regex match.
- Usage parsing: see example above.

### codex

```toml
[agents.codex]
command = "codex"
```

- Spawn args: `codex` interactive mode.
- Ready prompt: `^\s*❯\s*$`.
- Skills integration: codex reads `AGENTS.md`. Orca injects similarly.
- Model selection: `OPENAI_MODEL` env var or `/model` slash command on spawn.
- Reasoning effort: Codex supports `--reasoning-effort {low,medium,high}`; map from effort preset.
- Completion heuristic: same pattern as claude-code; different prompt regex.
- Usage parsing: Codex prints lines like `tokens: 8421 in, 1203 out · $0.18`. Regex accordingly.
- Special use: Codex is the default *reviewer* in cavekit and a strong choice for Orca's `adversarial_review` capability. The review prompt should include a "find flaws, don't accept"-style preamble.

### gemini-cli

```toml
[agents.gemini-cli]
command = "gemini"
```

- Spawn args: `gemini`, interactive TUI.
- Ready prompt: `^\s*>\s*$` (customizable).
- Skills integration: reads `GEMINI.md`. Orca injects.
- Model selection: `--model` flag or config.
- Killer use: long-context tasks. When Gemini is the assigned agent, the adapter's spawn should automatically prepend the entire relevant subset of the repo as attached context. A helper `attach_for_gemini(task)` gathers files from `task.context_files` + graphify-surfaced neighbors and adds them to the session's working context.
- Completion heuristic: same pattern; different regex.
- Usage parsing: Gemini's format TBD — capture real output during integration testing and write the parser then.

### pi

```toml
[agents.pi]
command = "pi"
extra_args = ["--rpc"]
```

- **Uses RpcAdapter, not TmuxAdapter.**
- Pi's `--rpc` mode: line-delimited JSONL on stdin/stdout. See https://shittycodingagent.ai/ (yes, really) for the protocol spec.
- Skills integration: Pi supports installable skills via its extension system; Orca registers graphify at daemon start via `pi-cli add skill graphify` if not present.
- Model selection: `/model` slash command or `--model` flag. Default to `kimi-k2.5:cloud` for cost unless overridden.
- Session persistence: Pi auto-saves sessions to `~/.pi/agent/sessions/`. Orca's adapter passes `--session <path>` pointing inside the task's worktree so sessions are isolated per task.
- Completion: explicit `turn_end` event. Reliable.
- Usage parsing: explicit in `turn_end.usage`. Reliable.
- **Role sweet spot**: utility/coordination work. Things like "summarize the last 5 commits on orca/T-003," "run this prompt template against these 20 files," "check if all PRs mentioning X have been merged." Scripted work where RPC determinism matters more than reasoning depth.

### opencode

```toml
[agents.opencode]
command = "opencode"
```

- Spawn args: `opencode` interactive mode.
- Ready prompt: `^\s*❯\s*$` (similar to Codex — OpenCode is also Go + Bubble Tea).
- Skills integration: OpenCode reads `AGENTS.md`. Injection pattern same as Codex.
- Model selection: OpenCode is provider-agnostic. Config via `opencode.json` in repo or `/connect` command. Orca writes an `opencode.json` at `.orca/opencode.json` referencing the configured provider.
- **Plan vs Build mode**: OpenCode has a built-in Plan/Build agent toggle. Orca uses this directly — map our `Planner` role to OpenCode's `plan` agent and our `Implementer` role to its `build` agent. Saves us a prompt-engineering step.
- Completion heuristic: same pattern; different regex.
- Usage parsing: OpenCode session exports include usage; capture via `/stats` command periodically or parse the statusline.

## Writing a new agent adapter

To add a sixth agent (e.g., Aider):

1. Create `crates/orca-agents/src/agents/aider.rs`.
2. Pick a strategy. If the agent is TTY-only, use `TmuxAdapter` with agent-specific config. If it has structured I/O, use `RpcAdapter`.
3. Register in `crates/orca-agents/src/lib.rs::builtin_adapters()`:
   ```rust
   pub fn builtin_adapters() -> Vec<Box<dyn AgentAdapter>> {
       vec![
           Box::new(claude_code::adapter()),
           Box::new(codex::adapter()),
           // ...
           Box::new(aider::adapter()),
       ]
   }
   ```
4. Add a config section to `config.toml` schema in docs.
5. Write integration tests with real-output fixtures.

For third-party adapters (not shipped with Orca core): a dynamic loading mechanism is a v0.3 feature. In v0, ship as a crate the user adds to their `Cargo.toml` via a trait object registered at startup. Details in later BUILD_PLAN milestones.

## Agent role mapping

Each of the four roles in `SpawnContext::role` has sensible defaults per agent:

| Agent | Planner prompt | Implementer prompt | Reviewer prompt | Utility prompt |
|---|---|---|---|---|
| claude-code | "Plan the work…" | "Implement the plan…" | "Review against spec…" | via /task |
| codex | "Propose approach…" | "Make the edits…" | "Find flaws…" | inline |
| gemini-cli | "Survey and plan…" | "Implement…" | "Review holistically…" | inline |
| pi | via RPC | via RPC | via RPC | primary |
| opencode | `plan` agent | `build` agent | custom review agent | `plan` agent |

Prompt templates live at `crates/orca-agents/src/prompts/{agent}/{role}.md`. Users can override in `.orca/prompts/{agent}/{role}.md` if they want custom preambles.

## Observability

Every adapter emits events to the daemon's event bus:
- `AgentSpawned { agent, task, pane_or_pid }`
- `AgentHeartbeat { agent, last_output_at, output_hash }` (every 2s)
- `AgentUsageUpdate { agent, task, usage }` (every usage line parsed)
- `AgentDied { agent, task, reason }`

These flow to `events.jsonl` and power the TUI's agent panel.
