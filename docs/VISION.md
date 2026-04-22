# Vision

## The problem

AI coding agents are getting good fast, but they're single-player tools. If you want to use three of them on the same project — Claude Code for careful multi-file work, Codex for quick surgical edits, Gemini CLI for repo-wide review — you're stuck managing three terminals, three contexts, three separate worldviews of your codebase. They step on each other's changes, don't share memory, and can't hand off work.

The current state of the ecosystem splits into four camps, none of which solves this:

1. **Session managers** (Claude Squad, Conductor, Crystal) — isolate agents in separate workspaces but provide no coordination. They're tmux wrappers with git worktrees. You still orchestrate manually.
2. **Single-vendor agent teams** (Claude Code Agent Teams) — one lead agent spawns teammates, but they're all the same vendor. No Codex, no Gemini, no heterogeneous capabilities.
3. **Spec-driven orchestrators** (cavekit) — rigorous, proven, but hardcoded: Claude always builds, Codex always reviews, everything flows through specs. No Gemini. Roles aren't configurable.
4. **Parallel-agent desktop apps** (SuperSet, Capy) — GUI-first, not terminal-first, often hosted or semi-hosted.

There's a gap: **a terminal-native, vendor-symmetric, role-configurable orchestrator with a shared KB**. That's what Orca is.

## Who Orca is for

The person who:
- Has paid subscriptions or API keys for 2+ AI coding tools and wants to actually use all of them
- Lives in tmux, not in an IDE-first AI editor
- Wants their orchestration logic to be inspectable — not a black box — so they can see what each agent did and why
- Is building on real codebases where context doesn't fit in any one model's window
- Cares about cost and wants to route cheap tasks to cheap models and hard tasks to expensive ones

Not for: people who just want one agent, or who want a GUI, or who want hosted infrastructure.

## The hypothesis

**If you route tasks to agents based on what each agent is uniquely good at, and share a KB across them, you get output that no single agent could produce alone — at a cost lower than using the strongest agent on everything.**

Concretely:
- Gemini's 1M+ context window is wasted today because nobody routes "review this whole module" to it. They should.
- Codex is cheaper per token than Opus and nearly as good at surgical edits. Route short tasks to it.
- Claude Code is the strongest generalist for planning and multi-file work. Route complex changes to it.
- Pi is the only one with a proper RPC mode. Route scriptable utility work and coordination-layer tasks to it.
- OpenCode bridges to local/on-prem models. Route privacy-sensitive work to it.

A project that uses all five well should cost less and move faster than the same project on Claude Code alone. That's the bet.

## What makes Orca different

In one sentence per competitor:

- **vs. Claude Squad:** We add an orchestration brain above the session layer — dispatcher, state machine, handoffs, review cycles.
- **vs. Claude Code Agent Teams:** Vendor-symmetric. Any agent can play any role.
- **vs. cavekit:** Roles are configurable, not hardcoded. Gemini and Pi and OpenCode are first-class. Specs are optional, not mandatory.
- **vs. aider architect/editor mode:** Three agents, not two. Handoffs between vendors, not within one.
- **vs. SuperSet/Conductor:** Terminal-first, open-source, Rust.
- **vs. Stoneforge/Agent Orchestrator:** KB-integrated via graphify. Pi's RPC mode used for coordinator-level work.

## Design principles

1. **The filesystem is the API.** All state is on disk in `.orca/`. You can inspect it with `cat`, version it with git, and edit it by hand. The daemon watches for changes and reacts. Nothing is hidden in a process's memory.

2. **Agents are peers, not subordinates.** No agent is the "main" agent. Every role is configurable. Claude Code can be the reviewer if you want. Gemini can be the planner. Users decide.

3. **User confirms routing.** The dispatcher *suggests* an agent based on task capabilities. The user always picks. No LLM-based routing in v0 — it's either rules-based suggestion or explicit user choice. Removes a whole class of "the AI decided wrong" failures.

4. **Handoff, not conversation.** Tasks move through a state machine. One agent touches the task at each state. Agents don't talk to each other in freeform; they leave structured artifacts (plan.md, review.md) that the next agent reads. Simpler to reason about, harder to deadlock.

5. **Rigor is opt-in.** Spec mode (cavekit-style kits with R-numbered requirements) is available but off by default. Most tasks are simple — don't force ceremony.

6. **Kill-switch semantics.** Every operation is reversible at the worktree level. If Codex broke something, delete the worktree. The main branch never sees in-progress work.

7. **Observability is not an afterthought.** Every state change emits a JSONL event. Every agent invocation logs its tokens and cost. The TUI surfaces this live; the daemon exposes it via a local socket.

## What's explicitly out of scope

- **Hosted service.** No cloud component. Everything runs on your machine.
- **Web UI** in v0. Maybe v2 — the daemon would expose HTTP endpoints reading the same state.
- **Windows support** in v0. Linux-first, macOS should work with minor adjustments. Windows needs tmux alternatives which is a separate engineering problem.
- **MCP server implementation.** Orca is an MCP *client* (for graphify). It doesn't expose an MCP interface for other tools to call into.
- **Non-coding agents.** Orca is for coding work. A research agent orchestrator is a different product.
- **Browser automation.** If a task needs browser control, that's a subagent's problem, not Orca's.
- **LLM-based dispatch** in v0. Rules + user choice only. LLM-dispatch is a plugin for v0.3+.
- **Multi-project orchestration** in v0. One project at a time. Multi-project is v1+.
- **Inter-agent conversation** in v0. Handoff only. Conversation is v0.3+.

## Success criteria for v0 (MVP)

1. A user can `orca init` a project, pick all 5 agents, and have them configured with models and usage limits.
2. A user can create a task, have it routed (with user confirmation) to any of the 5 agents, watch it execute in a tmux pane, and see the result.
3. A task can move from `drafted → planned → implementing → reviewing → done` with two agents involved (one builder, one reviewer).
4. The TUI shows live task status, agent status, token counts, and cost estimates without the user running refresh commands.
5. graphify integration works: `orca kb init` builds the graph, agents can query it via MCP.
6. If an agent dies mid-task, the user is prompted with options (re-route, wait, park) rather than the task silently breaking.
7. All state is inspectable: `cat .orca/state/tasks/T-001/task.toml` shows the task; `tail -f .orca/state/events.jsonl` shows the event stream.
8. A user who learns Orca can onboard a second user in under 10 minutes by sharing the repo (the config and state travel with it).

Success metric, not just criteria: **a user who pays for both Claude Code and Codex subscriptions stops opening them in separate terminals.** They use Orca.

## Future vision (post-MVP)

v0.2–v0.3: parallel tasks in waves (cavekit pattern), spec mode, richer dispatcher rules, usage dashboards with historical trends, a `orca doctor` for debugging setup issues.

v1.0: plugin system for third-party agents, optional web dashboard, multi-project support, GitHub/Linear integration for task sync, LLM-based dispatcher as an optional plugin.

Beyond v1.0: agent conversation mode (opt-in), autonomous execution with checkpointing, team mode for multiple humans driving the same Orca instance.

None of that gets built until v0 works for at least one real person on at least one real project.
