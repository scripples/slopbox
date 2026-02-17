# Slopbox

SaaS platform for renting sandboxed AI agents as containerized apps. Each agent runs inside an isolated microVM (Fly.io Machine or Hetzner Cloud server) with provider-managed storage. Users interact with agents through a forward proxy that gates and meters all traffic. The control plane handles provisioning, billing limits, and account management.

## Architecture

```
                          ┌─────────────────────────────────────┐
                          │           Control Plane             │
                          │                                     │
  Frontend ──────────────►│  API Server (:8080)                 │
  (Auth.js + Next.js)     │    ├── Agent CRUD                   │
                          │    ├── VPS lifecycle                 │
                          │    └── Usage metering                │
                          │                                     │
  Agent VPS ─────────────►│  Forward Proxy (:3128)              │──────► Internet
  (outbound traffic)      │    ├── Auth (agent_id:token)        │
                          │    ├── Bandwidth enforcement         │
                          │    └── CONNECT tunneling + HTTP fwd  │
                          │                                     │
                          │  Background Monitor                 │
                          │    ├── Polls VPS metrics             │
                          │    └── Enforces overage limits       │
                          └──────────┬──────────────────────────┘
                                     │
                          ┌──────────▼──────────────────────────┐
                          │         VPS Provider                 │
                          │  (VpsProvider trait)                  │
                          │                                     │
                          │  ┌─────────────┐ ┌────────────────┐ │
                          │  │  FlyProvider │ │HetznerProvider │ │
                          │  │  (fly-api)   │ │  (hcloud)      │ │
                          │  └──────┬──────┘ └───────┬────────┘ │
                          └─────────┼────────────────┼──────────┘
                                    │                │
                              Fly.io API      Hetzner Cloud API
```

## Crate Map

| Crate | Type | Purpose |
|-------|------|---------|
| `cb-api` | Binary | Control plane API, forward proxy, gateway proxy, background monitor |
| `cb-db` | Library | PostgreSQL models + migrations (sqlx) |
| `cb-infra` | Library | VpsProvider trait + Fly/Hetzner implementations |
| `fly-api` | Library | Typed Fly.io Machines REST API client |
| `sprites-api` | Library | Typed Sprites REST API client (not yet integrated) |

## Data Flows

### Agent Provisioning

1. Frontend calls `POST /agents` → creates Agent record
2. Frontend calls `POST /agents/{id}/vps` with a `vps_config_id`
3. Control plane creates a VPS with `HTTP_PROXY`/`HTTPS_PROXY` env vars pointing back to the forward proxy (storage is provider-managed)
4. VPS boots, starts OpenClaw agent service
5. Agent is now reachable and all its outbound traffic flows through the proxy

### Outbound Proxy Traffic

1. Agent makes HTTP/HTTPS request → OS routes through configured `HTTP_PROXY`
2. Forward proxy authenticates via `Proxy-Authorization: Basic base64(agent_id:gateway_token)`
3. Proxy checks usage against plan limits + overage budget (skipped for Hetzner — monitor handles enforcement)
4. CONNECT (HTTPS): bidirectional TCP tunnel with byte counting
5. Plain HTTP: request/response forwarding with byte counting
6. Bytes accumulated into `vps_usage_periods` table

## Environment Variables

### Required

| Variable | Crate | Description |
|----------|-------|-------------|
| `DATABASE_URL` | cb-api | PostgreSQL connection string |
| `CONTROL_PLANE_API_KEY` | cb-api | Bearer token for API authentication |

### Provider Selection

| Variable | Default | Description |
|----------|---------|-------------|
| `VPS_PROVIDER` | `fly` | `fly` or `hetzner` |

### Fly.io Provider

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `FLY_API_TOKEN` | Yes | — | Fly.io API token |
| `FLY_APP_NAME` | No | `slopbox-agents` | Fly app name |
| `FLY_REGION` | No | `iad` | Default region |

### Hetzner Cloud Provider

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `HETZNER_API_TOKEN` | Yes | — | Hetzner API token |
| `HETZNER_LOCATION` | No | `fsn1` | Datacenter location |
| `HETZNER_NETWORK_ID` | No | — | Private network ID |
| `HETZNER_FIREWALL_ID` | No | — | Firewall ID |
| `HETZNER_SSH_KEY_NAMES` | No | — | Comma-separated SSH key names |

### Application

| Variable | Default | Description |
|----------|---------|-------------|
| `LISTEN_ADDR` | `0.0.0.0:8080` | API server bind address |
| `PROXY_LISTEN_ADDR` | `0.0.0.0:3128` | Forward proxy bind address |
| `PROXY_EXTERNAL_ADDR` | `cb-api:3128` | Proxy address advertised to agents |
| `AGENT_BASE_IMAGE` | `slopbox/agent-base:latest` | Container image for VPSes |
| `MONITOR_INTERVAL_SECS` | `60` | Metrics polling interval |
| `RUST_LOG` | `info` | Tracing log filter |

## CI/CD

### Backend (`.github/workflows/ci.yml`)

Triggers on push to `master` or PR when `backend/**` files change.

**Jobs:**
1. **check** — `cargo fmt --check` + `cargo clippy --workspace -- -D warnings` (Rust 1.93.1)
2. **deploy** — `flyctl deploy --remote-only` (only on push/dispatch, after check passes)

**Required GitHub Secrets:**

| Secret | Description |
|--------|-------------|
| `FLY_API_TOKEN` | Fly.io deploy token. Generate via `fly tokens create deploy -a slopbox-api` |

### Frontend

Not yet in CI. Deployed to Vercel (auto-deploys on push once connected).

## Infrastructure

| Service | Provider | Purpose |
|---------|----------|---------|
| Control plane | Fly.io (`slopbox-api`) | API server + forward proxy |
| Database | Neon Postgres | Shared by backend + frontend Auth.js |
| Agent VMs | Hetzner Cloud / Sprites | One microVM per agent |

### Fly.io Secrets (backend)

| Secret | Description |
|--------|-------------|
| `DATABASE_URL` | Neon Postgres connection string |
| `JWT_SECRET` | HMAC secret for JWT signing (shared with frontend) |
| `ADMIN_API_TOKEN` | Static token for admin API routes |
| `FRONTEND_ORIGIN` | Allowed CORS origin (Vercel URL) |
| `HETZNER_API_TOKEN` | Hetzner Cloud API token |
| `HETZNER_LOCATION` | Default Hetzner datacenter |

### Vercel Env Vars (frontend)

| Variable | Description |
|----------|-------------|
| `DATABASE_URL` | Neon Postgres connection string (same as backend) |
| `AUTH_SECRET` | Auth.js session encryption secret |
| `AUTH_TRUST_HOST` | `true` (required for Vercel) |
| `JWT_SECRET` | Must match backend `JWT_SECRET` |
| `NEXT_PUBLIC_API_URL` | `https://slopbox-api.fly.dev` |
| `AUTH_GITHUB_ID` | GitHub OAuth App client ID |
| `AUTH_GITHUB_SECRET` | GitHub OAuth App client secret |

## Build

```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
```

No tests yet.

## Notes

- `sprites-api` is complete but not integrated into the workspace dependency graph.
- The background monitor currently uses a `StubCollector` that produces no real metrics.
