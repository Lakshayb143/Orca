# Contributing to Orca

Thanks for your interest. Orca is a small Rust project with strong opinions about how it should be built, so a few minutes reading this file will save you and reviewers time.

> **If you're an AI coding agent** (Claude Code, Codex, Gemini CLI, etc.), read [AGENTS.md](AGENTS.md) instead. This file is for humans.

## Before you start

1. Skim [docs/VISION.md](docs/VISION.md) — Orca has a specific scope and a long list of non-goals. If your idea lands in a non-goal, we probably won't merge it even if the code is great.
2. Check [existing issues](https://github.com/your-org/orca/issues) — your idea may already be tracked.
3. For any change beyond a typo fix or a trivial bug, **open an issue before opening a PR**. A quick paragraph saves a rejected PR.

## Dev setup

### Prerequisites

- Rust stable (project MSRV pinned in `rust-toolchain.toml`)
- `tmux` ≥ 3.0
- `git` ≥ 2.25 (for worktree support)
- For integration tests that spawn real agents, install whichever agents you want to test: `claude`, `codex`, `gemini`, `pi`, `opencode`. None are required for unit tests.

### Clone and build

```bash
git clone https://github.com/your-org/orca.git
cd orca
cargo build --workspace
cargo test --workspace
```

First build is slow (TUI/CLI dependency graph). Subsequent builds hit the cache.

### Run locally

```bash
# Build and run the binary without installing
cargo run --bin orca -- --help

# Run the daemon in the foreground (useful for debugging)
cargo run --bin orca -- daemon --foreground

# In another terminal
cargo run --bin orca -- status
```

### IDE setup

- `rust-analyzer` is assumed. VS Code / Neovim / Helix all work.
- If you use `clippy` inline, set `checkOnSave.command = "clippy"`.
- No project-specific IDE configs are checked in — contributors pick their own.

## The dev loop

1. Pick a ticket from [docs/BUILD_PLAN.md](docs/BUILD_PLAN.md), or open a new issue if yours isn't there.
2. Branch: `git checkout -b <ticket-id>-<short-name>` (e.g. `M1-T04-task-serde`).
3. Write code + tests. Re-read the acceptance criteria when you think you're done.
4. Run the pre-PR checks locally:
   ```bash
   cargo fmt --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   ```
5. Commit. Message format: `[M1-T04] Add Task type with TOML serialization`.
6. Open a PR with the template (auto-filled by `.github/pull_request_template.md` when it exists).

## Coding standards

Short version; the long version is in [AGENTS.md § Coding standards](AGENTS.md#coding-standards) and applies to humans too.

- Rust 2024 edition, `rustfmt` default settings, `clippy -D warnings`.
- Libraries use `thiserror` for errors; binaries use `anyhow`. Don't mix.
- `tracing` for logs, not `println!` (except for CLI output meant for users).
- Tests colocated under `#[cfg(test)] mod tests`. Integration tests in `tests/`.
- Add a test for every bug fix. Even if it's small. Especially if it's small.

## Tests

Three tiers:

1. **Unit** — `cargo test --workspace`. Runs fast, covers invariants, serialization, state machine.
2. **Integration** — same command. Covers the RPC surface end-to-end with a real daemon and fake agent adapters.
3. **Agent integration** — `cargo test --workspace -- --ignored`. Requires real agent binaries + API keys in env. Not run in normal CI; nightly only. Opt in locally with `ORCA_AGENT_IT=1 cargo test -- --ignored`.

Snapshot tests (for TUI rendering) use `insta`. To approve new snapshots: `cargo insta review`.

## PR review process

- CI must be green (fmt + clippy + unit/integration tests).
- At least one maintainer approval.
- If you're touching an architectural piece (state machine, RPC, adapter trait), expect more scrutiny — we want that part of the codebase to stay small and consistent.
- Reviewers: be concrete ("the error path in `foo()` leaks the pid file on panic") rather than vague ("refactor this"). If you need a refactor, describe what the result should look like.

## Docs changes

Docs live in `docs/` as markdown. If your PR changes user-visible behavior, update the relevant doc in the same PR. Doc-only PRs are welcome and land fast.

## Scope creep

Orca's scope is deliberately narrow. We will push back on:

- New agent adapters added to core (open a new issue first; the adapter trait is explicitly designed to support out-of-tree adapters)
- Features that duplicate what a skill/MCP already does (e.g. reimplementing graphify)
- Web UI, hosted service, cloud sync — all explicit non-goals
- LLM-based auto-routing in the core dispatcher (v0.3+ plugin)
- Per-language features that could be a skill instead

None of this means "your idea is bad" — it might be a great skill, plugin, or separate project.

## Security issues

Do not file security issues as public GitHub issues. Email `security@<domain>` with a description and reproducer. We'll respond within 48h and coordinate disclosure. See `SECURITY.md` (when it exists) for details.

## Licensing of contributions

By opening a PR, you agree that your contribution is licensed under the project's MIT license (see `LICENSE`). No CLA; the license handles it.

If you're contributing something you didn't write (e.g. code from another project), flag it clearly and ensure the upstream license is compatible with MIT. Usually this means MIT, Apache-2.0, or BSD-2/3 are fine; GPL/AGPL are not.

## Getting help

- **Code questions**: open a GitHub Discussion, not an issue. Issues are for bugs and features.
- **Design questions**: same — Discussions with the `design` label.
- **Agent-specific weirdness** (e.g. "Claude Code changed its output format and the parser regex broke"): that's a bug. File an issue with a real output sample.

## Releasing (for maintainers)

Not your concern unless you have the keys. Short version: bump versions in `Cargo.toml` (workspace inherits), update `CHANGELOG.md`, tag `vX.Y.Z`, CI publishes. Full docs will live in `docs/RELEASING.md` once it exists.

## Thanks

Every PR that gets merged makes Orca better, and the project exists because people are willing to spend a Saturday writing tmux glue code. If you're one of those people: we appreciate you.
