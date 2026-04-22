# RPC Protocol

The Orca daemon exposes a JSON-RPC-ish protocol over a Unix domain socket. CLI subcommands, the TUI, and any future third-party tools speak this protocol to read state and trigger mutations.

This doc specifies the wire format, versioning rules, and every message type the daemon understands.

## Transport

- **Socket**: Unix domain socket at `.orca/state/daemon.sock`.
- **Permissions**: `0600`, owned by the user running the daemon.
- **Framing**: line-delimited JSON. One JSON value per line, UTF-8, terminated by `\n` (`0x0A`). Implementations MUST split on `\n` only — not on Unicode line separators that might appear inside JSON strings.
- **Encoding**: UTF-8 strict. Reject invalid UTF-8 with a protocol error.
- **Direction**: full duplex. Client sends requests; daemon sends responses and async events over the same stream.

## Message envelope

Every message has a common envelope:

```json
{
  "v": 1,
  "id": "req-abc123",
  "kind": "request" | "response" | "event" | "error",
  "type": "...",
  "data": { ... }
}
```

Fields:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `v` | integer | yes | Protocol version. Daemon rejects unknown versions. See `§ Versioning`. |
| `id` | string | sometimes | Correlation ID. Required on `request`/`response`/`error`. Omitted on unsolicited `event`. |
| `kind` | string enum | yes | One of `request`, `response`, `event`, `error`. |
| `type` | string | yes | Specific message type within the kind (e.g. `task.create`, `task.created`, `agent.usage_update`). |
| `data` | object | yes | Type-specific payload. Never null — use `{}` for empty. |

### Request/response correlation

- Client generates a unique `id` per request (UUIDs, monotonic counters, or short random strings — daemon doesn't parse).
- Daemon echoes `id` in the matching `response` or `error`.
- One request yields exactly one response or error (never both, never more than one).
- Events (`kind: event`) carry no `id` and are unsolicited except when issued as a consequence of a subscription (see `§ Subscriptions`).

### Error envelope

```json
{
  "v": 1,
  "id": "req-abc123",
  "kind": "error",
  "type": "task.not_found",
  "data": {
    "code": "TASK_NOT_FOUND",
    "message": "No task with id T-999",
    "details": { "task_id": "T-999" }
  }
}
```

Error `type` uses dotted namespaces mirroring the request type where applicable. Error codes are SCREAMING_SNAKE_CASE strings, stable across minor versions.

Common error codes:

| Code | Meaning |
|---|---|
| `PROTOCOL_VERSION_UNSUPPORTED` | Client sent `v` the daemon doesn't speak |
| `MALFORMED_REQUEST` | JSON parse or schema mismatch |
| `UNKNOWN_MESSAGE_TYPE` | `type` not recognized |
| `TASK_NOT_FOUND` | Referenced task doesn't exist |
| `AGENT_NOT_FOUND` | Referenced agent not configured or disabled |
| `INVALID_TRANSITION` | State machine rejected the transition |
| `LIMIT_EXCEEDED` | Agent daily usage limit hit |
| `DAEMON_BUSY` | An active task prevents the requested action (v0 single-active-task rule) |
| `INTERNAL_ERROR` | Unexpected daemon failure; message contains a ref to log |

## Versioning

- `v` is an integer, starting at `1` for the MVP.
- The daemon advertises supported versions via the `handshake` response (see below).
- A client should `handshake` first and adapt. A well-behaved client never sends `v: 2` without first seeing it in the advertised set.
- Breaking changes bump `v`. The daemon may speak multiple versions concurrently for a transition period; policy is "previous major + current".
- Non-breaking additions (new message types, new optional fields) do NOT bump `v`. Clients must ignore unknown fields.

## Handshake

Every connection starts with a handshake. The daemon will reject subsequent messages until handshake completes.

**Request:**
```json
{"v": 1, "id": "h1", "kind": "request", "type": "handshake",
 "data": {"client": "orca-cli", "client_version": "0.1.0"}}
```

**Response:**
```json
{"v": 1, "id": "h1", "kind": "response", "type": "handshake",
 "data": {
   "daemon_version": "0.1.0",
   "protocol_versions": [1],
   "project": "liat-ball-detection",
   "started_at": "2026-04-22T09:03:11Z"
 }}
```

Clients caching the handshake result for the connection lifetime is fine.

## Message types (v1)

### Lifecycle

#### `handshake` → `handshake`
See above.

#### `ping` → `pong`

Keep-alive for long-lived connections (TUI).

Request:
```json
{"v":1,"id":"p1","kind":"request","type":"ping","data":{}}
```
Response:
```json
{"v":1,"id":"p1","kind":"response","type":"pong","data":{"ts":"2026-04-22T10:00:00Z"}}
```

#### `shutdown` → `ack`

Request the daemon to shut down cleanly. Daemon responds with `ack`, then drains and exits. Used by `orca daemon stop`.

### Config

#### `config.get` → `config`
Data: `{"path": "agents.claude-code.daily_usage_limit_usd"}` — TOML-ish dot path.
Response data: `{"value": 20.0}`.
Errors: `PATH_NOT_FOUND`.

#### `config.set` → `ack`
Data: `{"path": "effort.preset", "value": "balanced"}`.
Daemon validates, writes atomically, reloads, emits `config.changed` event.
Errors: `PATH_NOT_FOUND`, `INVALID_VALUE`, `VALIDATION_FAILED`.

#### `config.show` → `config`
Data: `{}`. Response: `{"config": { ...full resolved config... }}`.

### Tasks

#### `task.create` → `task`
Request data mirrors the task.toml creation form:
```json
{
  "title": "...",
  "description": "...",
  "capabilities": ["multi_file_edit", "needs_review"],
  "context_files": ["src/..."],
  "acceptance": [...],
  "parent": "T-007" // optional
}
```
Response data: `{"task": { ...full Task object... }}`.
Emits `task.created` event.

#### `task.get` → `task`
Data: `{"id": "T-007"}`.

#### `task.list` → `tasks`
Data: `{"state": "implementing", "agent": "claude-code", "include_archived": false}` — all optional filters.
Response data: `{"tasks": [ ... ]}`.

#### `task.update_state` → `task`
Data: `{"id": "T-007", "state": "reviewing", "reason": "user initiated"}`.
Daemon validates the transition. Returns updated task. Emits `task.state_changed`.
Errors: `TASK_NOT_FOUND`, `INVALID_TRANSITION`.

#### `task.assign` → `task`
Data: `{"id": "T-007", "agent": "claude-code", "role": "implementer"}`.
Role is `planner` | `implementer` | `reviewer` | `utility`.
Emits `task.assigned` + `agent.spawned` events.

#### `task.cancel` → `task`
Data: `{"id": "T-007", "force": false}`.
Non-force: errors if uncommitted changes exist (`UNCOMMITTED_WORK`).

#### `task.park` / `task.resume` → `task`
Data: `{"id": "T-007"}`.

#### `task.suggest_agent` → `suggestion`
Data: `{"id": "T-007"}`.
Response data:
```json
{
  "suggestion": {
    "task": "T-007",
    "ranked": [
      {"agent": "gemini-cli", "reason": "long_context"},
      {"agent": "claude-code", "reason": "fallback"}
    ]
  }
}
```
Does not mutate state. Used by TUI to populate the route overlay.

### Agents

#### `agent.list` → `agents`
Response: array of `AgentStatus` objects.

#### `agent.status` → `agent`
Data: `{"id": "claude-code"}`.

#### `agent.kill` → `ack`
Data: `{"id": "claude-code"}`. Kills the agent; current task transitions to `blocked`.

#### `agent.send_prompt` → `ack`
Data: `{"id": "claude-code", "prompt": "..."}`.
Direct prompt injection. Used by the TUI's command mode for quick steering. Use sparingly — the dispatcher and lifecycle should drive most interaction.

### Knowledge base

#### `kb.query` → `kb_result`
Data: `{"query": "auth flow", "budget_tokens": 1500}`.
Response data: `{"result": "...text from graphify..."}`.
Daemon shells out to `graphify query`.

#### `kb.init` / `kb.update` → `ack`

#### `kb.mcp_status` → `mcp_status`
Response: `{"running": true, "pid": 48123, "graph_path": ".orca/kb/graph.json"}`.

### Events (server-pushed)

#### `events.subscribe` → `subscribed`
Data: `{"filter": ["task.*", "agent.usage_update"]}` — glob patterns. Empty array = all.
Response: `{"subscription": "sub-x1"}`.

After this, the daemon pushes matching events on the same connection as `kind: event`. The response `id` echoed on events is the subscription ID, not a request ID.

#### `events.unsubscribe` → `ack`
Data: `{"subscription": "sub-x1"}`.

Event types emitted (`kind: event, type: <name>`):

| Type | Data fields |
|---|---|
| `task.created` | `task` (full object) |
| `task.state_changed` | `task_id`, `from`, `to`, `reason` |
| `task.assigned` | `task_id`, `agent`, `role` |
| `task.completed` | `task_id`, `resolution` (`done` \| `cancelled`) |
| `task.blocked` | `task_id`, `reason` |
| `task.suggestion_ready` | `suggestion` object |
| `agent.spawned` | `agent`, `task_id`, `pane_or_pid` |
| `agent.heartbeat` | `agent`, `last_output_at`, `output_hash` |
| `agent.usage_update` | `agent`, `task_id`, `tokens_in`, `tokens_out`, `cost_usd` |
| `agent.died` | `agent`, `task_id`, `reason` |
| `config.changed` | `path`, `old`, `new` |
| `kb.query_issued` | `query`, `caller` |
| `kb.query_returned` | `query`, `truncated_result`, `duration_ms` |
| `daemon.started` | `daemon_version`, `started_at` |
| `daemon.shutting_down` | `reason` |

Events are also appended to `events.jsonl` on disk (see `STATE.md`). The disk format is the same envelope minus the `v`/`id`/`kind` fields (those are transport concerns).

### Diagnostics

#### `doctor.run` → `doctor_report`
Response data: structured report matching `orca doctor` output.

#### `metrics.usage` → `usage`
Data: `{"range": "today" | "7d" | "30d" | "all", "agent": "claude-code"}` (agent optional).
Response: `{"tokens_in": ..., "tokens_out": ..., "cost_usd": ..., "by_task": [...]}`.

## Subscriptions

A single connection can hold multiple subscriptions. The daemon multiplexes events onto the stream. Clients demultiplex by the subscription ID (carried in the event envelope's `id` field).

The TUI typically opens one connection with one wildcard subscription. CLI `orca watch` does the same and prints.

When a client disconnects, all its subscriptions are dropped server-side.

## Backpressure & flow control

- Daemon's per-connection send buffer is bounded (default 1024 messages).
- If a slow client fills the buffer, daemon drops the **oldest** event and emits an `events.dropped` event noting the count. Request/response messages are never dropped.
- Clients should consume promptly. The TUI specifically should never block the socket read loop on rendering.

## Connection lifecycle

```
connect → handshake → [arbitrary request/response + events] → disconnect
```

- No TLS, no auth tokens — Unix socket permissions are the security boundary.
- Reconnection is the client's responsibility. After a daemon restart, clients re-handshake and re-subscribe.
- The TUI's reconnect logic: on EOF, retry every 500ms up to 10s; if still failing, show "daemon down" banner and offer `r` to retry manually.

## Full example exchange

```
C→D  {"v":1,"id":"1","kind":"request","type":"handshake","data":{"client":"orca-cli","client_version":"0.1.0"}}
D→C  {"v":1,"id":"1","kind":"response","type":"handshake","data":{"daemon_version":"0.1.0","protocol_versions":[1],"project":"liat-ball-detection","started_at":"2026-04-22T09:03:11Z"}}

C→D  {"v":1,"id":"2","kind":"request","type":"task.create","data":{"title":"Fix ref FP","capabilities":["multi_file_edit","needs_review"]}}
D→C  {"v":1,"id":"2","kind":"response","type":"task","data":{"task":{"id":"T-008","state":"drafted",...}}}
D→C  {"v":1,"kind":"event","type":"task.created","data":{"task":{...}}}

C→D  {"v":1,"id":"3","kind":"request","type":"task.suggest_agent","data":{"id":"T-008"}}
D→C  {"v":1,"id":"3","kind":"response","type":"suggestion","data":{"suggestion":{"task":"T-008","ranked":[{"agent":"claude-code","reason":"multi_file_edit"}]}}}

C→D  {"v":1,"id":"4","kind":"request","type":"task.assign","data":{"id":"T-008","agent":"claude-code","role":"planner"}}
D→C  {"v":1,"id":"4","kind":"response","type":"task","data":{"task":{...}}}
D→C  {"v":1,"kind":"event","type":"task.assigned","data":{"task_id":"T-008","agent":"claude-code","role":"planner"}}
D→C  {"v":1,"kind":"event","type":"agent.spawned","data":{"agent":"claude-code","task_id":"T-008","pane_or_pid":"orca:2.0"}}
D→C  {"v":1,"kind":"event","type":"task.state_changed","data":{"task_id":"T-008","from":"drafted","to":"planning"}}

C→D  {"v":1,"id":"5","kind":"request","type":"events.subscribe","data":{"filter":["agent.usage_update"]}}
D→C  {"v":1,"id":"5","kind":"response","type":"subscribed","data":{"subscription":"sub-x1"}}
D→C  {"v":1,"id":"sub-x1","kind":"event","type":"agent.usage_update","data":{"agent":"claude-code","task_id":"T-008","tokens_in":1203,"tokens_out":421,"cost_usd":0.02}}
...
```

## Schema reference (Rust types)

These mirror the RPC types in `orca-core::rpc`. They're the single source of truth — docs should be regenerated from code when types change.

```rust
// Skeleton — see crates/orca-core/src/rpc.rs for canonical definitions.

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Envelope {
    Request(RequestEnvelope),
    Response(ResponseEnvelope),
    Event(EventEnvelope),
    Error(ErrorEnvelope),
}

#[derive(Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub v: u32,
    pub id: String,
    #[serde(flatten)]
    pub body: Request,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Request {
    Handshake { client: String, client_version: String },
    Ping {},
    Shutdown {},
    #[serde(rename = "config.get")]
    ConfigGet { path: String },
    #[serde(rename = "config.set")]
    ConfigSet { path: String, value: serde_json::Value },
    // ... and so on for every message type above
}
```

Note the `#[serde(rename = "config.get")]` pattern — dotted names aren't legal Rust identifiers so we rename on serde.

## Testing

- Round-trip tests for every message type: serialize, deserialize, assert equal.
- Golden-file tests: canonical examples of every message stored in `tests/rpc_fixtures/` and diff-tested against serialization.
- A mock client + mock daemon pair in `crates/orca-core/tests/` for integration tests of the state machine without spawning real agents.

## Non-goals

- **No streaming responses** — every request yields exactly one response. Long operations return an ID and emit events.
- **No request cancellation in v0** — if you send it, it runs to completion. Cancellation is v0.2.
- **No batching in v0** — one request per envelope. Batch is v0.3.
- **No authentication** — Unix socket permissions are the boundary.
- **No TCP** — Unix socket only. Remote control is explicitly not a v0 goal.

If these constraints become painful, revisit in v0.3+ with a `v: 2` protocol.
