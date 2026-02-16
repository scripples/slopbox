# sprites-api

Standalone typed Rust client for the [Sprites REST API](https://sprites.dev).

## Purpose

Provides a typed wrapper around the Sprites API for managing lightweight sandboxed containers (sprites), executing commands, managing checkpoints, network policies, and services.

**Note:** This crate is not currently integrated — no workspace crate depends on it. It exists as a future building block for a `SpritesProvider` in `cb-infra`.

## API Coverage

### Sprites (5 endpoints)

| Endpoint | Method | Path |
|----------|--------|------|
| Create Sprite | `create_sprite` | `POST /sprites` |
| List Sprites | `list_sprites` | `GET /sprites` |
| Get Sprite | `get_sprite` | `GET /sprites/{name}` |
| Update Sprite | `update_sprite` | `PUT /sprites/{name}` |
| Delete Sprite | `delete_sprite` | `DELETE /sprites/{name}` |

### Exec (3 endpoints)

| Endpoint | Method | Path |
|----------|--------|------|
| Exec Command | `exec` | `POST /sprites/{name}/exec` |
| List Sessions | `list_exec_sessions` | `GET /sprites/{name}/exec` |
| Kill Session | `kill_exec_session` | `POST /sprites/{name}/exec/{id}/kill` |

### Checkpoints (4 endpoints)

| Endpoint | Method | Path |
|----------|--------|------|
| Create Checkpoint | `create_checkpoint` | `POST /sprites/{name}/checkpoint` |
| List Checkpoints | `list_checkpoints` | `GET /sprites/{name}/checkpoints` |
| Get Checkpoint | `get_checkpoint` | `GET /sprites/{name}/checkpoints/{id}` |
| Restore Checkpoint | `restore_checkpoint` | `POST /sprites/{name}/checkpoints/{id}/restore` |

### Network Policy (2 endpoints)

| Endpoint | Method | Path |
|----------|--------|------|
| Get Policy | `get_network_policy` | `GET /sprites/{name}/policy/network` |
| Set Policy | `set_network_policy` | `POST /sprites/{name}/policy/network` |

### Services (6 endpoints)

| Endpoint | Method | Path |
|----------|--------|------|
| List Services | `list_services` | `GET /sprites/{name}/services` |
| Get Service | `get_service` | `GET /sprites/{name}/services/{svc}` |
| Create/Update Service | `create_service` | `PUT /sprites/{name}/services/{svc}` |
| Start Service | `start_service` | `POST /sprites/{name}/services/{svc}/start` |
| Stop Service | `stop_service` | `POST /sprites/{name}/services/{svc}/stop` |
| Get Logs | `get_service_logs` | `GET /sprites/{name}/services/{svc}/logs` |

All paths are relative to `https://api.sprites.dev/v1`.

## Types

### Core Types

- **`Sprite`** — `id`, `name`, `organization`, `status` (Cold/Warm/Running), `url`, `url_settings`, timestamps
- **`ExecResult`** — `stdout`, `stderr`, `exit_code`
- **`ExecSession`** — `id`, `command`, `is_active`, `tty`, `workdir`, timestamps
- **`Checkpoint`** — `id`, `create_time`, `source_id`, `comment`
- **`NetworkPolicy`** — `rules: Vec<NetworkPolicyRule>`
- **`NetworkPolicyRule`** — `domain`, `action` (Allow/Deny), `include`
- **`Service`** — `name`, `cmd`, `args`, `needs`, `http_port`, `state`
- **`ServiceState`** — `name`, `status`, `pid`, `started_at`, `error`

### NDJSON Streaming Types

Several endpoints return NDJSON (newline-delimited JSON) as a raw `String`. Callers parse each line:

- **`StreamEvent`** (checkpoints) — `Info { data, time }`, `Error { error, time }`, `Complete { data, time }`
- **`KillEvent`** (exec kill) — `Signal`, `Timeout`, `Exited`, `Killed`, `Error`, `Complete { exit_code }`

NDJSON endpoints: `kill_exec_session`, `create_checkpoint`, `restore_checkpoint`, `start_service`, `stop_service`, `get_service_logs`.

## Authentication

Bearer token via `Authorization: Bearer <token>` header.

```rust
let client = SpritesClient::new("sprites-token");
```

## Error Handling

```rust
pub enum Error {
    Request(reqwest::Error),           // Network/transport errors
    Api { endpoint, status, body },    // Non-2xx responses
}
```

Delete operations treat `404` as success.

## Dependencies

- `reqwest` 0.12 — HTTP client
- `serde` / `serde_json` — JSON serialization
- `chrono` — DateTime types
- `thiserror` — Error derive
