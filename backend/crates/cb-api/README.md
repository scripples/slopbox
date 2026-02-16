# cb-api

Control plane API server, forward proxy, and background monitor for Cludbox.

## Purpose

The main binary crate. Runs three components:

1. **Control Plane API** (axum, `:8080`) — Agent/VPS lifecycle, usage checks, account management
2. **Forward Proxy** (hyper, `:3128`) — Authenticates and meters all outbound agent traffic
3. **Background Monitor** — Polls VPS metrics and accumulates usage deltas

## Routes

All routes require `Authorization: Bearer <CONTROL_PLANE_API_KEY>` and `X-User-Id: <uuid>` headers.

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| POST | `/agents` | `create_agent` | Create a new agent |
| GET | `/agents` | `list_agents` | List user's agents |
| GET | `/agents/{id}` | `get_agent` | Get agent details |
| DELETE | `/agents/{id}` | `delete_agent` | Delete agent (destroys attached VPS) |
| POST | `/agents/{id}/vps` | `provision_vps` | Provision VPS for agent |
| DELETE | `/agents/{id}/vps` | `destroy_vps` | Destroy VPS |
| POST | `/agents/{id}/vps/start` | `start_vps` | Start stopped VPS |
| POST | `/agents/{id}/vps/stop` | `stop_vps` | Stop running VPS |
| GET | `/agents/{id}/usage` | `get_usage` | Current period usage metrics |
| GET | `/users/me/overage-budget` | `get_overage_budget` | Current month's overage budget |
| PUT | `/users/me/overage-budget` | `set_overage_budget` | Set current month's overage budget |
| GET | `/users/me` | `get_me` | Current user profile + plan |
| GET | `/plans` | `list_plans` | Available plans |
| PUT | `/agents/{id}/config` | `update_config` | Update OpenClaw config via gateway RPC |
| PUT | `/agents/{id}/workspace/{filename}` | `update_workspace_file` | Write workspace file via gateway /tools/invoke |
| POST | `/agents/{id}/restart` | `restart_agent` | Restart gateway via gateway RPC |
| GET | `/agents/{id}/health` | `agent_health` | Health check via gateway HTTP |
| POST | `/agents/{id}/channels` | `add_channel` | Add messaging channel |
| GET | `/agents/{id}/channels` | `list_channels` | List agent's channels |
| DELETE | `/agents/{id}/channels/{kind}` | `remove_channel` | Remove channel |

**Gateway Proxy (session auth):**

| Method | Path | Handler | Description |
|--------|------|---------|-------------|
| ANY | `/agents/{agent_id}/gateway/{*path}` | `proxy_http` | Proxy HTTP to OpenClaw gateway on VM |
| GET/WS | `/agents/{agent_id}/gateway/ws` | `proxy_ws` | Proxy WebSocket with handshake interception + RPC method filtering |

## Modules

### `auth` — Authentication Middleware

Shared-secret BFF model: a single `CONTROL_PLANE_API_KEY` authenticates the frontend. The frontend passes `X-User-Id` to identify which user the request is for. The middleware injects `UserId(Uuid)` into request extensions.

### `proxy` — Forward Proxy

Spawns a standalone TCP listener (default `:3128`). Handles two traffic types:

**Authentication:**
- Agents authenticate with `Proxy-Authorization: Basic base64(agent_id:gateway_token)`
- Validated against `Agent::get_by_id_and_token()` in the database
- Failure returns `407 Proxy Authentication Required`

**Usage enforcement (provider-aware):**
- **Hetzner VPSes**: proxy skips usage checks entirely — the background monitor handles enforcement by stopping servers
- **Fly VPSes (and other providers)**: checks aggregate user-level usage against plan limits + overage budget
- If within plan limits → OK; if over plan limits, computes overage cost and checks against user's overage budget
- Returns `403 Forbidden` only when both plan limits and overage budget are exhausted

**CONNECT tunneling (HTTPS):**
- Establishes TCP connection to target
- Sends `200 OK` to client, upgrades to raw TCP
- Bidirectional relay with byte counting (ingress + egress)
- Flushes total to `VpsUsagePeriod::add_bandwidth()`

**HTTP forwarding:**
- Forwards request via `reqwest::Client` (strips `Proxy-Authorization`)
- Counts request body (egress) + response body (ingress)
- Flushes total to `VpsUsagePeriod::add_bandwidth()`

### `monitor` — Background Metrics Monitor

Defines the `MetricsCollector` trait and runs a polling loop:

```rust
#[async_trait]
pub trait MetricsCollector: Send + Sync + 'static {
    async fn collect(&self, vps: &Vps) -> Result<VpsMetrics, BoxError>;
}
```

`VpsMetrics` contains `storage_used_bytes`, `cpu_used_ms`, `memory_used_mb_seconds`.

The default `StubCollector` returns existing DB values (no real metrics collection). The monitor loop:
1. Lists all Running VPSes
2. Collects metrics for each
3. Computes deltas (new - old, clamped to 0)
4. Upserts deltas into `VpsUsagePeriod`
5. Updates absolute values on the `Vps` row
6. Runs `enforce_limits()`: for each user with running Hetzner VPSes, checks aggregate usage against plan limits + overage budget; stops VPSes via provider when budget is exhausted

### `config` — Application Configuration

```rust
pub struct AppConfig {
    pub database_url: String,
    pub listen_addr: SocketAddr,          // default: 0.0.0.0:8080
    pub control_plane_api_key: String,
    pub monitor_interval_secs: u64,       // default: 60
    pub proxy_listen_addr: SocketAddr,    // default: 0.0.0.0:3128
    pub proxy_external_addr: String,      // default: cb-api:3128
}
```

### `state` — Shared Application State

```rust
pub struct AppState {
    pub db: PgPool,
    pub providers: ProviderRegistry,
    pub config: AppConfig,
}
```

### `error` — API Error Types

| Variant | HTTP Status |
|---------|-------------|
| `NotFound` | 404 |
| `LimitExceeded(msg)` | 403 |
| `BadRequest(msg)` | 400 |
| `Unauthorized` | 401 |
| `Conflict(msg)` | 409 |
| `Database(sqlx::Error)` | 500 (or 404 if RowNotFound) |
| `Infra(cb_infra::Error)` | 502 |
| `Internal(msg)` | 500 |

### `dto` — Request/Response Types

**Requests:** `CreateAgentRequest` (`name`), `ProvisionVpsRequest` (`vps_config_id`), `SetOverageBudgetRequest` (`budget_cents`)

**Responses:** `AgentResponse`, `VpsResponse`, `UsageResponse` (with `UsageMetric` for bandwidth, storage, cpu, memory + `overage_cost_cents`, `overage_budget_cents`), `UserResponse`, `PlanResponse` (with overage rate fields), `OverageBudgetResponse`

## VPS Provisioning Flow

1. Validate agent ownership, check VPS count limit, validate VpsConfig belongs to plan
2. Derive provider and image from VpsConfig (provider + image are per-config, not global)
3. Insert Vps row (state: Provisioning)
4. Assign VPS to agent
5. Build VpsSpec with env vars including `HTTP_PROXY`/`HTTPS_PROXY` pointing to the forward proxy
6. Create VPS via provider (storage is provider-managed)
7. Update Vps row with provider ID and address
8. Set state to Running

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string |
| `LISTEN_ADDR` | No | `0.0.0.0:8080` | API server bind address |
| `CONTROL_PLANE_API_KEY` | Yes | — | Bearer token for API auth |
| `MONITOR_INTERVAL_SECS` | No | `60` | Metrics polling interval |
| `PROXY_LISTEN_ADDR` | No | `0.0.0.0:3128` | Proxy bind address |
| `PROXY_EXTERNAL_ADDR` | No | `cb-api:3128` | Proxy address advertised to agents |
| `RUST_LOG` | No | `info` | Tracing log filter |

Plus all `VPS_PROVIDER` / `FLY_*` / `HETZNER_*` variables from `cb-infra`.

## Dependencies

- `axum` 0.8 — HTTP framework
- `hyper` / `hyper-util` / `http-body-util` — Raw HTTP for proxy
- `reqwest` 0.12 — HTTP client (proxy forwarding)
- `sqlx` 0.8 — Database (via cb-db)
- `cb-db` — Models and migrations
- `cb-infra` — VPS provider abstraction
- `tower` / `tower-http` — Middleware (tracing layer)
- `base64` — Proxy auth decoding

## Known Limitations

- `StubCollector` means the monitor produces no real metrics — CPU/memory deltas are always zero.
- Proxy only handles outbound traffic (agent → internet). Inbound traffic to VPSes is not gated.
