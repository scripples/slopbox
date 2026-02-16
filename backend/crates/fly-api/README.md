# fly-api

Standalone typed Rust client for the [Fly.io Machines REST API](https://fly.io/docs/machines/api/).

## Purpose

Provides a thin, typed wrapper around the Fly.io Machines API for creating and managing Fly Machines. Used by `cb-infra`'s `FlyProvider` — not intended for direct use by application code.

## API Coverage

| Endpoint | Method | Path | Returns |
|----------|--------|------|---------|
| Create Machine | `create_machine` | `POST /machines` | `Machine` |
| Get Machine | `get_machine` | `GET /machines/{id}` | `Machine` |
| Start Machine | `start_machine` | `POST /machines/{id}/start` | `()` |
| Stop Machine | `stop_machine` | `POST /machines/{id}/stop` | `()` |
| Delete Machine | `delete_machine` | `DELETE /machines/{id}` | `()` |

All paths are relative to `https://api.machines.dev/v1/apps/{app}`.

## Types

### Request Types

- **`CreateMachineRequest`** — `name`, `region`, `config: MachineConfig`
- **`MachineConfig`** — `image`, `env` (optional), `guest: GuestConfig`, `mounts` (optional), `files` (optional), `auto_destroy` (optional)
- **`GuestConfig`** — `cpus`, `cpu_kind`, `memory_mb`
- **`MachineMount`** — `volume`, `path`
- **`MachineFile`** — `guest_path`, `raw_value`

### Response Types

- **`Machine`** — `id`, `name`, `state`, `region`, `private_ip` (optional)

## Authentication

Bearer token via `Authorization: Bearer <token>` header on every request.

```rust
let client = FlyClient::new("fly-token", "my-app");
```

## Error Handling

```rust
pub enum Error {
    Request(reqwest::Error),           // Network/transport errors
    Api { endpoint, status, body },    // Non-2xx responses
}
```

Delete operations treat `404` as success for idempotency.

## Dependencies

- `reqwest` 0.12 — HTTP client
- `serde` / `serde_json` — JSON serialization
- `thiserror` — Error derive
