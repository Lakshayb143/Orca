# AGENTS.md

Instructions for AI coding agents (Claude Code, Codex, Gemini CLI, Pi, OpenCode, Aider, and others) working on the Orca codebase itself.

This file is read automatically by agents that respect the AGENTS.md convention. The same instructions also live in `CLAUDE.md` (symlinked) for Claude Code's convention.

## What Orca is

Orca is a Rust CLI + TUI that orchestrates multiple AI coding agents. Read [README.md](README.md) first if you don't have project context.

You (the agent reading this) are building Orca. Orca itself will eventually orchestrate you. This is both an ergonomic project to work on (you understand the domain) and a high-stakes one (bugs will be obvious to future-you using it).

## Read before writing code

In priority order:

1. [docs/VISION.md](docs/VISION.md) — why Orca exists, what's explicitly out of scope
2. [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — the system design you must respect
3. [docs/STATE.md](docs/STATE.md) — data schemas, state machine, invariants
4. [docs/BUILD_PLAN.md](docs/BUILD_PLAN.md) — the milestone + ticket you're working on

For specific areas:
- Working on agent adapters → [docs/AGENT_ADAPTERS.md](docs/AGENT_ADAPTERS.md)
- Working on task flow → [docs/TASK_LIFECYCLE.md](docs/TASK_LIFECYCLE.md)
- Working on the TUI → [docs/UI.md](docs/UI.md)
- Working on CLI surface → [docs/CLI.md](docs/CLI.md)
- Working on KB → [docs/KB.md](docs/KB.md)

## Coding standards

- **Rust edition**: 2024.
- **MSRV**: current stable at project start (to be frozen once M0-T01 lands).
- **Formatting**: `rustfmt` with default settings. CI enforces.
- **Linting**: `clippy` with `-D warnings`. CI enforces.
- **Errors**: libraries return `thiserror`-based types; binaries use `anyhow`. Don't mix.
- **Async**: use `tokio`. Avoid blocking syscalls on async functions.
- **Panics**: reserve for "this is a bug" invariant violations. Expected failures return `Result`.
- **Logging**: use `tracing`. No `println!` except in CLI output for users. No `eprintln!` except for early-startup errors before tracing is initialized.
- **Tests**: colocated `#[cfg(test)] mod tests` per file. Integration tests in `tests/`. Name them clearly — `test_task_can_transition_from_drafted_to_planning`, not `test_1`.

## Commit discipline

- Each ticket ID maps to one PR, typically one or two commits.
- Commit messages: `[ticket-id] <short imperative>`. Body explains *why* if non-obvious.
- Example: `[M1-T04] Add Task type with TOML serialization`
- No "wip" or "fixes" in main-bound commits. Squash locally first.
- Every PR updates the ticket's status in the BUILD_PLAN.md (strikethrough when done).

## PR workflow

1. Check out `main`, branch to `orca/<ticket-id>` (Orca itself uses this convention for its own dogfooding).
2. Implement. Run `cargo test --workspace`, `cargo clippy -- -D warnings`, `cargo fmt --check`.
3. Open PR. Body includes:
   - Ticket ID
   - Scope done (bullet list)
   - Acceptance criteria status (quote them, ✅/❌ each)
   - Out-of-scope items deferred (bullet list)
4. Wait for review. Address feedback.
5. Merge.

## The non-obvious rules

These save future-you from regressions:

### Rule 1: the filesystem is canonical

Don't store state in the daemon's memory that isn't written through to disk. If `orca daemon` crashes and restarts, everything should be recoverable from `.orca/`. This means:
- Every state mutation is a file write (atomic: temp + rename).
- In-memory caches are *reads*, not authoritative writes.
- If you're tempted to add a field that only lives in memory: stop, write a task.toml field for it.

### Rule 2: agents are opaque

Never assume anything about what agents output beyond what's specified in each agent's integration notes (in `docs/AGENT_ADAPTERS.md`). Don't regex the hell out of their output looking for specific phrases unless it's documented. The usage parsers are an exception because they're explicitly allowed to be brittle, but even there: test fixtures with real output, never made-up examples.

### Rule 3: events are append-only

Never rewrite `events.jsonl`. Ever. Not for formatting, not for compression, not for "cleanup." It's an audit log. Rotation creates new files; it doesn't modify old ones.

### Rule 4: transitions always emit events

Every state transition in `Task` or `Agent` emits exactly one event. Don't transition silently. Don't emit events without a state change. The invariant is: `replaying events.jsonl from scratch should reconstruct current state`.

### Rule 5: the dispatcher suggests, the user decides

No LLM-based auto-routing in v0. Keep the dispatcher rule-based and deterministic. This isn't because LLM routing is a bad idea — it's because for a tool that's about user control, adding opaque model-based decisions early erodes trust. Plugin interface for LLM dispatch comes in v0.3+.

### Rule 6: worktrees are sacrosanct

Never write to the main working tree directly. Every piece of generated code lives in a worktree under `.orca/worktrees/<task-id>/`. Rebuilding the main tree is never Orca's job.

### Rule 7: one active task

Until v0.2, the daemon must enforce "at most one task in an active state." If you find yourself wanting to parallelize something, ask yourself if you're building v0 scope or v0.2 scope. When in doubt, ship serial and let the future version handle parallelism.

## Testing philosophy

- **Unit tests**: cover invariants, transitions, serialization, parsers.
- **Integration tests**: cover the RPC surface end-to-end with a real daemon + fake adapters.
- **Agent integration tests**: `#[ignore]` by default (require real agent binaries + API keys). CI runs them nightly with secrets.
- **UI tests**: snapshot tests for ratatui rendering using `insta`.
- **Fuzz**: config parser and task.toml parser are good targets. Low priority for MVP.

Coverage target is not a number — it's "every non-trivial branch has a test." Aim for that, not %.

## Dependency hygiene

- Add a dep only when the alternative is clearly worse. Every dep is a future vulnerability + bloat + compile-time hit.
- Prefer well-maintained, widely-used crates. Avoid single-contributor pre-1.0 dependencies for core paths.
- Document non-obvious dep choices in the relevant code section.
- Run `cargo audit` in CI.

## Performance targets (MVP)

These are ceiling estimates, not hard SLOs. If you're blowing past them, pause and think.

- Daemon startup: <500ms from invocation to accepting RPC connections.
- CLI subcommand latency (non-agent-involving): <200ms median.
- TUI redraw cost: <16ms per frame (60fps achievable, 10fps targeted by default).
- Task state transition (daemon-side): <50ms including disk write.
- Event publish → subscriber receive: <100ms.

No benchmarks in CI yet. Add when you have reason to believe something regressed.

## Security reminders

- API keys never touch disk via Orca. If a key is in `secrets.toml`, the user put it there. Orca reads, never writes keys.
- Daemon socket is chmod 600. No network listening in v0.
- Subprocess invocations pass args via argv, not shell-escaped strings. No `sh -c`-style composition from user input.
- Git operations in worktrees only. Never `git reset --hard` the main tree.

## What to do when stuck

In order of preference:

1. Re-read the relevant doc. Seriously. Docs are written to answer implementation questions.
2. Check `docs/BUILD_PLAN.md § Open design questions` — if your question is listed, make a call and document it in the PR.
3. Open a GitHub issue tagged `design-question` with the ticket ID and a concrete proposal + alternatives.
4. Don't invent architecture unilaterally. If ARCHITECTURE.md doesn't cover your situation, it's a gap to be filled by discussion, not by your best guess silently becoming the answer.

## When working with other agents (you're part of an Orca build team)

If Claude Code just wrote code that Codex is now reviewing (or vice versa):

- **Reviewers**: be adversarial. Find real problems. Don't rubber-stamp.
- **Builders**: respond to every finding. Fix or dismiss with a reason. Never "noted" without action.
- **Both**: the project docs are the source of truth. If you disagree with a doc, open a PR to change the doc first, then the code.

If Gemini is doing a long-context review, its job is architectural cross-cutting concerns that per-file review might miss — name collisions, API inconsistencies, places where a change to module A should have prompted a change to module B but didn't. Trust its repo-wide view.

If Pi is running a utility task (generating a changelog, checking links, running a decomposition), treat its output as structured data to be validated, not as prose to be trusted.

## Style notes

- Comments explain *why*, not *what*. The code shows what.
- Docstrings on public items required. Private items need them if the intent isn't obvious.
- Prefer small types with clear invariants over large types with lots of `Option`s.
- `impl Trait` in return position fine; overuse of associated types makes the code hard to follow.
- When in doubt, match the style of existing code in the same crate.

## Release cadence (post-MVP)

Not your concern during M0–M7. But for context: patch releases weekly, minor releases monthly, major releases when we break config or RPC schema. Semver applied strictly.

## Have fun

You're building the tool you'll use tomorrow to build everything else. That's the good version of recursion.
