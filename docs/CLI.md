# CLI

The `orca` binary is a single Rust binary with subcommands, using `clap` with the derive API.

## Top-level

```
Usage: orca [OPTIONS] [COMMAND]

Commands (no subcommand launches the TUI):
  init         Set up Orca in the current repo (interactive wizard)
  config       Modify config (interactive) or set individual keys
  daemon       Run the daemon (usually auto-launched; useful for headless/systemd)
  task         Task subcommands: new, list, show, route, route, start, pause, ...
  agent        Agent subcommands: list, kill, reset, status
  kb           Knowledge-base subcommands: init, update, query, path, explain
  status       One-shot status dump (tasks + agents + events)
  watch        Follow events.jsonl live (like tail -f, pretty-printed)
  version      Print version
  doctor       Diagnose setup issues (missing binaries, stale locks, etc.)

Global options:
  --project DIR    Operate on an Orca project in DIR (default: current dir)
  --json           Machine-readable output for all commands that support it
  -q, --quiet      Suppress non-essential output
  -v, --verbose    Extra logging
  -h, --help       Help
```

Running `orca` with no subcommand opens the TUI.

## `orca init`

Interactive setup wizard. See `UI.md § Setup wizard`. Writes `.orca/config.toml`. Also:
- Creates `.orca/` directory tree
- Adds sensible `.gitignore` entries if `.gitignore` exists
- Verifies that each enabled agent's CLI binary is on `$PATH`; warns on missing ones
- Offers to install `graphify` if enabled and not found (`pip install graphifyy`)

Flags:
```
--non-interactive    Require all values via flags
--agents LIST        Comma-separated agent IDs to enable
--effort PRESET      One of: expensive, quality, balanced, fast
--force              Overwrite existing config
```

## `orca config`

Three modes:

**Interactive**: `orca config` — re-runs the wizard with current values pre-filled.

**Edit-in-editor**: `orca config edit` — opens `.orca/config.toml` in `$EDITOR`, validates on save.

**Set**: `orca config set <path> <value>` — set a single key. Example:
```
orca config set agents.claude-code.daily_usage_limit_usd 30
orca config set effort.preset balanced
orca config set agents.codex.enabled false
```

**Get**: `orca config get <path>` — print a single value. Supports `--json`.

**Show**: `orca config show` — print the full resolved config (defaults merged with user overrides).

## `orca daemon`

Launch the daemon in the foreground. Usually you don't call this directly — `orca` (TUI) auto-launches it. Useful for:
- Headless environments (servers, CI, automation)
- `systemd` or similar supervisors
- Debugging

Flags:
```
--foreground, -f     Don't daemonize (default: daemonize)
--pid-file PATH      Override PID file location
--socket PATH        Override socket path
```

To stop a running daemon: `orca daemon stop`. To restart: `orca daemon restart`.

## `orca task`

The meat of the CLI.

### `orca task new <title>`

```
orca task new "fix the referee false-positive" \
  --description-file notes.md \
  --capabilities multi_file_edit,needs_review \
  --context src/card_classifier/ \
  --parent T-007
```

Flags:
- `--title`, `-t` — if title arg not passed
- `--description` — inline description
- `--description-file` — read from file
- `--capabilities` — comma-separated capability tags
- `--context` — one or more files/dirs
- `--parent` — parent task ID (makes this a subtask)
- `--acceptance` — multi-value, acceptance criteria bullets
- `--spec-mode` — enforce acceptance criteria (cavekit-lite)

Prints the new task ID. With `--json`, prints the full task object.

### `orca task list`

```
orca task list [--state STATE] [--agent AGENT] [--since DURATION]
```

Default: shows active + drafted tasks. Archive is hidden unless `--all`.

### `orca task show <id>`

```
orca task show T-007
orca task show T-007 --json
orca task show T-007 --plan         # print plan.md
orca task show T-007 --review       # print review.md
orca task show T-007 --diff         # print git diff of worktree
orca task show T-007 --log          # print log.jsonl
```

### `orca task route <id> --to <agent>`

Manually assign an agent.
```
orca task route T-007 --to codex
orca task route T-007 --accept     # accept dispatcher's top suggestion
```

### `orca task start <id>` / `orca task pause <id>`

Start moves from `drafted` to `planning`. Pause parks.

### `orca task review <id> --by <agent>`

Send an `implemented` task to review.

### `orca task accept <id>` / `orca task revise <id>`

Accept a reviewed task as done, or send back for revision.

### `orca task cancel <id>`

Confirm prompt, then kill agents, remove worktree, mark cancelled.

### `orca task cleanup <id>`

Only valid on `done` tasks that chose `keep` merge strategy. Removes worktree after the user has manually merged.

### `orca task tree [<id>]`

Print the task + subtask tree. With no ID, shows all active roots.

### `orca task export <id> [--format md|json]`

Bundles the task.toml, plan.md, review.md, and log.jsonl into a single file. Useful for archiving or sharing.

## `orca agent`

### `orca agent list`

```
ID            STATUS   MODEL               TODAY   LIMIT    CURRENT
claude-code   busy     claude-sonnet-4.6   $1.87   $20.00   T-007 (implementing)
codex         busy     o3                  $0.42   $10.00   T-006 (reviewing)
gemini-cli    idle     gemini-2.5-pro      $0.08   $10.00   —
pi            idle     kimi-k2.5:cloud     $0.00    $5.00   —
opencode      idle     claude-sonnet-4.6   $0.00   $10.00   —
```

### `orca agent status <id>`

Detailed status for one agent. Includes the last ~50 lines of output.

### `orca agent kill <id>`

Kill the agent's process/pane. The current task goes to `blocked`.

### `orca agent reset <id>`

Kill + restart the agent on the current task (picks up where it left off if possible).

### `orca agent test <id>`

Smoke-test an agent: spawn, send a trivial prompt ("respond with OK"), verify output, tear down. Useful for verifying new installs.

## `orca kb`

Thin wrapper around graphify + any future KB backends.

### `orca kb init`

Runs `graphify .` on the project. Writes to `.orca/kb/`. Also installs the graphify skill into each enabled agent's instructions (adds to `CLAUDE.md`, `AGENTS.md`, `GEMINI.md` as appropriate).

### `orca kb update`

Re-runs graphify in update mode (only changed files).

### `orca kb query <text>`

```
orca kb query "show the card classifier's data flow"
orca kb query "what connects train.py to model.py?"
```

Shells out to `graphify query`. Results printed to stdout.

### `orca kb path <from> <to>`

Shortest-path traversal in the graph.

### `orca kb mcp start` / `orca kb mcp stop`

Start/stop the graphify MCP server. When running, agents that support MCP see it as a tool.

## `orca status`

One-shot snapshot. Useful for scripting or quick checks.

```
$ orca status
Orca — liat-ball-detection — daemon: ok (pid 49231)

Active tasks:
  T-007 Fix referee FP                 implementing   claude-code (32 min)
  T-008   └─ refactor color-jitter     implementing   claude-code

Agents:
  ● claude-code  busy  T-007  tokens 142k  cost $1.87
  ○ codex        idle         tokens  58k  cost $0.42
  ○ gemini-cli   idle         tokens  12k  cost $0.08
  ○ pi           idle         tokens   0   cost $0.00
  ○ opencode     idle         tokens   0   cost $0.00

Today's totals: 212k tokens, $2.37 / $55.00 limit

Last 3 events:
  14:22  agent.usage      claude-code +1.2k tokens
  14:21  task.state       T-006 → reviewed
  14:19  task.state       T-006 → reviewing
```

With `--json` returns the full state tree.

## `orca watch`

Live stream of `events.jsonl`, pretty-printed:

```
$ orca watch
14:22:18  claude-code  usage   +1.2k tokens · $0.03
14:22:03  daemon       dispatch  T-007 → claude-code accepted
14:21:54  codex        idle    task T-006
14:21:50  codex        usage   +843 tokens · $0.01
14:21:47  daemon       state   T-006 reviewing → reviewed
14:21:01  codex        started task T-006
```

With `--json` emits raw JSONL.

With `--filter TYPE` filters (e.g., `--filter task.state` shows only state transitions).

## `orca doctor`

Diagnoses common problems:

```
$ orca doctor
Checking Orca installation...
  ✓ orca binary: v0.1.0
  ✓ daemon running: pid 49231

Checking configured agents...
  ✓ claude-code    v1.8.0 on $PATH
  ✓ codex          v0.4.2 on $PATH
  ✓ gemini-cli     v0.6.0 on $PATH
  ✗ pi             not found on $PATH
       → install with: npm install -g @mariozechner/pi-coding-agent
  ✓ opencode       v0.12.1 on $PATH

Checking skills...
  ✓ graphify       v0.4.9 installed
  ✓ graphify MCP   running on stdio

Checking state...
  ✓ config.toml    valid
  ✓ no stale locks
  ✓ 3 worktrees tracked, all present
  ⚠ events.jsonl is 47MB (consider rotation)

Overall: 1 error, 1 warning
```

Exit code non-zero on errors so CI can gate on it.

## `orca version`

```
$ orca version
orca 0.1.0 (abcd123 2026-04-22)
rustc 1.80.0
```

With `--json`:
```json
{"version":"0.1.0","commit":"abcd123","build_date":"2026-04-22","rustc":"1.80.0"}
```

## Machine-readable mode

Any command that produces human output supports `--json`. The schema is stable across patch versions within a minor version.

Scripting example:
```bash
# Check if a task is done
if orca task show T-007 --json | jq -e '.state == "done"'; then
  echo "Done, merging"
  git merge orca/T-007
fi
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | General error |
| 2 | Usage error (bad flags, missing required args) |
| 3 | Daemon not running and couldn't be started |
| 4 | Agent not found or not enabled |
| 5 | Task not found |
| 6 | State transition not allowed |
| 10 | Configuration invalid |

## Shell completion

`orca completion bash|zsh|fish` outputs a completion script. Standard clap-generated.

## Suggested aliases

The wizard offers to add these to the user's shell rc (opt-in):
```bash
alias ot='orca task'
alias os='orca status'
alias ow='orca watch'
```
