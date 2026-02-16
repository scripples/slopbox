# Slopbox — Product Requirements Document

> Updated 2026-02-16

---

## 1. Product Overview

**Product:** Slopbox
**Tagline:** "Your AI agent, sandboxed and ready."

Slopbox is a SaaS platform that gives users their own personal AI agent running
in an isolated cloud VM. Each agent is a full OpenClaw instance running in a
dedicated virtual machine with Docker-based sandbox isolation. Users configure
their agent's personality and channels, then interact via messaging platforms
(WhatsApp, Telegram, Discord, Slack) or the web UI.

**Key differentiator:** True VM-level isolation per agent. The agent's tool
execution (shell, filesystem) runs inside a Docker sandbox with no network,
no access to gateway credentials, and no access to the host environment. Channel
credentials never enter the sandbox. Resource usage (CPU, RAM, bandwidth) is
metered and enforced per plan.

**Business model:** Subscription plans with resource-based tiers. Plans define
max agents, max VPSes, and monthly caps for bandwidth, CPU, RAM, and storage.
Overages billed at per-unit rates with user-configurable budget caps.

---

## 2. Target Users

| Persona | Description |
|---------|-------------|
| **Casual user** | Wants a personal AI assistant on WhatsApp/Telegram for conversation, reminders, quick tasks |
| **Power user** | Multi-channel assistant with memory, tools, scheduled tasks, and workspace |

---

## 3. Core User Journey

```
Landing Page → Sign Up → Onboarding → Dashboard → Chat (via gateway proxy or channel)
```

1. User visits landing page, sees features and pricing
2. Signs up via Auth.js (Google, GitHub OAuth or email/password)
3. Onboarding flow:
   - **Step 1:** Name the agent and pick personality (preset or custom)
   - **Step 2:** Select plan (determines resource limits + VPS config options)
   - **Step 3:** System provisions a VM (~30-90s depending on provider)
   - **Step 4:** System writes OpenClaw config, starts gateway, health check
   - **Step 5:** Connect channels (WhatsApp QR, Telegram bot token, etc.) — or skip
   - **Step 6:** Done — open web chat or go to dashboard
4. User interacts with agent via WhatsApp, Telegram, web UI (proxied), or other channels
5. Dashboard shows agent status, resource usage, and settings

---

## 4. Feature Requirements

### 4.1 Authentication

| Requirement | Details |
|-------------|---------|
| Sign up | OAuth providers (Google, GitHub) via Auth.js, or email/password |
| Auth provider | Auth.js (NextAuth) — manages OAuth flows and sessions |
| Session store | PostgreSQL `sessions` table (Auth.js managed) |
| API auth (BFF) | Frontend calls cb-api with `Authorization: Bearer <CONTROL_PLANE_API_KEY>` + `X-User-Id` header. The frontend is the trusted intermediary. |
| Gateway proxy auth | Auth.js session cookie (`authjs.session-token`) — user never sees gateway token |

Auth.js handles identity (OAuth, email/password) and writes to PostgreSQL
tables (`accounts`, `sessions`, `verification_tokens`). The `users` table is
shared — Auth.js extended it with `email_verified` and `image` columns. The
Rust backend reads Auth.js sessions directly from PostgreSQL for gateway proxy
auth, and uses the BFF pattern (shared API key + trusted `X-User-Id`) for
control plane API routes called by the frontend.

### 4.2 Agent Management

Each user can have multiple agents (limited by plan's `max_agents`).

| Field | Type | Purpose |
|-------|------|---------|
| `name` | string | User-chosen agent name |
| `gateway_token` | string | 64-char hex token for OpenClaw gateway auth |
| `vps_id` | UUID? | Linked VPS (nullable — agent can exist without VM) |
| `user_id` | UUID | Owning user |

### 4.3 VM Lifecycle

Each agent gets a dedicated VM provisioned via the VPS provider (Hetzner or Fly.io).

```
create agent → provision VM → write config → start gateway → health check → ready
```

**Provisioning steps:**
1. Create VM via provider API (Hetzner Cloud or Fly Machines)
2. VM boots from pre-built image with OpenClaw + Docker pre-installed
3. Control plane writes `~/.openclaw/openclaw.json` with locked-down config
4. Control plane writes channel credentials to `~/.openclaw/credentials/`
5. Control plane writes workspace files (AGENTS.md, SOUL.md, etc.)
6. Start OpenClaw gateway process
7. Health check gateway endpoint (poll with retries)
8. Mark VPS as `running` in DB

**VPS states:** `provisioning → running ⇄ stopped → destroyed`

**VPS config** defines the resource tier (CPU millicores, memory MB, disk GB)
tied to a specific provider and VM image. Plans have a set of allowed VPS configs.

### 4.4 OpenClaw Configuration

Each VM runs OpenClaw with a locked-down config. See `openclaw-notes/README.md`
for full security analysis.

| Setting | Value | Purpose |
|---------|-------|---------|
| `sandbox.mode` | `all` | All tool execution in Docker container |
| `sandbox.workspaceAccess` | `rw` | Agent can read/write workspace, nothing else |
| `sandbox.docker.network` | `none` | No network from sandbox |
| `tools.deny` | `["gateway", "nodes"]` | Block dangerous tools |
| `tools.elevated.enabled` | `false` | No host exec escape |
| `gateway.bind` | `0.0.0.0:18789` | Accessible from control plane (firewalled) for gateway proxy |
| `gateway.auth.mode` | `token` | Token auth required |
| `hooks.enabled` | `true` | Automation webhooks available |

**LLM API keys** are injected as host environment variables at provision time.
The Docker sandbox does not inherit host `process.env`, so the agent's tool
calls cannot read them. The gateway process (unsandboxed) reads them normally.

### 4.5 Messaging Channels

Channels connect natively via OpenClaw's built-in channel plugins. No proxy
relay — the gateway on the VM talks directly to platform APIs. Channel
credentials live on the VM in `~/.openclaw/credentials/`, inaccessible to the
sandboxed agent.

| Channel | Status | Transport |
|---------|--------|-----------|
| WhatsApp | Primary | Baileys (WhatsApp Web protocol) |
| Telegram | Primary | grammY (Bot API) |
| Discord | Planned | OpenClaw built-in |
| Slack | Planned | OpenClaw built-in |
| Signal | Planned | OpenClaw built-in |
| Web Chat | Built-in | OpenClaw Control UI (proxied through our app) |

Channel credentials are written to the VM by the control plane at setup time
or via a config update endpoint.

### 4.6 Gateway Proxy

Users access the OpenClaw web UI (Control UI) through our application. The
gateway token and VM address are never exposed to the browser.

**Flow:**
```
User browser
  → our-app.com/agents/{id}/gateway/* (Auth.js session cookie)
  → cb-api gateway proxy:
    1. Validate session cookie against PG sessions table
    2. Look up agent + VPS for user (ownership check)
    3. Proxy HTTP/WebSocket to VM's gateway (inject gateway token + HMAC nonce)
    4. Filter dangerous WebSocket RPC methods (config.*, exec.approvals.*, update.run)
    5. Block POST /tools/invoke at HTTP layer
    6. Stream response back (with bandwidth tracking)
  → User sees filtered OpenClaw Control UI
```

**What this enables:**
- Gateway token stays server-side (never in browser)
- VM address stays server-side (never exposed)
- Session-based auth (existing login)
- Can add rate limiting, CSP headers at our edge
- Revoke access instantly by stopping VM

**RPC method filtering:** The gateway proxy blocks dangerous RPC methods
(`config.*`, `exec.approvals.*`, `exec.approval.resolve`, `update.run`) by
parsing WebSocket JSON text frames and checking the `method` field. Blocked
methods get a synthetic JSON-RPC error response. `POST /tools/invoke` is
blocked at the HTTP layer. See `openclaw-notes/GATEWAY.md` §Transport for
details on frame interception.

### 4.7 Dashboard

**Tabs:** Overview, Usage, Channels, Settings

**Overview:**
- Agent card with status indicator (running/stopped/provisioning/error)
- Action buttons (Open Chat, Start, Stop, Destroy)
- Quick stats: VPS state, uptime, connected channels

**Usage:**
- Current month resource usage vs plan limits
- Bandwidth (bytes in/out)
- CPU time (milliseconds)
- Memory (MB-seconds)
- Storage (bytes)
- Overage cost and budget

**Channels:**
- Connected channels with status
- Add/remove channels (credentials written to VM)
- WhatsApp QR code pairing flow

**Settings:**
- Agent personality (AGENTS.md, SOUL.md, IDENTITY.md)
- Model selection
- Overage budget cap

### 4.8 Admin Panel

| Feature | Details |
|---------|---------|
| User list | All users with plan, agent count, usage |
| VM management | View all VPSes, force stop/destroy on overage |
| Plan management | CRUD on plans and VPS configs |
| Usage overview | Aggregate resource usage across all users |
| Overage enforcement | Automatic stop on overage; manual override |

### 4.9 Billing & Resource Metering

**Plan structure:**

| Field | Purpose |
|-------|---------|
| `max_agents` | Max agents per user |
| `max_vpses` | Max running VMs per user |
| `max_bandwidth_bytes` | Monthly bandwidth cap |
| `max_storage_bytes` | Storage cap |
| `max_cpu_ms` | Monthly CPU time cap |
| `max_memory_mb_seconds` | Monthly RAM usage cap |
| `overage_*_cost_per_*_cents` | Per-unit overage rates |

**Usage tracking:** `vps_usage_periods` table accumulates per-VPS, per-month
counters for bandwidth, CPU, and memory. Aggregated across all user VPSes for
plan limit checks.

**Overage handling:**
1. Background monitor in cb-api polls provider metrics APIs for resource usage
2. Background monitor compares usage to plan limits
3. If usage exceeds plan limits AND overage cost exceeds user's overage budget:
   - Stop the VPS
   - Notify user
4. User can increase overage budget via API/dashboard to resume

**Not yet implemented:** Stripe integration, subscription management, automatic
billing.

---

## 5. Data Model

### 5.1 Tables

**PostgreSQL (all data — managed by cb-api and Auth.js):**
```
plans               — Subscription plans (resource limits + overage rates)
vps_configs         — VM resource tier definitions (CPU, RAM, disk, provider, image)
plan_vps_configs    — Join table: which VPS configs are available on which plans
users               — User accounts (email, name, plan_id, email_verified, image). Shared between Auth.js and cb-api.
accounts            — OAuth provider accounts (Auth.js managed, read-only from Rust)
sessions            — Active auth sessions (Auth.js managed, read-only from Rust)
verification_tokens — Email verification tokens (Auth.js managed)
vpses               — VM instances (state, provider refs, usage counters)
agents              — AI agents (name, gateway_token, linked VPS)
agent_channels      — Channel credentials per agent (kind, credentials JSONB, webhook_secret)
vps_usage_periods   — Monthly resource usage counters per VPS
overage_budgets     — Per-user monthly overage spending caps
```

Auth.js manages `accounts`, `sessions`, and `verification_tokens`. The `users`
table is shared — Auth.js creates users and manages `email_verified` + `image`
columns; cb-api manages `plan_id` and reads the rest.

### 5.2 Key Relationships

```
users         1 ←→ N  agents
users         1 ←→ N  accounts          (Auth.js OAuth credentials)
users         1 ←→ N  sessions          (Auth.js sessions)
agents        1 ←→ 0..1  vpses          (agent may or may not have a VM)
agents        1 ←→ N  agent_channels
vpses         1 ←→ N  vps_usage_periods
users         1 ←→ N  overage_budgets
plans         N ←→ N  vps_configs       (via plan_vps_configs)
users         N ←→ 1  plans             (optional)
```

---

## 6. Architecture

### 6.1 Technology Stack

| Layer | Technology |
|-------|-----------|
| Frontend | Not yet built (planned: React/Next.js SPA) |
| Auth | Auth.js (NextAuth) — OAuth + email/password, sessions in PostgreSQL |
| Control plane API | Rust (axum 0.8) — cb-api crate |
| Database | PostgreSQL (sqlx 0.8, compile-time-unchecked queries) — all tables |
| VPS Providers | Hetzner Cloud (hcloud 0.25), Fly.io Machines (fly-api crate) |
| Agent Runtime | OpenClaw (Node.js, runs inside VM) |
| Sandbox | Docker (hypervisor + virtio-fs) inside VM |
| Deployment | TBD (control plane) + Hetzner/Fly (VMs) |

### 6.2 Crate Structure

| Crate | Purpose |
|-------|---------|
| `cb-api` | Control plane HTTP API (axum). Agent/VPS lifecycle, usage checks, background monitor, gateway proxy. |
| `cb-db` | Database models + migrations (sqlx). Plan, VpsConfig, User, Vps, Agent, AgentChannel, etc. |
| `cb-infra` | VpsProvider trait + Hetzner/Fly implementations. Provider registry. |
| `fly-api` | Typed Fly.io Machines REST API client. |
| `sprites-api` | Typed Sprites API client (sprites CRUD, exec, checkpoints). |

### 6.3 Control Plane API Endpoints

**Authenticated (BFF pattern: `Authorization: Bearer <CONTROL_PLANE_API_KEY>` + `X-User-Id`):**

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/agents` | Create agent |
| GET | `/agents` | List user's agents |
| GET | `/agents/{id}` | Get agent details |
| DELETE | `/agents/{id}` | Delete agent |
| POST | `/agents/{id}/vps` | Provision VM for agent |
| DELETE | `/agents/{id}/vps` | Destroy agent's VM |
| POST | `/agents/{id}/vps/start` | Start stopped VM |
| POST | `/agents/{id}/vps/stop` | Stop running VM |
| POST | `/agents/{id}/channels` | Add channel to agent |
| GET | `/agents/{id}/channels` | List agent's channels |
| DELETE | `/agents/{id}/channels/{kind}` | Remove channel |
| GET | `/agents/{id}/usage` | Get current usage vs limits |
| GET | `/users/me` | Get current user + plan |
| GET | `/users/me/overage-budget` | Get overage budget |
| PUT | `/users/me/overage-budget` | Set overage budget |
| GET | `/plans` | List available plans |

**Gateway proxy (Auth.js session cookie):**

| Method | Path | Purpose |
|--------|------|---------|
| ANY | `/agents/{agent_id}/gateway/{*path}` | Proxy HTTP to OpenClaw gateway on VM |
| GET/WS | `/agents/{agent_id}/gateway/ws` | Proxy WebSocket with handshake interception + RPC method filtering |

Provider metrics are fetched by the background monitor directly from
Fly Prometheus API / Hetzner REST API — no inbound endpoint needed.

### 6.4 Message Flow

```
User (WhatsApp/Telegram)
    |
    v
Platform API (native transport)
    |
    v
OpenClaw Gateway (on VM, holds credentials in ~/.openclaw/credentials/)
    |  channel plugin receives message
    |  routes to agent session
    |
    v
Agent runtime (embedded in gateway process)
    |  LLM API call (keys in host env, invisible to sandbox)
    |  tool calls dispatched to Docker sandbox
    |
    v
Agent response
    |
    v
Channel plugin outbound (gateway process, native platform API)
    |
    v
User receives reply
```

No intermediary proxy for channel messages. The gateway talks to platforms
directly.

### 6.5 Resource Metering Flow

Resource metering uses **provider metrics APIs** — external to the VM and
tamper-proof. The background monitor polls these APIs and accumulates usage.

```
Provider Metrics APIs (external, tamper-proof)

Fly: Prometheus API
  - CPU, RAM, disk, network (17 metrics)
Hetzner: GET /servers/{id}/metrics
  - CPU, disk, network
  - ⚠ NO RAM metrics (use fixed allocation from VPS config tier)

        |
        v
+-------------------------------------------------------+
|              Control Plane (cb-api)                    |
|                                                        |
|  Background monitor (spawn_monitor):                   |
|    - Polls provider metrics per running VPS            |
|    - Updates vps_usage_periods table                   |
|    - Checks against plan limits                        |
|    - Hetzner RAM: estimated from VPS config tier       |
+-------------------------------------------------------+
        |
        v  (if usage exceeds limits AND overage > budget)
        |
        v
Stop VM via provider API → notify user
```

---

## 7. VM Image & Deployment

### 7.1 Pre-built VM Image Contents

The VM image is built once and used for all provisioned VMs. It contains:

- Linux base (Debian/Ubuntu minimal)
- Docker engine (for OpenClaw sandbox)
- OpenClaw (pre-installed via npm)

### 7.2 Per-VM Configuration (written at provision time)

| File | Contents | Written by |
|------|----------|-----------|
| `~/.openclaw/openclaw.json` | Gateway + sandbox + tool policy config | Control plane |
| `~/.openclaw/credentials/` | Channel credentials (Telegram bot token, etc.) | Control plane |
| `~/.openclaw/workspace/AGENTS.md` | Agent personality + instructions | Control plane |
| `~/.openclaw/workspace/SOUL.md` | Persona and boundaries | Control plane |
| `~/.openclaw/workspace/IDENTITY.md` | Agent name and emoji | Control plane |
| `/etc/openclaw-env` or host env | LLM API keys, gateway token | Control plane (env vars at provision) |

### 7.3 Security Lockdown Summary

| Layer | Mechanism | What it prevents |
|-------|-----------|-----------------|
| Docker sandbox (`mode=all`) | Agent tools run in container | Access to host fs, env vars, gateway config |
| No sandbox network (`network=none`) | Container has no network stack | Calling gateway API, bypassing sandbox via `/tools/invoke` |
| Tool policy deny | `["gateway", "nodes"]` | Agent restarting/reconfiguring gateway |
| Elevated disabled | `tools.elevated.enabled=false` | Host exec escape hatch |
| Gateway bind | `bind=0.0.0.0:18789` (firewalled, control plane only) | Direct external access to gateway API |
| Gateway auth | `auth.mode=token` | Unauthorized access even on loopback |
| Gateway proxy (our app) | Filters `config.*`, `update.run`, `exec.approvals.*` | User modifying security settings via Control UI |

---

## 8. Resource Monitoring Service

Resource metering uses **provider metrics APIs** — external to the VM and
tamper-proof. The background monitor polls these APIs and accumulates usage
into the `vps_usage_periods` table for billing and enforcement.

### 8.1 Design Principles

1. **Tamper resistance:** Provider metrics come from outside the VM. Even if the
   agent compromises the entire VM, provider metrics are unaffected.
2. **Simplicity:** No additional processes on the VM. The gateway is the only
   running service. Metrics are collected externally.
3. **Graceful degradation:** If the provider API is unavailable, alert but don't
   stop the VPS (avoid false positives).

### 8.2 Provider Metrics APIs

| Provider | CPU | RAM | Disk | Network | API |
|----------|-----|-----|------|---------|-----|
| **Fly.io** | `fly_instance_cpu` (17 metrics) | `fly_instance_memory_*` (rss, cache, swap, etc.) | `fly_instance_disk_*` | `fly_instance_net_*` (sent/recv bytes + packets) | Prometheus: `GET https://api.fly.io/prometheus/<org>/api/v1/query` |
| **Hetzner** | `cpu` (per-core utilization) | **Not available** | `disk` (IOPS + bandwidth) | `network` (bandwidth + packets per interface) | REST: `GET /servers/{id}/metrics?type=cpu,disk,network&start=...&end=...` |

**Fly.io** provides complete external coverage — all four metric categories are
available via the Prometheus API with standard PromQL queries.

**Hetzner** is missing RAM metrics. For Hetzner VMs, RAM usage is estimated from
the VPS config tier (fixed allocation). This is acceptable because Hetzner VMs
have dedicated RAM — the VM always uses its full allocation.

### 8.3 Control Plane Enforcement

The background monitor in cb-api (`spawn_monitor`) runs the collection loop:

1. List all running VPSes
2. For each VPS, fetch provider metrics (Fly Prometheus / Hetzner REST)
3. Accumulate into `vps_usage_periods` table
4. Aggregate across all user VPSes, compare to plan limits
5. If usage exceeds limits AND overage cost exceeds user's overage budget:
   - Stop the VPS via provider API
   - Notify user (email / dashboard alert)
6. User can increase overage budget via API/dashboard to resume

**Edge cases:**

| Situation | Behavior |
|-----------|----------|
| Provider API unavailable | Alert admin. Do NOT stop VPS (avoid false positives). |
| Provider API returns stale data | Use latest available. Flag for investigation. |

---

## 9. Implementation Status

### Done (backend)

- PostgreSQL schema: plans, vps_configs, users, vpses, agents, agent_channels,
  vps_usage_periods, overage_budgets, Auth.js tables (12 migrations)
- Control plane API: agent CRUD, VPS lifecycle (provision/start/stop/destroy),
  channel CRUD, usage checks, overage budget management
- VPS provider abstraction: VpsProvider trait with Hetzner + Fly implementations
- Provider registry: `build_providers()` constructs available providers at startup
- Forward proxy: Basic-auth proxy (agent_id:gateway_token) for outbound traffic
  with bandwidth tracking and per-request usage enforcement (Fly)
- Gateway proxy: HTTP + WebSocket proxy with session cookie auth, gateway token
  injection, HMAC nonce signing, RPC method filtering, `/tools/invoke` blocking
- Auth middleware: BFF pattern (CONTROL_PLANE_API_KEY + X-User-Id) for API routes,
  Auth.js session cookies for gateway proxy routes
- OpenClaw config generation: locked-down sandbox + tool policy + gateway config
- Database models with all CRUD operations
- Workspace file updates via gateway HTTP `/tools/invoke` endpoint
- Agent health checks via gateway HTTP endpoint

### Done (architecture decisions)

- OpenClaw runs natively on VM with Docker sandbox
- Channel credentials on VM, protected by sandbox isolation
- LLM API keys on VM as host env vars, invisible to sandbox
- Control UI proxied through cb-api (gateway token hidden from browser)
- WebSocket RPC method filtering at proxy layer (config.*, exec.approvals.*, update.run blocked)
- Resource-based billing with overage budgets
- Auth.js for auth (OAuth + sessions in PostgreSQL)
- Backend in `backend/` subdirectory

### Partially Done

- **Background monitor:** Enforcement logic complete (detects overage, stops
  Hetzner VPSes, checks overage budgets). Metrics collection uses StubCollector
  (returns existing DB values). **Missing: real provider API integration.**

### Not Started

- **Frontend:** Web app (landing, auth, onboarding, dashboard, admin). No
  framework chosen yet — Auth.js is in the backend, frontend needs to integrate.
- **Onboarding flow:** Wizard that creates agent, provisions VM, writes config
- **Channel setup UI:** WhatsApp QR pairing (via gateway `web.login.*` RPC),
  Telegram bot token input
- **VM image:** Pre-built image with OpenClaw + Docker (Packer or similar)
- **Provider metrics integration:** Fly Prometheus API + Hetzner REST metrics
  API polling — needs a real `MetricsCollector` implementation to replace
  `StubCollector`
- **Gateway RPC client:** WebSocket client in cb-api for `config.patch`,
  `config.apply` (control plane writes config to running VMs via gateway-native
  RPC). Currently `update_config` and `restart_agent` return 501.
- **Stripe integration:** Subscription billing
- **Admin panel:** User/VM management, overage enforcement UI
- **Notifications:** Email/dashboard alerts for overage, VPS status changes
- **VM pool:** Pre-provisioned VMs for fast onboarding
- **Automated tests:** No test files in the workspace

---

## 10. Key Risks & Technical Debt

| Risk | Severity | Notes |
|------|----------|-------|
| Docker-in-VM feasibility (Fly) | High | Fly Machines are Firecracker microVMs — Docker sandbox may need investigation. Hetzner VMs work natively. |
| Provider metrics not implemented | High | StubCollector in use. Provider API integration (Fly Prometheus, Hetzner REST) not built. Hetzner lacks RAM metrics (use fixed allocation from VPS tier). |
| Gateway RPC client not implemented | High | `update_config` and `restart_agent` return 501. Control plane cannot write config to running VMs until a WebSocket RPC client is built for `config.patch` / `config.apply`. |
| No payment integration | High | Revenue model not functional without Stripe. |
| BFF auth model | Medium | API routes use shared `CONTROL_PLANE_API_KEY` + trusted `X-User-Id`. This is safe when the frontend is the only caller, but the API key is a single point of compromise. Consider per-user tokens or JWT. |
| Gateway token in forward proxy | Medium | Forward proxy uses Basic auth with gateway_token. Token is on the VM but sandbox-protected. |
| No automated tests | Medium | No test files in the workspace. |
| WhatsApp pairing UX | Medium | Baileys requires QR scan — needs real-time WebSocket from VM to frontend via gateway proxy (`web.login.*` RPC). |
| No frontend | High | No web UI exists. Backend API is functional but unusable without a frontend. |
