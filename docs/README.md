# Orca

> A tmux-first, agent-agnostic orchestrator for coordinating Claude Code, Codex, Gemini CLI, Pi, and OpenCode on a single codebase.

Orca is a Rust CLI + TUI that sits above your AI coding agents. It doesn't replace them — it routes work between them. You still get Claude Code's planning, Codex's surgical edits, Gemini's long-context sweeps, Pi's scriptable RPC, and OpenCode's provider flexibility. What's new is that they're pointed at the *same task graph*, share the *same knowledge base*, and hand off work through an inspectable *filesystem state bus* instead of getting lost in separate terminal windows.

## Why this exists

If you've tried using three AI coding agents on the same project, you know the pain: duplicate work, conflicting edits, no shared memory, constant copy-paste between panes. The existing tools pick one side of that problem — Claude Squad gives you session management, cavekit gives you spec-driven builds with Claude + Codex review, Agent Teams keeps you in the Anthropic universe. None of them are symmetric across vendors with configurable role-routing, terminal-native, and KB-aware at the same time.

Orca is.

## Core idea in one diagram

```
                           ┌─ orca control (ratatui) ─┐
                           │  tasks · agents · events  │
                           └─────────────┬─────────────┘
                                         │
                                   ┌─────┴─────┐
                                   │  orcad    │   ← daemon: state machine,
                                   │  daemon   │      dispatch, lifecycle
                                   └─────┬─────┘
                                         │
              ┌──────────────┬───────────┼───────────┬──────────────┐
              │              │           │           │              │
        ┌─────▼────┐  ┌──────▼────┐ ┌────▼────┐ ┌───▼────┐  ┌──────▼────┐
        │ claude-  │  │  codex    │ │ gemini- │ │  pi    │  │ opencode  │
        │ code     │  │           │ │  cli    │ │ (RPC)  │  │           │
        └─────┬────┘  └──────┬────┘ └────┬────┘ └───┬────┘  └──────┬────┘
              │              │           │          │              │
              └──────────────┴───────────┴──────────┴──────────────┘
                                     │
                                     ▼
                              ┌──────────────┐
                              │   .orca/     │   ← filesystem state bus:
                              │   config,    │      tasks, events, KB,
                              │   state,     │      git worktrees
                              │   kb/        │
                              └──────────────┘
```

Every agent reads from and writes to the same `.orca/` directory. Every state transition is a file change. Every handoff is inspectable. You can `tail -f .orca/state/events.jsonl` and watch the work flow.

## What it does in practice

1. You write a task in the TUI (or drop a markdown file in `.orca/state/tasks/T-007/task.toml`).
2. Orca's dispatcher suggests which agent should handle it based on capabilities — "this needs long context, suggest Gemini" — and you confirm or override.
3. The agent spawns in a tmux pane with an isolated git worktree, reads the task file, does the work.
4. When it's done, the task transitions state and (optionally) gets routed to a second agent for review — cavekit-style.
5. If the agent dies, the daemon surfaces options: re-route to another agent, keep waiting, or park the task.
6. When everyone's happy, merge the worktree into main and close the task.

## Agent roster (v0)

| Agent | Strength | Communication |
|---|---|---|
| **claude-code** | Multi-file edits, planning, long reasoning | tmux pane |
| **codex** | Fast surgical changes, adversarial review | tmux pane |
| **gemini-cli** | Repo-wide review, architecture, 1M+ context | tmux pane |
| **pi** | Scriptable coordination, utility work | stdin/stdout JSONL (RPC) |
| **opencode** | Provider-flexible, LSP-aware, Plan/Build modes | tmux pane |

Adding a sixth agent = writing a new `AgentAdapter` impl. See `docs/AGENT_ADAPTERS.md`.

## Non-goals

- **Not a coding agent itself.** Orca doesn't edit code. It tells other agents to edit code.
- **Not a replacement for the agents.** If you want Claude Code, install Claude Code. Orca orchestrates what's already there.
- **Not cloud/hosted.** Pure local. Your keys, your machine, your code.
- **Not a web app.** Terminal-first. A web dashboard might come later but is not on the MVP roadmap.
- **Not an MCP server** (though Orca consumes MCPs, e.g., graphify).

## Quickstart (target state — not yet built)

```bash
# install
cargo install orca-cli     # or: curl -fsSL get.orca.dev | sh

# one-time setup in your repo
cd my-project
orca init                  # interactive wizard: pick agents, models, effort

# start the TUI (also starts the daemon if not running)
orca

# or drive from the CLI
orca task new "fix the card-classifier false positive on referees in dark kits"
orca route T-001 --to claude-code
orca review T-001 --by codex
orca status
```

## Documentation

- **[docs/VISION.md](docs/VISION.md)** — Why, who for, what's the niche, what's explicitly out of scope.
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — System architecture, process model, data flow.
- **[docs/STATE.md](docs/STATE.md)** — Filesystem layout, config schema, state machine.
- **[docs/AGENT_ADAPTERS.md](docs/AGENT_ADAPTERS.md)** — Agent adapter trait, per-agent integration notes, adding new agents.
- **[docs/TASK_LIFECYCLE.md](docs/TASK_LIFECYCLE.md)** — Task states, transitions, failure handling, breakdown into subtasks.
- **[docs/UI.md](docs/UI.md)** — ratatui control pane + inquire wizard specs.
- **[docs/CLI.md](docs/CLI.md)** — Full command reference.
- **[docs/KB.md](docs/KB.md)** — graphify integration via skills + MCP.
- **[docs/BUILD_PLAN.md](docs/BUILD_PLAN.md)** — MVP decomposed into implementable tickets with dependencies.
- **[AGENTS.md](AGENTS.md)** — Instructions for AI coding agents building Orca itself.

## License

MIT. Orca orchestrates other tools; its orchestration code should be as unencumbering as possible.
