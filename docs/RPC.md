# RPC Protocol (v0 stub)

Orca clients and daemon communicate over a Unix socket using newline-delimited JSON.

## Envelope

Every message is a tagged enum encoded as:

```json
{"type":"<Variant>","data":{...}}
```

For unit-like variants, `data` may be omitted by serde.

## Request variants

- `Ping`
- `CreateTask { task }`
- `GetTask { id }`
- `ListTasks`
- `UpdateTaskState { id, state }`

## Response variants

- `Pong`
- `TaskCreated { task }`
- `Task { task }`
- `TaskList { tasks }`
- `TaskStateUpdated { id, state }`
- `Error { message }`

## Versioning

Current protocol version is implicitly `v1` for the scaffold phase. A request-level explicit version field will be added when daemon/client negotiation is introduced.
