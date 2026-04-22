# UI

Two surfaces, one state backend.

## Surface 1: Setup wizard (`orca init` / `orca config`)

Built with `inquire`. Step-by-step prompts. Runs on:
- `orca init` — first-time setup in a repo
- `orca config` — modify existing config interactively
- From within the TUI: pressing `c` pops the wizard inline

The wizard is a *pure writer of `config.toml`*. It never stores state elsewhere.

### Flow

```
┌─ Step 1 of 8 ──────────────────────────────────────┐
│                                                    │
│  ? Project name: [liat-ball-detection_]            │
│                                                    │
│    (auto-filled from current dir)                  │
│                                                    │
└────────────────────────────────────────────────────┘

┌─ Step 2 of 8 ──────────────────────────────────────┐
│                                                    │
│  ? Which agents do you want to enable?             │
│    (Space to toggle, Enter to confirm)             │
│                                                    │
│    ◉ claude-code    multi-file edits, planning     │
│    ◉ codex          fast surgical, review          │
│    ◉ gemini-cli     long-context review            │
│    ◉ pi             RPC-driven utility             │
│    ◉ opencode       provider-flexible              │
│                                                    │
└────────────────────────────────────────────────────┘

┌─ Step 3 of 8 ──────────────────────────────────────┐
│                                                    │
│  ? Default effort preset:                          │
│                                                    │
│    ❯ quality        opus for reasoning,            │
│                     sonnet for execution           │
│      expensive      opus everywhere                │
│      balanced       opus plan, sonnet impl,        │
│                     haiku exploration              │
│      fast           sonnet+haiku only              │
│                                                    │
│    (You can override per-agent in the next steps)  │
│                                                    │
└────────────────────────────────────────────────────┘

┌─ Step 4 of 8: claude-code ─────────────────────────┐
│                                                    │
│  ? Model for claude-code planning role:            │
│                                                    │
│    ❯ claude-opus-4-7      (from preset)            │
│      claude-sonnet-4-6                             │
│      claude-haiku-4-5                              │
│      custom...                                     │
│                                                    │
│  ? Daily USD limit for claude-code:                │
│    [20.00_]                                        │
│                                                    │
└────────────────────────────────────────────────────┘

... (steps 5–7 similar for other enabled agents) ...

┌─ Step 8 of 8 ──────────────────────────────────────┐
│                                                    │
│  ? Enable skills:                                  │
│    (Space to toggle)                               │
│                                                    │
│    ◉ graphify       Knowledge graph over codebase  │
│    ◯ caveman        Token compression (~65% less)  │
│                                                    │
│  ? Default task merge strategy:                    │
│                                                    │
│    ❯ keep           leave branch, merge manually   │
│      ff             fast-forward to main           │
│      pr             create PR via gh CLI           │
│                                                    │
└────────────────────────────────────────────────────┘

┌─ Review ───────────────────────────────────────────┐
│                                                    │
│   Will write .orca/config.toml:                    │
│                                                    │
│   5 agents enabled (claude-code, codex, gemini,    │
│   pi, opencode)                                    │
│   Effort preset: quality                           │
│   Skills: graphify                                  │
│   Merge strategy: keep                             │
│   Total daily budget: $55 USD                      │
│                                                    │
│   ? Write config and proceed? [Y/n]                │
│                                                    │
└────────────────────────────────────────────────────┘
```

### Implementation notes

- One `InquireStep` per prompt. The wizard is a `Vec<Box<dyn InquireStep>>` that's iterated.
- Skipping/going back: `Esc` at any step returns the user to the TUI (or exits if running standalone) without writing.
- Validation happens inline (e.g., "daily USD limit must be ≥ 0").
- Before the final write, a dry-run displays the full TOML for user review.
- Config is written atomically (temp file + rename). Daemon picks up the change via `notify`.

### Partial config

If `.orca/config.toml` already exists (re-running `orca config`), the wizard pre-fills current values and lets the user Enter-through to keep them.

### Non-interactive mode

For scripting/CI: `orca config --set agents.claude-code.daily_usage_limit_usd=30` writes directly without prompting. Full TOML path syntax supported.

## Surface 2: Control pane (the TUI)

Built with `ratatui`. Launched by `orca` (no subcommand) or `orca tui`. Lives inside tmux pane `orca:0.0`.

### Layout

The control pane is one `Frame` drawn with `ratatui`. Four regions:

```
┌─ Orca ─ liat-ball-detection ─────────────── ●live | daemon: ok | 14:22:18 ─┐
│                                                                            │
│  TASKS                                                                     │
│   ▸ T-007  Fix ref FP in dark kits            implementing   claude-code   │
│     T-008   ├─ refactor color-jitter          done           codex          │
│     T-009   └─ add held-out ref clips         drafted        —              │
│     T-006  Benchmark RF-DETR variants         reviewing      codex          │
│     T-005  Update wandb logging               done           claude-code    │
│     T-004  Survey SoccerNet format            done           gemini-cli     │
│   + 14 older tasks (archived)                                              │
│                                                                            │
├───────────────────────────────────────────────────────────────────────────┤
│  AGENTS          tokens today    cost today      limit        status      │
│                                                                            │
│   ● claude-code     142,031 tok      $1.87      $20/day      busy T-007   │
│   ● codex            58,204 tok      $0.42      $10/day      busy T-006   │
│   ○ gemini-cli       12,100 tok      $0.08      $10/day      idle         │
│   ○ pi                    0 tok      $0.00       $5/day      idle         │
│   ○ opencode              0 tok      $0.00      $10/day      idle         │
│                                                                            │
├───────────────────────────────────────────────────────────────────────────┤
│  RECENT EVENTS                                                             │
│   14:22:18  T-007 agent.usage  +1.2k tokens  $0.03                        │
│   14:21:54  T-006 task.state   reviewing → reviewed                        │
│   14:21:50  T-006 agent.idle   codex                                       │
│   14:19:03  T-006 task.state   implemented → reviewing                     │
│   14:18:11  T-007 task.state   planned → implementing                      │
│   14:17:22  T-007 task.state   planning → planned                          │
│                                                                            │
├───────────────────────────────────────────────────────────────────────────┤
│  [n]ew  [r]oute  [i]mpl  [v]iew  [k]b  [p]ause  [c]onfig  [l]ogs  [q]uit  │
└───────────────────────────────────────────────────────────────────────────┘
```

Regions:

1. **Header bar** — project name, daemon status, time. Red if daemon is down.
2. **Tasks list** — tree view with keyboard navigation. Parent tasks expand to show subtasks.
3. **Agents panel** — one row per configured agent with live token/cost counters.
4. **Events stream** — last ~20 events, auto-scrolling.
5. **Footer** — keybinds for current context.

### Keybinds

Single keystroke commands, grouped by context.

**Global (any focus):**
- `q` or `Ctrl-C` — quit (daemon keeps running)
- `Q` — quit + stop daemon
- `?` — help overlay

**When a task is selected:**
- `n` — new task (opens creation wizard)
- `Enter` — drill into task detail view
- `r` — route/reroute (suggestion overlay)
- `i` — implement (transition planned → implementing, or reviewed → revising)
- `v` — view file (plan.md / review.md / diff, user picks)
- `d` — mark done
- `x` — cancel
- `p` — park
- `b` — break into subtasks (only on blocked)

**Global commands:**
- `k` — KB query (overlay prompt: "query: _")
- `c` — config wizard (inline)
- `l` — logs viewer (tail of selected source)
- `:` — command mode (vim-style, `:route T-007 --to codex`)

**Navigation:**
- `j/k` or `↓/↑` — move selection
- `h/l` — collapse/expand tree node
- `Tab` — cycle focus between Tasks / Agents / Events panels
- `g/G` — top/bottom
- `/` — fuzzy search tasks

### Task detail view

Drill into a task (Enter on task row) shows:

```
┌─ T-007: Fix referee false-positive in dark kits ──────────────────────────┐
│                                                                           │
│  State: implementing (claude-code, 34 min)                                │
│  Created: 14:07 today | Capabilities: multi_file_edit, needs_review       │
│  Worktree: .orca/worktrees/T-007 (branch orca/T-007)                      │
│                                                                           │
│  ┌─ Description ────────────────────────────────────────────────────────┐│
│  │ The card classifier reports false positives on referees wearing dark ││
│  │ kits that are confusable with player kits. Root-cause the jersey-    ││
│  │ color shortcut and propose mitigation.                               ││
│  └──────────────────────────────────────────────────────────────────────┘│
│                                                                           │
│  ┌─ Plan (plan.md) ─────────────────────────────────────────────────────┐│
│  │ # Plan for T-007                                                     ││
│  │ ## Goal                                                              ││
│  │ Eliminate jersey-color shortcut causing ref false-positives.         ││
│  │ ## Approach                                                          ││
│  │ 1. Audit color-jitter augmentation bounds ...                        ││
│  │ [truncated — press v to view full]                                   ││
│  └──────────────────────────────────────────────────────────────────────┘│
│                                                                           │
│  ┌─ Live diff ──────────────────────────────────────────────────────────┐│
│  │ M  src/card_classifier/train.py  +12 -3                              ││
│  │ M  configs/train_card.yaml       +4 -1                               ││
│  │ A  tests/test_ref_fp.py          +45 -0                              ││
│  └──────────────────────────────────────────────────────────────────────┘│
│                                                                           │
│  [a]gent-pane  [p]lan  [r]eview  [d]iff  [n]otes  [Esc] back              │
└───────────────────────────────────────────────────────────────────────────┘
```

### Overlay patterns

Some commands open modal overlays:

**Route overlay** (triggered by `r`):
```
┌─ Route T-007 ──────────────────────────────┐
│                                            │
│  Suggested: claude-code (multi_file_edit)  │
│                                            │
│  Or choose:                                │
│    1. claude-code                          │
│    2. codex                                │
│    3. gemini-cli                           │
│    4. pi                                   │
│    5. opencode                             │
│                                            │
│  [Enter] accept suggestion                 │
│  [1-5] pick specific                       │
│  [Esc] cancel                              │
│                                            │
└────────────────────────────────────────────┘
```

**KB query overlay** (triggered by `k`):
```
┌─ KB Query ─────────────────────────────────────────┐
│                                                    │
│  query: show me the card classifier's data flow_   │
│                                                    │
│  [Enter] run · [Esc] cancel                        │
│                                                    │
└────────────────────────────────────────────────────┘
```

Result renders inline or in a scrollable modal.

### Command mode (`:`)

Vim-style for power users. Full command syntax mirrors the CLI (see `CLI.md`):
```
:task new fix the thing --capabilities surgical_edit
:route T-007 --to gemini-cli
:agent kill codex
:config edit
:kb query "auth flow"
```

Tab completion on agent names and task IDs.

### Diff view

A built-in diff view for worktree content against main. Keys: `j/k` to scroll, `]`/`[` for next/prev hunk, `f` to toggle file list, `o` to open file in $EDITOR. Powered by the `git2` crate (no shell-out).

### Log viewer

`l` opens a log tail for a pluggable source:
- Daemon (`.orca/logs/daemon.log`)
- Current task (`.orca/state/tasks/T-NNN/log.jsonl` pretty-printed)
- Specific agent (`.orca/logs/agents/claude-code.log`)
- Global events (`.orca/state/events.jsonl`)

Uses the same key pattern as `less` (`j/k`, `/search`, `G`, `g`).

### Status indicators

- `●` green — agent busy on a task
- `○` gray — agent idle
- `●` red — agent dead or limit exceeded
- `▸`/`▾` — task has subtasks (collapsed/expanded)
- `✓` — task done
- `!` — task blocked

### Theming

Three themes shipped:
- `default` — tokyo-night-ish, works everywhere
- `solarized-dark`
- `dracula`

Set via `[tui] theme = "..."` in config or `t` keybind for runtime toggle.

### Accessibility notes

- All colors paired with a text or shape indicator (never color-only)
- Keybinds documented inline in footer; `?` shows the full help sheet
- Terminals without true color fall back to 16-color approximations

## Interaction with tmux

Orca assumes it's running inside a tmux session named `orca`. On launch:
1. If inside tmux: use current session, create new windows for agent panes.
2. If outside tmux: create/attach to the `orca` session and run the TUI in window 0.
3. Agent panes are `orca:<agent-id>-<task-id>`, e.g. `orca:claude-code-T007`.

The TUI does NOT try to show agent output itself — that's what the agent's own tmux pane is for. The TUI shows Orca-level state. The user flips to the agent's pane (tmux prefix + pane number) to watch an agent work.

Proposed tmux window layout is recommended, not enforced:
- Window 0: Orca control pane
- Window 1-5: one per active agent

Users can rearrange freely. The TUI tracks panes by name/ID, not position.

## Implementation notes for the TUI crate

```
crates/orca-tui/
├── src/
│   ├── app.rs              # main App struct, event loop
│   ├── event.rs            # key events + daemon events merged
│   ├── daemon_client.rs    # subscribe to daemon events via socket
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── header.rs
│   │   ├── tasks.rs
│   │   ├── agents.rs
│   │   ├── events.rs
│   │   ├── footer.rs
│   │   ├── overlays/
│   │   │   ├── route.rs
│   │   │   ├── kb.rs
│   │   │   └── help.rs
│   │   └── task_detail.rs
│   └── theme.rs
```

State is read-only from the TUI's POV; all mutations go through the daemon RPC. This keeps the UI thin and testable.

Refresh strategy: merge two event streams with `tokio::select!`:
- Terminal key events from `crossterm`
- Daemon events from the Unix socket

Redraw only when state changes. `refresh_hz` in config caps the max redraw rate (default 10Hz; lower on ssh or slow terminals).
