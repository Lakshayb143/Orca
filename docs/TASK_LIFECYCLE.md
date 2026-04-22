# Task Lifecycle

This doc specifies how a task moves from "a thought in the user's head" to "merged to main," including subtasks, review cycles, and every failure mode.

## The ten states

Recap from `STATE.md`:

```
drafted → planning → planned → implementing → implemented →
                                                           ↓
                                    reviewing ← ← ← ← ← ← (optional)
                                        ↓
                                    reviewed
                                        ↓
                                  [revising or done]
                                        ↓
                                      done

                              blocked (from any active state)
                              parked  (user pause)
```

## Phase 1: Creating a task

A task can come into existence three ways:

**1. TUI: `n` (new task)**
Opens a full-screen inquire prompt:
```
? Title: _
? Description (opens $EDITOR): _
? Capabilities (multi-select): [ ] long_context  [ ] multi_file_edit  ...
? Context files (fuzzy multi-select over repo): _
? Enable KB for this task? [Y/n]
```
Writes `.orca/state/tasks/T-NNN/task.toml`, emits `TaskCreated`.

**2. CLI: `orca task new`**
```bash
orca task new "fix the referee false-positive" \
  --capabilities multi_file_edit,needs_review \
  --context src/card_classifier/
```

**3. Manual: drop a `task.toml` in `.orca/state/tasks/T-NNN/`**
The daemon's filesystem watcher notices and ingests it. This is how you import tasks from external tools (Linear, GitHub Issues, a markdown backlog). `T-NNN` is computed by the daemon if the dir uses a temporary name like `new-task/`.

### Task ID allocation

Sequential, zero-padded: `T-001`, `T-002`, .... The daemon holds the counter in memory, persists it to `.orca/state/.last_id`. Never reused, even after a task is deleted.

### Invariants at creation

- A task is always created in `drafted` state.
- `created_at` = `updated_at` at this moment.
- Worktree is NOT created yet (it's created on transition to `planning`).
- Capabilities are required but can be `["general"]` as a catch-all.

## Phase 2: Assignment & planning

### Dispatch

On `TaskCreated`, the dispatcher computes a `Suggestion`:

```rust
pub struct Suggestion {
    task: TaskId,
    ranked: Vec<(AgentId, Reason)>,   // e.g. [(gemini-cli, "long_context"), (claude-code, "fallback")]
}
```

Emitted as a `SuggestionReady` event. The TUI shows:
```
T-007: Fix referee false-positive
Suggested: gemini-cli (long_context)
Alternatives: claude-code, codex
[Y] accept top · [1–5] pick alt · [n] skip assignment · [e] edit task
```

If `routing.auto_accept = true` in config, the daemon auto-picks the top suggestion and emits `UserDecision` itself. Most users should leave this `false` at first.

### Spawn

On user accept:
1. Daemon creates a git worktree: `git worktree add .orca/worktrees/T-007 -b orca/T-007 <default_branch>`.
2. Symlinks `.orca/state/tasks/T-007/worktree → ../../worktrees/T-007`.
3. Invokes the agent adapter's `spawn(SpawnContext { role: Planner, ... })`.
4. Adapter returns an `AgentHandle`. Daemon stores it.
5. State: `drafted → planning`. Emit `TaskStateChanged`.
6. Write an initial prompt to the agent: the task description + KB preamble + "First, write a plan to `./plan.md` before making any edits."

### What "planning" means

The agent should produce `plan.md` at the worktree root. Recommended structure (prompted via template):

```markdown
# Plan for T-007

## Goal
One sentence restating the task goal.

## Approach
What you'll do, in order. Numbered.

## Files to touch
Path list with a line of why each one matters.

## Risks & unknowns
What could go wrong, what you're uncertain about.

## Acceptance check
How you'll know you're done.
```

Agents are not forced to produce this exact structure, but the default prompt template asks for it and most will comply.

### Planning completion

The adapter's `is_complete` returns true when:
- The agent is idle (TmuxAdapter heuristic) AND
- `plan.md` exists in the worktree AND
- The file has size > 0

On completion: `planning → planned`. Emit event. TUI shows:
```
T-007 plan ready. [i] implement · [r] reject+replan · [o] reroute · [v] view plan
```

## Phase 3: Implementation

User hits `i`. State `planned → implementing`. Daemon sends a new prompt to the same agent: "The plan has been approved. Implement it. Make commits in the worktree."

### What "implementing" means

Agent edits files in the worktree. Ideally commits as it goes (Orca doesn't force this but commit-as-you-go is better for diff review). The adapter watches:
- Output idle
- `git status --porcelain` in the worktree has changes (indicates work happened)
- OR commits landed on the branch

When idle + at least one commit exists on `orca/T-007` past the branch point: `implementing → implemented`.

If the agent never commits and just edits files, the daemon auto-commits uncommitted changes at transition with a message `"[orca] Implementation of T-007 (uncommitted)"` so nothing is lost.

## Phase 4: Review (optional but recommended)

User choices at `implemented`:
- `d` — mark done, merge per `tasks.merge_strategy`
- `r` — send for review
- `x` — reject, revise
- `p` — park for later

On `r`: dispatcher picks a reviewer. Rule: if `task.assigned_to == claude-code`, prefer `codex` for review (adversarial-different-vendor). Override-able.

### Reviewer spawn

Same spawn pattern, but `role = Reviewer`. The reviewer agent gets a different prompt template:
```
You are reviewing the implementation of T-007 in this worktree.
Read plan.md, then inspect the diff against the main branch.
Write review.md with:
- Findings (grouped by severity: P0, P1, P2, P3)
- Specific file:line references
- Suggested fixes
Do not edit any code. Only write review.md.
```

State: `implemented → reviewing`.

### Review completion

When `review.md` exists and reviewer is idle: `reviewing → reviewed`. TUI surfaces findings:
```
T-007 reviewed by codex: 2 P0, 1 P2
  [a] accept anyway · [f] fix findings · [v] view review · [i] ignore & merge
```

### Revise loop

On `f`: `reviewed → revising`. The original implementer agent (or a fresh one, user's choice) gets a new prompt that includes `review.md` and the findings. After work: `revising → implementing` (loop back; a second review cycle is possible).

### Accept

On `a` or `i`: `reviewed → done`.

## Phase 5: Done

Merge strategy from config:

- **`ff` (fast-forward)**: `git merge --ff-only orca/T-007` into default branch. If non-FF, fall back to `keep`.
- **`pr` (pull request)**: `gh pr create --base <default> --head orca/T-007 --title "..." --body-file .orca/state/tasks/T-007/summary.md` (summary auto-generated from plan + review).
- **`keep`**: do nothing. User merges manually. The worktree stays until user runs `orca task cleanup T-007`.

On merge (or on `keep` cleanup):
- Remove worktree: `git worktree remove .orca/worktrees/T-007`
- Delete branch (optional, configurable)
- Move task dir to `.orca/state/tasks/_archive/T-007/`

Archived tasks are still queryable (`orca task show T-007`) but don't clutter the active list.

## Subtasks

Any task can declare subtasks. Two ways they're created:

**1. User explicitly:**
```
orca task new "implement tracking" --parent T-007
```
Creates `T-008` with `parent = "T-007"`. T-007's `subtasks` array gets `["T-008"]`.

**2. Agent-proposed (via plan.md):**
If the planner's plan.md contains a recognized `## Subtasks` section with a list, the daemon auto-proposes them:
```markdown
## Subtasks
- [ ] Refactor the color-jitter augmentation (T-008)
- [ ] Add held-out ref clips to test set (T-009)
```
TUI prompts: "Plan proposes 2 subtasks. Create them? [Y/n]"

### Subtask state vs. parent state

- A parent can't transition to `done` while any subtask is not `done` (or `parked`/`blocked`).
- A parent in `implementing` may have subtasks in any state, including `done`.
- Subtasks can have different assigned agents than the parent. That's the whole point — you can decompose big tasks so Gemini surveys while Claude implements.
- `orca status` renders the tree:
  ```
  T-007 fix referee FP                      implementing  claude-code
    T-008 refactor color-jitter             done          codex
    T-009 add held-out ref clips            implementing  claude-code
  ```

### Breaking a blocked task into subtasks (the recovery pattern)

Your request earlier: when a task gets blocked, the user should be able to break it into smaller tasks with a tracker. Mechanism:

On `blocked`, one of the TUI options is `b` (break down):
```
T-007 blocked: agent died mid-implementation
  [w] wait · [r] reroute · [p] park · [b] break into subtasks
```

On `b`: the daemon spawns a **utility agent** (Pi by default, since it's scriptable and cheap) with a prompt: "This task got stuck. Its plan.md is attached, as is the partial work in the worktree. Propose a decomposition into 2–5 subtasks." Pi returns a JSON array of subtask specs. User approves, subtasks get created, parent transitions `blocked → parked` until subtasks resolve.

This is why Pi being in the roster matters even if the user never directly invokes it — it's the coordinator-layer agent.

## Failure modes

| Failure | Detection | Automatic action | User options |
|---|---|---|---|
| Agent process died | TmuxAdapter: pane closed / PID gone. RpcAdapter: pipe closed or heartbeat timeout | state → blocked(`AgentDied`) | reroute / wait / park / break down |
| Agent hit usage limit | Usage meter trips limit on an `UsageUpdate` event | state → blocked(`LimitExceeded`) | wait (until midnight reset) / reroute to agent with budget / park |
| Agent went into a loop | Output hash repeating for >N turns (configurable, default 5) | state → blocked(`SuspectedLoop`) | kill+reroute / kill+revise / park |
| Daemon crashed | On next `orca` invocation, stale pid file | rebuild state from disk, emit `DaemonRestarted` | continue (state is safe) |
| Git worktree corrupt | Worktree fsck fails | state → blocked(`WorktreeCorrupt`) | rebuild (checkout fresh) / abandon task |
| Merge conflict on `ff` | `git merge --ff-only` rejected | state stays `done` but merge_failed flag set | switch to `pr` mode / resolve manually |
| Spec acceptance unmet (spec mode only) | Reviewer flags uncovered acceptance criteria | state stays `reviewed` with blocker | revise / accept with waiver |

Every failure writes an entry to `log.jsonl` and emits a global event. Nothing fails silently.

## Parking

Parking is user-initiated "set this aside" with intent to resume. Differs from `blocked` (which is forced) and `done` (which is terminal).

- Park from any active state: `p` in the TUI or `orca task park T-007`.
- Parked tasks: worktree is NOT destroyed; agent is killed; state saved.
- Unpark: `orca task resume T-007` — dispatcher re-suggests an agent (since time may have passed), user confirms, state returns to whatever it was before park.

## Limits on active tasks

V0: one active task at a time. The daemon rejects `orca task start T-002` if any task is in `{planning, implementing, reviewing, revising}`. Shows: `T-001 is active. Park it first: orca task park T-001.`

Passive states (`drafted`, `planned`, `implemented`, `reviewed`, `blocked`, `parked`, `done`) don't count; you can have hundreds of tasks in those states.

V0.2 will relax this to allow `{1 active + speculative review of previous}` — cavekit's pattern. V0.3 will allow wave-based parallelism across independent subtasks of the same parent.

## Observability of lifecycle

Every state transition emits both:
1. A line in `.orca/state/events.jsonl` (global)
2. A line in `.orca/state/tasks/T-NNN/log.jsonl` (local)

Power-user monitoring:
```bash
# watch the global event stream
tail -f .orca/state/events.jsonl | jq .

# filter to one task
tail -f .orca/state/tasks/T-007/log.jsonl | jq 'select(.type | startswith("task."))'

# query historical task time-in-state
sqlite3 .orca/cache/tokens.db 'select ...'   # (v0.2 includes a history table)
```

The TUI's event panel is literally `tail -f events.jsonl` under the hood with nicer rendering.

## Cancellation semantics

`orca task cancel T-007` (or `x` in the TUI on an active task):
- Kills the assigned agent (and reviewer if any).
- Removes the worktree.
- Sets state to `cancelled` (11th terminal state, functionally equivalent to `done` but distinguishable in reports).
- Emits `TaskCancelled` event.

User is asked to confirm cancellation: "T-007 has uncommitted work. Cancel anyway? [y/N]"
