# Build Plan

This document decomposes the MVP into implementable tickets. It's written for Claude Code and Codex (the agents who will actually build Orca). Each ticket has:

- **ID** — `M0-T01`, `M1-T04`, etc. — Milestone + Ticket.
- **Depends on** — list of ticket IDs that must be done first.
- **Scope** — what code to write, in what crate.
- **Acceptance** — concrete test/behavior that validates completion.
- **Out of scope** — what *not* to do in this ticket (deferred to later tickets).

Tickets are ordered so that at each milestone boundary, you have a runnable-and-useful Orca. No big-bang integration at the end.

## Overall structure

7 milestones, roughly 1–2 weeks of work each for one engineer + agents:

- **M0** Scaffolding — workspace, crates, CLI skeleton, daemon skeleton, logging.
- **M1** Config & state — `config.toml` wizard and parser, `.orca/` state writes/reads, task CRUD.
- **M2** Tmux & adapters — TmuxAdapter with claude-code as the first integration.
- **M3** Dispatcher & lifecycle — full state machine, dispatcher rules, event bus.
- **M4** TUI — ratatui control pane, live updates, overlays.
- **M5** Remaining agents — codex, gemini-cli, pi (RpcAdapter), opencode.
- **M6** KB — graphify integration, MCP management.
- **M7** Polish — `orca doctor`, error paths, docs, MVP release.

## M0 — Scaffolding

**Goal**: one binary that compiles, runs, has a working CLI skeleton and daemon skeleton, speaks a minimal protocol over a Unix socket.

### M0-T01 — Cargo workspace
- **Depends on**: nothing
- **Scope**: Create the workspace `Cargo.toml` with the crate layout from `ARCHITECTURE.md § Project layout`. Each crate has a stub `lib.rs` (or `main.rs` for the binary crate) that just exports nothing / prints hello.
- **Acceptance**: `cargo build --workspace` passes. `cargo run --bin orca -- version` prints a version string.
- **Out of scope**: any real functionality; dependencies beyond `clap` and `tokio`.

### M0-T02 — CLI skeleton with clap
- **Depends on**: M0-T01
- **Scope**: In `orca-cli`, define the top-level `Cli` struct with derive clap. Add subcommand enums matching `CLI.md`: `Init`, `Config`, `Daemon`, `Task(TaskCmd)`, `Agent(AgentCmd)`, `Kb(KbCmd)`, `Status`, `Watch`, `Version`, `Doctor`. All handlers are `unimplemented!()` except `Version`.
- **Acceptance**: `orca --help` shows all subcommands and their options. `orca version` works. Running any other subcommand exits 1 cleanly with "not yet implemented".
- **Out of scope**: subcommand implementations.

### M0-T03 — Daemon process skeleton
- **Depends on**: M0-T02
- **Scope**: In `orca-daemon`, implement `run_daemon()` that:
  1. Creates `.orca/state/` if missing
  2. Acquires a flock on `.orca/state/daemon.pid`
  3. Writes PID
  4. Binds a Unix socket at `.orca/state/daemon.sock`
  5. Accepts connections, reads newline-delimited JSON, responds with `{"type":"pong"}` to a `{"type":"ping"}` message
  6. Handles SIGINT/SIGTERM cleanly (removes pid + socket)

  Hook this up from `orca daemon` and `orca daemon --foreground`. Backgrounding uses `daemonize` crate or manual `fork+setsid`.
- **Acceptance**: `orca daemon -f` runs in foreground; in another terminal, `nc -U .orca/state/daemon.sock` and sending `{"type":"ping"}` gets `{"type":"pong"}` back. `Ctrl-C` cleans up.
- **Out of scope**: real RPC protocol; any actual state.

### M0-T04 — RPC client helper
- **Depends on**: M0-T03
- **Scope**: In `orca-cli`, add a helper module `daemon_client` that:
  - Connects to the socket (with retry if daemon is starting)
  - Sends a JSON request, reads one JSON response
  - Auto-launches the daemon if not running (via `orca daemon`)
- **Acceptance**: A new `orca ping` subcommand (temporary, remove in M1) that calls the helper, prints the response.
- **Out of scope**: typed request/response schemas.

### M0-T05 — Logging
- **Depends on**: M0-T01
- **Scope**: Add `tracing` + `tracing-subscriber` to the binary. Log level from `RUST_LOG` env and `-v`/`-q` flags. Logs to stderr by default; daemon logs to `.orca/logs/daemon.log` via file appender.
- **Acceptance**: `RUST_LOG=debug orca daemon -f` produces structured debug logs. `.orca/logs/daemon.log` exists when daemonized.

## M1 — Config & state

**Goal**: `orca init` works, writes a valid config, daemon reads state on startup, CRUD on tasks via CLI.

### M1-T01 — config schema types
- **Depends on**: M0
- **Scope**: In `orca-core`, define Rust structs mirroring the `config.toml` schema in `STATE.md`. Use `serde` derive. Provide a `Config::defaults()` function.
- **Acceptance**: Unit tests: round-trip serialize/deserialize a valid config. Unit test with the `STATE.md` example config. Defaults produce a parseable TOML.

### M1-T02 — config loader
- **Depends on**: M1-T01
- **Scope**: `Config::load_from(path: &Path)` merges file with defaults. Env var expansion (`${VAR}` in string values). Validation: reject unknown agent IDs, negative USD limits, etc.
- **Acceptance**: Tests for malformed configs produce clear error messages pointing to the bad field.

### M1-T03 — inquire wizard for `orca init`
- **Depends on**: M1-T02
- **Scope**: In `orca-wizard`, implement the 8-step flow from `UI.md`. Uses `inquire`. Writes the resulting TOML atomically (temp + rename). Detects existing config and offers to overwrite/merge.
- **Acceptance**: `orca init` in an empty dir produces a valid `.orca/config.toml`. Re-running with `--force` overwrites. Without `--force`, prompts.

### M1-T04 — task types & on-disk format
- **Depends on**: M0
- **Scope**: Define `Task`, `TaskState`, `TaskId` in `orca-core`. Implement `Task::from_file` and `Task::write_atomic`. Match the `task.toml` schema in `STATE.md`.
- **Acceptance**: Round-trip tests. A task written by the code can be read back identically. Hand-editing a task.toml and re-reading works.

### M1-T05 — state store in daemon
- **Depends on**: M1-T04, M0-T03
- **Scope**: The daemon holds `StateStore`, a struct that owns `HashMap<TaskId, Task>`, plus agent status files. On startup, scans `.orca/state/tasks/` and loads every task. Provides methods: `create_task`, `update_task`, `get_task`, `list_tasks`. Every mutation writes through to disk.
- **Acceptance**: Start daemon, create a task via direct store method call, restart daemon, task is still there.

### M1-T06 — filesystem watcher
- **Depends on**: M1-T05
- **Scope**: Use `notify` to watch `.orca/state/tasks/` and `.orca/config.toml`. On changes from outside, reconcile with in-memory state. Log a warning on conflicts (shouldn't happen if nobody else is writing).
- **Acceptance**: Manual test: `echo` a new task.toml into `.orca/state/tasks/T-999/`, daemon logs "task T-999 ingested".

### M1-T07 — RPC protocol types
- **Depends on**: M1-T04, M0-T04
- **Scope**: Define request/response enums. Requests: `Ping`, `CreateTask`, `GetTask`, `ListTasks`, `UpdateTaskState`. Responses: matched types. Use `serde` tagged enum format. Document the protocol in a new file `docs/RPC.md` (stub for now).
- **Acceptance**: Types compile, serialize cleanly as `{"type": "...", "data": {...}}`.

### M1-T08 — `orca task new/list/show`
- **Depends on**: M1-T05, M1-T07
- **Scope**: Implement the three CLI subcommands. `new` builds a `Task` from flags and sends `CreateTask` RPC. `list` sends `ListTasks`. `show` sends `GetTask` and formats output.
- **Acceptance**: End-to-end test via `assert_cmd`: run `orca task new "test" --capabilities surgical_edit`, then `orca task list` shows it, then `orca task show T-001` shows the details. `--json` mode works.

### M1-T09 — events.jsonl
- **Depends on**: M1-T05
- **Scope**: In `orca-core`, implement an `EventBus`. Publish/subscribe over an in-process `tokio::sync::broadcast`. Every published event is also appended to `.orca/state/events.jsonl`. Implement rotation: when file exceeds `event_log_retention_bytes`, rotate to `events-<ts>.jsonl`, keep last N.
- **Acceptance**: Tests: publish N events, verify they appear in the file. Rotation test with a small threshold.

### M1-T10 — `orca watch`
- **Depends on**: M1-T09
- **Scope**: Subscribe to the daemon's event stream via the socket (new `Subscribe` RPC). Pretty-print events. `--filter TYPE` and `--json` supported.
- **Acceptance**: Run `orca watch` in one terminal; `orca task new "..."` in another; the create event appears.

**M1 milestone demo**: user runs `orca init`, gets a config, runs `orca task new "hello"`, sees it in `orca task list`, sees the event in `orca watch`. No agent execution yet — the system just manages tasks as data.

## M2 — Tmux & first adapter

**Goal**: `claude-code` can be spawned on a task, the user can interact with it, Orca detects when it's idle.

### M2-T01 — tmux wrapper
- **Depends on**: M0
- **Scope**: In `orca-agents`, module `tmux`. Thin Rust wrapper over `tmux` CLI: `ensure_session(name)`, `new_window`, `send_keys`, `capture_pane`, `kill_pane`, `list_panes`. Uses `tokio::process::Command` for each.
- **Acceptance**: Integration test (skipped if no tmux): create a session, a window, send-keys an `echo hello`, capture the pane, see "hello".

### M2-T02 — git worktree wrapper
- **Depends on**: M0
- **Scope**: In `orca-core`, module `worktree`. Create/remove/list. Uses `git2` or shells out. Opinionated: always creates `orca/<task-id>` branches from `default_branch`.
- **Acceptance**: Tests in a temp git repo: create worktree, branch exists, remove worktree, branch gone (if configured).

### M2-T03 — `AgentAdapter` trait
- **Depends on**: M2-T01, M2-T02
- **Scope**: Define the trait per `AGENT_ADAPTERS.md`. Define `SpawnContext`, `AgentHandle`, `AgentStatus`. Put all in `orca-agents::adapter`.
- **Acceptance**: Compiles. Trait is object-safe.

### M2-T04 — TmuxAdapter
- **Depends on**: M2-T03
- **Scope**: Generic TmuxAdapter struct parameterized on agent-specific config (command, ready_regex, usage_parser). Implements `AgentAdapter`. Handles spawn, send_prompt, status (capture + hash heuristic), kill.
- **Acceptance**: With a fake "agent" (shell prompt running `bash`), can spawn, send a command, detect idle, capture output, kill.

### M2-T05 — claude-code integration
- **Depends on**: M2-T04
- **Scope**: In `orca-agents::agents::claude_code`, define the per-agent config (ready regex, usage parser regex, capabilities) and a factory function `claude_code::adapter() -> TmuxAdapter`. Register in `builtin_adapters()`.
- **Acceptance**: Integration test (marked `#[ignore]` by default, requires claude installed): spawn a real claude-code instance, send a trivial prompt, verify output appears, kill cleanly.

### M2-T06 — spawn a task manually
- **Depends on**: M2-T05, M1
- **Scope**: New RPC: `SpawnAgentForTask { task, agent }`. Daemon:
  1. Creates worktree
  2. Calls adapter.spawn
  3. Sends initial prompt (inline template for now, proper templates in M3)
  4. Stores handle, updates task.assigned_to, transitions state
- **Acceptance**: Manually: `orca task new "trivial test"`; then `orca task route T-001 --to claude-code`; a new tmux window opens with claude-code running in the task's worktree. User can see the prompt in the pane.

## M3 — Dispatcher & lifecycle

**Goal**: full state machine works end-to-end, dispatcher suggests agents, completion heuristics transition states.

### M3-T01 — full state machine in daemon
- **Depends on**: M1, M2
- **Scope**: `StateMachine` module in `orca-daemon` that validates every transition per `TASK_LIFECYCLE.md § Transition rules`. Reject invalid transitions with error.
- **Acceptance**: Property tests: random sequences of transitions either succeed (landing in valid states) or fail cleanly.

### M3-T02 — dispatch rules
- **Depends on**: M3-T01
- **Scope**: `Dispatcher` struct. Loads rules from config. `suggest(task, available_agents) -> Suggestion`. Emits `SuggestionReady` events. Respects `auto_accept`.
- **Acceptance**: Tests with canned capabilities: task needing `long_context` gets `gemini-cli`; task needing `adversarial_review` with `claude-code` as builder gets `codex`; fallback yields `claude-code`.

### M3-T03 — plan.md template + planning completion
- **Depends on**: M2-T06, M3-T01
- **Scope**: Prompt template in `orca-agents::prompts::claude_code::planner`. Completion heuristic: idle + `plan.md` exists. On completion, transition `planning → planned`.
- **Acceptance**: Run end-to-end with claude-code (still `#[ignore]`): create task, route, plan.md appears, state transitions.

### M3-T04 — implementing completion
- **Depends on**: M3-T03
- **Scope**: User can transition `planned → implementing` via `orca task accept-plan T-NNN` (new subcommand) or TUI keybind. Agent prompted to implement. Completion heuristic: idle + commits exist on branch. Transition to `implemented`.
- **Acceptance**: End-to-end test with a canned prompt that makes a small edit.

### M3-T05 — review cycle
- **Depends on**: M3-T04
- **Scope**: `orca task review T-NNN --by <agent>` spawns reviewer with review prompt template, writes `review.md`, transitions `reviewing → reviewed`.
- **Acceptance**: Mock two agents (real claude-code for build, real codex for review) and confirm both produce artifacts; review.md has reasonable content.

### M3-T06 — accept / revise / done
- **Depends on**: M3-T05
- **Scope**: `orca task accept` transitions reviewed→done. `orca task revise` transitions reviewed→revising→implementing. `done` triggers merge strategy.
- **Acceptance**: Fast-forward merge works. `keep` leaves worktree. (PR mode deferred to M7.)

### M3-T07 — failure handling
- **Depends on**: M3-T01, M2
- **Scope**: Agent process-death detection. Transition to `blocked(AgentDied)`. Emit event. TUI (when it exists in M4) will offer recovery options; for now, CLI: `orca task recover T-NNN --reroute <agent>`.
- **Acceptance**: Kill the tmux pane of a running agent externally; daemon detects within 10s, task state becomes `blocked`.

### M3-T08 — usage metering
- **Depends on**: M2-T04
- **Scope**: SQLite DB at `.orca/cache/tokens.db`. Adapter calls `EventBus::publish(UsageUpdate { agent, task, usage })` for every parsed line; daemon inserts a row. Daily summary view.
- **Acceptance**: After a task run, `orca agent list` shows non-zero tokens and cost for the agents that ran.

### M3-T09 — limit enforcement
- **Depends on**: M3-T08
- **Scope**: On `SpawnAgentForTask`, check today's cost against `daily_usage_limit_usd`. Reject with `LimitExceeded` if over. When an `UsageUpdate` pushes an agent over, kill it and transition tasks to blocked.
- **Acceptance**: Set limit to a tiny value; second task fails to spawn; running task gets blocked mid-execution.

**M3 milestone demo**: user creates a task, dispatcher suggests an agent, user accepts, claude-code runs, writes plan.md, user accepts plan, claude-code implements, user requests review by codex, codex reviews, user accepts, task merges, everything logged. All via CLI, no TUI yet.

## M4 — TUI

**Goal**: the ratatui control pane is live and preferable to the CLI for day-to-day use.

### M4-T01 — TUI skeleton
- **Depends on**: M1, M3
- **Scope**: In `orca-tui`, `App` struct, `event.rs` with merged event stream (key events + daemon events), render loop. Four empty panels placed per `UI.md` layout.
- **Acceptance**: `orca` launches, shows the layout with placeholders, `q` quits, resize handled.

### M4-T02 — daemon subscription
- **Depends on**: M4-T01, M1-T09
- **Scope**: TUI connects to daemon socket, sends `Subscribe`, reads events into its stream. Reconnect on daemon restart.
- **Acceptance**: Events from a separate `orca task new` appear in the TUI's event panel in real time.

### M4-T03 — tasks panel
- **Depends on**: M4-T02
- **Scope**: Render tasks as a tree. Keyboard navigation (j/k, h/l for collapse/expand). Selected task highlighted.
- **Acceptance**: Tasks update when state changes, selection persists across redraws, subtasks render nested.

### M4-T04 — agents panel
- **Depends on**: M4-T02, M3-T08
- **Scope**: Row per agent with live cost/tokens/status. Updates on `UsageUpdate` and status-change events.
- **Acceptance**: Cost counter ticks up during a real agent run.

### M4-T05 — task detail view
- **Depends on**: M4-T03
- **Scope**: Enter on a task drills into the detail page per `UI.md`. Shows description, plan, live diff. `Esc` returns.
- **Acceptance**: Diff renders correctly; navigating between tasks in detail mode works.

### M4-T06 — overlays (route, kb, help)
- **Depends on**: M4-T03
- **Scope**: Modal overlays for routing, KB query (stub until M6), help sheet.
- **Acceptance**: `r` on a task shows route overlay; choosing an agent sends the RPC; task transitions reflect in the main view.

### M4-T07 — command mode
- **Depends on**: M4-T03
- **Scope**: `:` opens a command line that mirrors CLI syntax. Tab-completion. Enter executes.
- **Acceptance**: `:task new foo` creates a task; `:route T-001 --to codex` routes.

### M4-T08 — inline config wizard
- **Depends on**: M4-T01, M1-T03
- **Scope**: `c` keybind suspends the TUI, runs the inquire wizard, resumes TUI with updated config.
- **Acceptance**: Enable a new agent via `c`; it appears in the agents panel.

**M4 milestone demo**: most of the CLI now has keyboard equivalents in the TUI. Regular use flows through the TUI.

## M5 — Remaining agents

**Goal**: all five agents work.

### M5-T01 — codex
- **Depends on**: M3
- **Scope**: `agents::codex` module. Per-agent config (ready regex `❯`, usage parser). Reasoning effort mapping. Register in builtins.
- **Acceptance**: End-to-end a task with codex as builder.

### M5-T02 — gemini-cli
- **Depends on**: M3
- **Scope**: `agents::gemini_cli`. Regex for ready prompt. Long-context helper: `attach_for_gemini` that reads task.context_files and preprocesses.
- **Acceptance**: Route a task with `long_context` capability; Gemini gets the relevant files in context; completes.

### M5-T03 — RpcAdapter
- **Depends on**: M2-T03
- **Scope**: New adapter strategy in `orca-agents::adapter`. Spawns with stdin/stdout pipes. Reads a JSONL event stream. Explicit `turn_end` = completion.
- **Acceptance**: With a mock Pi (a Python script that speaks the expected JSONL), full lifecycle works.

### M5-T04 — pi integration
- **Depends on**: M5-T03
- **Scope**: `agents::pi` module using RpcAdapter. Pass `--rpc` arg. Session-per-task via `--session` flag. Also create a read-only tmux pane for human observation.
- **Acceptance**: End-to-end with real pi. Usage tracked correctly from `turn_end.usage`.

### M5-T05 — opencode
- **Depends on**: M3
- **Scope**: `agents::opencode`. Plan/Build mode mapping: Orca's `Planner` role uses OpenCode's `plan` agent, `Implementer` uses `build`. Config via `.orca/opencode.json`.
- **Acceptance**: End-to-end plan+build with opencode.

### M5-T06 — per-agent prompt templates
- **Depends on**: M5-T01 through M5-T05
- **Scope**: Fill out `orca-agents::prompts::{agent}::{role}.md` for all 5 agents × 4 roles. Allow user override at `.orca/prompts/`.
- **Acceptance**: Spot-checks: each prompt produces reasonable output on a canonical test task.

## M6 — KB

**Goal**: graphify is integrated end-to-end.

### M6-T01 — graphify detection & install
- **Depends on**: M1
- **Scope**: `orca-kb` module. `graphify_installed() -> Option<Version>`. During `orca init`, if skills.graphify enabled, prompt for install.
- **Acceptance**: `orca doctor` reports graphify status correctly.

### M6-T02 — `orca kb init` / `update` / `query` / `path` / `explain`
- **Depends on**: M6-T01
- **Scope**: All CLI subcommands shell out to graphify with correct args, output dir `.orca/kb/`.
- **Acceptance**: End-to-end on the Orca repo itself: `orca kb init` produces `.orca/kb/GRAPH_REPORT.md`; `orca kb query "..."` returns useful output.

### M6-T03 — per-agent skill install
- **Depends on**: M6-T01
- **Scope**: On `orca init` or `orca kb init`, run `graphify <platform> install` for each enabled agent.
- **Acceptance**: After `orca init` with graphify enabled, `CLAUDE.md` contains graphify reference, `AGENTS.md` contains the skill instruction, etc.

### M6-T04 — MCP server lifecycle
- **Depends on**: M6-T02
- **Scope**: `orca kb mcp start/stop`. Daemon supervises the Python subprocess; restart on crash. Write/update `.mcp.json` for MCP-capable agents.
- **Acceptance**: Server starts, Claude Code sees the tool, queries return graph results.

### M6-T05 — task-level KB preamble
- **Depends on**: M6-T02, M3
- **Scope**: When spawning an agent for a task with `domain_specific` capability or if `[kb] inject_preamble = true`, prepend the KB preamble from `KB.md § 4`.
- **Acceptance**: Preamble appears in the agent's initial prompt. Disable via task flag.

## M7 — Polish

**Goal**: Ship a thing people want to use.

### M7-T01 — `orca doctor`
- **Depends on**: M6
- **Scope**: Checks per `CLI.md § orca doctor`. Non-zero exit on errors.
- **Acceptance**: Breakage scenarios (missing binary, stale lock) are all caught.

### M7-T02 — PR merge strategy
- **Depends on**: M3-T06
- **Scope**: `gh pr create` wrapper for the `pr` merge strategy. Auto-generate body from plan + review.
- **Acceptance**: Task Done with `merge_strategy = pr` creates a PR visible on GitHub.

### M7-T03 — `orca task break` (subtask decomposition via pi)
- **Depends on**: M5-T04
- **Scope**: On a blocked or complex task, `orca task break T-NNN` spawns pi with a decomposition prompt, parses its JSON response, creates subtasks with user approval.
- **Acceptance**: Reasonable decompositions emerge; subtasks link back to parent.

### M7-T04 — shell completions
- **Depends on**: CLI complete
- **Scope**: `orca completion <shell>` outputs completion script (clap-generated).

### M7-T05 — installer & distribution
- **Depends on**: M7-T04
- **Scope**: Cargo-publish. A `install.sh` for curl-pipe-bash. Homebrew formula. Release GitHub Actions that builds static Linux x86_64 and aarch64 binaries plus macOS.
- **Acceptance**: Install from a fresh machine works via each method.

### M7-T06 — documentation pass
- **Depends on**: M7-T05
- **Scope**: Update all docs to reflect final behavior. Record a demo GIF/video. Write a short launch blog post.

### M7-T07 — bug bash
- **Depends on**: everything
- **Scope**: Run Orca on 3 real repos for a week. Fix every bug surfaced.
- **Acceptance**: Nothing in the GitHub issues tagged `mvp-blocker`.

## Dependency graph summary

```
M0 → M1 → M2 → M3 → M4 → M6 → M7
                ↘     ↘
                 M5 ──┘
```

M5 is largely parallelizable with M4 once M3 is done. M6 depends on having the agent adapters done for at least per-agent skill install. M7 is the sequential tail.

## Suggested task assignments (if Claude Code and Codex are building this)

A practical pattern:

- **Claude Code**: leads M0, M1, M4 (longer multi-file work, UI iteration)
- **Codex**: leads M2, M3, M5 (more surgical, per-agent adapter work is well-scoped)
- **Gemini CLI**: reviews each milestone PR (architectural review benefits from long context)
- **Pi**: runs weekly dependency-graph sanity checks and handles housekeeping PRs (formatting, doc updates)

Orca itself dogfooding its own roles — fitting.

## Open design questions

These are intentionally left open for the implementing agents to resolve, with guidance:

1. **Async runtime scope**: can `orca-core` stay `no_std`-ish / async-free, pushing all I/O to the daemon and TUI crates? Probably yes; do it if possible.

2. **git2 vs shell-out**: `git2` is cleaner but pulls in libgit2. Shelling out to `git` is simpler and matches what users expect. Recommendation: shell out for v0, reserve `git2` for specific hot paths (diff rendering in TUI).

3. **tmux_interface crate vs shell-out**: same question, same answer. Shell out.

4. **Protocol versioning**: the daemon RPC protocol needs a version. Recommendation: include `{"version": 1, ...}` in every request, daemon rejects unknown versions cleanly.

5. **Event schema evolution**: event types will grow. Recommendation: unknown types are logged as warnings but don't crash the TUI.

Flag these in the first few PRs so decisions are documented.
