# State

Everything Orca knows is on disk under `.orca/`. The daemon is a reactive layer that watches, validates, and mutates this directory; it owns no in-memory-only state.

## Directory layout

```
.orca/
├── config.toml                   # user config (committed to git)
├── secrets.toml                  # optional, gitignored
├── state/
│   ├── daemon.pid                # running daemon's PID
│   ├── daemon.sock               # Unix socket for CLI ↔ daemon
│   ├── events.jsonl              # append-only event log (rotated)
│   ├── agents/
│   │   ├── claude-code.json      # per-agent live status
│   │   ├── codex.json
│   │   ├── gemini-cli.json
│   │   ├── pi.json
│   │   └── opencode.json
│   └── tasks/
│       ├── T-001/
│       │   ├── task.toml         # spec + metadata
│       │   ├── log.jsonl         # per-task events
│       │   ├── plan.md           # planner output (if any)
│       │   ├── review.md         # reviewer output (if any)
│       │   └── worktree          # symlink to ../../worktrees/T-001
│       └── T-002/
│           └── ...
├── worktrees/
│   ├── T-001/                    # git worktree, branch orca/T-001
│   └── T-002/
├── kb/
│   ├── graph.json                # graphify output
│   ├── GRAPH_REPORT.md
│   └── cache/                    # graphify's own cache
├── cache/
│   ├── tokens.db                 # SQLite: usage history
│   └── thumbnails/               # if relevant
└── logs/
    ├── daemon.log                # daemon stderr
    └── agents/
        ├── claude-code.log
        └── ...
```

### What's git-tracked

Committed:
- `.orca/config.toml`
- `.orca/state/tasks/*/task.toml` (optional — user choice; lets you commit a task backlog)

Gitignored (example `.gitignore` entries):
```
.orca/secrets.toml
.orca/state/daemon.pid
.orca/state/daemon.sock
.orca/state/events.jsonl
.orca/state/agents/
.orca/worktrees/
.orca/kb/cache/
.orca/cache/
.orca/logs/
```

`orca init` sets this up automatically.

## config.toml

The single source of truth for user preferences. Editable by hand; the daemon picks up changes via `notify`.

```toml
# .orca/config.toml
[project]
name = "liat-ball-detection"
root = "."                        # relative to .orca/
default_branch = "main"

[orca]
# Daemon will listen on this Unix socket (relative to .orca/)
socket = "state/daemon.sock"
# Where to write worktrees (relative to .orca/)
worktrees_dir = "worktrees"
# Keep last N rotated events.jsonl files
event_log_retention = 10

[effort]
# Applied to all agents unless overridden
preset = "quality"                # expensive | quality | balanced | fast

# ─────────────────────────────────────────────────────
# AGENTS — one section per agent
# Only enabled agents are spawned
# ─────────────────────────────────────────────────────

[agents.claude-code]
enabled = true
command = "claude"                # binary on $PATH
# Per-role models (used when this agent plays the named role)
models = { planning = "claude-opus-4-7", implementing = "claude-sonnet-4-6", reviewing = "claude-sonnet-4-6" }
# Daily limits (daemon rejects spawn if exceeded)
daily_usage_limit_usd = 20.0
# Extra CLI args to pass on spawn
extra_args = []

[agents.codex]
enabled = true
command = "codex"
models = { planning = "o3", implementing = "o3-mini", reviewing = "o3" }
daily_usage_limit_usd = 10.0

[agents.gemini-cli]
enabled = true
command = "gemini"
models = { planning = "gemini-2.5-pro", implementing = "gemini-2.5-flash", reviewing = "gemini-2.5-pro" }
daily_usage_limit_usd = 10.0

[agents.pi]
enabled = true
command = "pi"
# Pi speaks JSONL on stdin/stdout when invoked with --rpc
extra_args = ["--rpc"]
models = { default = "kimi-k2.5:cloud" }
daily_usage_limit_usd = 5.0

[agents.opencode]
enabled = true
command = "opencode"
models = { default = "claude-sonnet-4-6" }
daily_usage_limit_usd = 10.0

# ─────────────────────────────────────────────────────
# ROUTING — dispatch rules
# ─────────────────────────────────────────────────────

[routing]
# Priority-ordered rules. First match wins.
# Each rule maps a set of required capabilities to an agent preference.
rules = [
  { needs = ["long_context"], prefer = "gemini-cli" },
  { needs = ["adversarial_review"], prefer = "codex" },
  { needs = ["rpc_driven"], prefer = "pi" },
  { needs = ["local_model"], prefer = "opencode" },
  { needs = ["multi_file_edit"], prefer = "claude-code" },
]
# Fallback if no rule matches
default = "claude-code"
# If true, auto-accept the top suggestion; if false, always prompt user
auto_accept = false

# ─────────────────────────────────────────────────────
# SKILLS — external skills installed system-wide
# ─────────────────────────────────────────────────────

[skills]
graphify = { enabled = true, mcp = true }     # use MCP if available
caveman = { enabled = false }                 # token compression
# Custom skills pointer
# custom = ["path/to/my-skill"]

# ─────────────────────────────────────────────────────
# TUI preferences
# ─────────────────────────────────────────────────────

[tui]
refresh_hz = 10
theme = "default"                 # default | solarized-dark | dracula
show_cost = true
show_tokens = true

# ─────────────────────────────────────────────────────
# Task defaults
# ─────────────────────────────────────────────────────

[tasks]
# Default auto-merge strategy on Done:
#   ff       = fast-forward merge into main
#   pr       = create PR via `gh`
#   keep     = leave branch, user merges manually
merge_strategy = "keep"
```

## secrets.toml (optional)

For users who don't want to rely on env vars for API keys:

```toml
[anthropic]
api_key = "sk-ant-..."

[openai]
api_key = "sk-..."

[google]
api_key = "..."
```

Gitignored. Loaded only if present; env vars take precedence.

## task.toml

One per task, at `.orca/state/tasks/T-NNN/task.toml`. Editable by hand.

```toml
id = "T-007"
title = "Fix referee false-positive in dark-kit frames"
description = """
The card classifier reports false positives on referees wearing dark kits
that are confusable with player kits. Root-cause the jersey-color shortcut
and propose mitigation. Likely involves revisiting the color-jitter
augmentation range.
"""
state = "implementing"
created_at = "2026-04-22T09:14:00Z"
updated_at = "2026-04-22T10:02:11Z"

# Who's working on it
assigned_to = "claude-code"
reviewer = "codex"

# Capabilities declared at creation — drive dispatch
capabilities = ["multi_file_edit", "domain_specific", "needs_review"]

# Optional parent/subtask relationships
parent = null
subtasks = []

# Context the agent should read first
context_files = [
  "src/card_classifier/model.py",
  "src/card_classifier/train.py",
  "configs/train_card.yaml",
]

# Worktree this task lives in (symlink target)
worktree = "../../../worktrees/T-007"
branch = "orca/T-007"

# Acceptance criteria (optional — only enforced if spec_mode = true)
acceptance = [
  "False-positive rate on held-out ref clips reduced to < 0.5%",
  "No regression on existing test set (accuracy within 0.5pp)",
  "Training config change documented in CHANGELOG",
]

# Human notes appended over time
notes = """
— 09:14 LB: Opened task, attached the failing clips
— 10:02 LB: Claude's plan at plan.md looks good, proceeding
"""
```

### Task capabilities (vocabulary)

A small controlled vocabulary the dispatcher uses. Users can add custom ones; built-ins:

| Capability | Meaning |
|---|---|
| `long_context` | Needs to load >50k lines of code or many files — Gemini territory |
| `multi_file_edit` | Touches ≥3 files — Claude Code territory |
| `surgical_edit` | Touches 1–2 files with clear scope — Codex territory |
| `adversarial_review` | Explicit review pass — prefer different agent than builder |
| `rpc_driven` | Scripted/automated utility work — Pi territory |
| `local_model` | Must run via local Ollama etc. — OpenCode territory |
| `domain_specific` | Needs KB context — triggers graphify preamble |
| `needs_review` | After build, auto-queue a review step |

## Task state machine

```
         ┌──────────┐
         │ drafted  │  (just created)
         └────┬─────┘
              │ user assigns
              ▼
         ┌──────────┐
         │ planning │  (agent is generating plan.md)
         └────┬─────┘
              │ plan ready
              ▼
         ┌──────────┐
         │ planned  │  (awaiting user ACK)
         └────┬─────┘
              │ user approves
              ▼
         ┌──────────────┐
         │ implementing │  (agent is coding)
         └────┬─────────┘
              │ implementation complete
              ▼
         ┌──────────────┐
         │ implemented  │  (awaiting review or user decision)
         └────┬─────────┘
              │ user requests review
              ▼
         ┌──────────┐
         │ reviewing│  (reviewer agent is checking)
         └────┬─────┘
              │ review done
              ▼
         ┌──────────┐
         │ reviewed │  (findings may require revision)
         └────┬─────┘
              │ user accepts   user requests fixes
              ▼                      ▼
         ┌──────────┐        ┌──────────┐
         │   done   │        │ revising │──► implementing (loop)
         └──────────┘        └──────────┘

         ┌──────────┐
         │ blocked  │  — entered on agent death, limit exceeded, user block
         └──────────┘  — user unblocks to resume

         ┌──────────┐
         │ parked   │  — user-initiated pause
         └──────────┘  — user unparks to resume
```

### Transition rules

| From → To | Trigger | Who initiates |
|---|---|---|
| drafted → planning | agent assigned, spawn successful | daemon |
| planning → planned | agent wrote plan.md and went idle | adapter |
| planning → blocked | agent died or errored | daemon |
| planned → implementing | user ACKs plan | user |
| planned → drafted | user rejects plan, re-route | user |
| implementing → implemented | agent committed to worktree and idle | adapter |
| implementing → blocked | agent died / timeout / limit | daemon |
| implemented → reviewing | user requests review | user |
| implemented → done | user accepts without review | user |
| reviewing → reviewed | reviewer wrote review.md | adapter |
| reviewed → revising | user requests fixes | user |
| reviewed → done | user accepts | user |
| revising → implementing | revision agent picks up | daemon |
| blocked → drafted | user reroutes | user |
| blocked → parked | user parks | user |
| parked → drafted | user unparks | user |
| any → parked | user manual park | user |

### Invariants

- A task is always in exactly one state.
- At most one agent is `assigned_to` a task at a time. Separate `reviewer` tracks the review-phase agent.
- `assigned_to` is non-null in `planning/implementing/revising`, null elsewhere.
- `reviewer` is non-null in `reviewing/reviewed`, null elsewhere.
- Worktree exists iff state ∉ {drafted, done, parked-with-cleanup}.
- State transitions are atomic at the task.toml level (write+rename).
- `log.jsonl` is append-only; never rewritten.

## events.jsonl

Append-only global event log. One JSON object per line:

```jsonl
{"ts":"2026-04-22T09:14:00Z","type":"task.created","task":"T-007","by":"user"}
{"ts":"2026-04-22T09:14:01Z","type":"suggestion.ready","task":"T-007","top":"claude-code","alternatives":["codex"]}
{"ts":"2026-04-22T09:14:04Z","type":"user.decision","task":"T-007","decision":"accept","agent":"claude-code"}
{"ts":"2026-04-22T09:14:05Z","type":"task.state","task":"T-007","from":"drafted","to":"planning"}
{"ts":"2026-04-22T09:14:06Z","type":"agent.spawned","agent":"claude-code","pane":"orca:1.0","pid":47213}
{"ts":"2026-04-22T09:17:22Z","type":"agent.usage","agent":"claude-code","task":"T-007","tokens_in":8421,"tokens_out":1203,"cost_usd":0.1814}
{"ts":"2026-04-22T09:22:11Z","type":"task.state","task":"T-007","from":"planning","to":"planned"}
...
```

Rotated daily or at 10MB, whichever first. Keeps the last N (configurable, default 10).

## agent status files

Per-agent status, written by the daemon, readable by anything:

```json
// .orca/state/agents/claude-code.json
{
  "id": "claude-code",
  "enabled": true,
  "status": "busy",             // idle | busy | dead | disabled
  "active_task": "T-007",
  "pane": "orca:1.0",
  "pid": 47213,
  "spawned_at": "2026-04-22T09:14:06Z",
  "last_heartbeat": "2026-04-22T09:45:02Z",
  "usage_today": { "tokens_in": 142031, "tokens_out": 18042, "cost_usd": 1.874 },
  "limit_remaining_usd": 18.126
}
```

## Schema evolution

`.orca/config.toml` begins with a `schema_version` key (absent = v1). On load, the daemon migrates old configs forward and writes back. We commit to backward-compat for at least the previous minor version.

Task files: same pattern. `task.toml` includes a `schema_version`; migrations are mechanical.

## Concurrency & safety

- `task.toml` writes use a temp-file + rename pattern (atomic on POSIX).
- `events.jsonl` uses append-only `O_APPEND` opens — no lock needed for single-daemon.
- Only one daemon per `.orca/`. `daemon.pid` is a flock'd file; `orca daemon` refuses to start if another instance holds the lock.
- CLI clients fail gracefully if daemon isn't running (with "run `orca` to start the TUI, or `orca daemon &` to run headless").
