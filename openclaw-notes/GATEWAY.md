# OpenClaw Gateway Endpoints & API Surface

Complete reference of the gateway's externally accessible endpoints, RPC methods,
and services. All served on a single multiplexed port (default `18789`).

---

## Agent ↔ Gateway ↔ External World: Communication Channels

This section maps every communication path into, out of, and through the gateway
process — how the agent reaches external services, how external services reach
the agent, and where security boundaries apply.

Ref: [OpenClaw docs — Agent Runtime](https://docs.openclaw.ai/concepts/agent),
[Agent Loop](https://docs.openclaw.ai/concepts/agent-loop),
[Sandboxing](https://docs.openclaw.ai/gateway/sandboxing),
[Tools](https://docs.openclaw.ai/tools),
[Security](https://docs.openclaw.ai/gateway/security)

### Process model recap

Everything runs in **one Node.js process** (the gateway). The agent runtime
(`pi-mono` / `pi-agent-core`), channel plugins, hooks, extensions, and the
WebSocket API all share the same event loop. The Docker sandbox is the only
component that runs in a separate process/container, connected to the gateway
via Docker IPC.

```
+================================================================+
|                    Gateway Process (Node.js)                    |
|                                                                |
|  +-----------+  +----------------+  +----------+  +----------+ |
|  | Channel   |  | Agent Runtime  |  | Tools    |  | WebSocket| |
|  | Plugins   |  | (pi-agent-core)|  | Dispatch |  | API      | |
|  | (Baileys, |  |                |  |          |  | (:18789) | |
|  |  grammY,  |  | LLM API calls ←--→ Provider |  +-----+----+ |
|  |  etc.)    |  | (outbound HTTP)|  | APIs     |  |     |      |
|  +-----+-----+  +-------+--------+  +----+-----+       |      |
|        |                |                 |             |      |
|        |    +-----------+-----------+     |             |      |
|        |    |     Tool routing      |     |             |      |
|        |    +-+--------+--------+--++     |             |      |
|        |      |        |        |  |      |             |      |
+========|======|========|========|==|======|=============|======+
         |      |        |        |  |      |             |
    Platform  Sandbox  Gateway  Node |   Browser       CLI /
    APIs      (Docker) host    host  |   (:18791+)     Web UI /
              tools    tools   tools |                 Nodes
              (exec,   (cron,        |
              read,    message,      |
              write,   web_search,   |
              edit)    web_fetch)    |
                                    |
                               External
                               APIs
```

### 1. Inbound message path (platform → agent)

The agent does NOT listen for messages. Channel plugins inside the gateway
process receive messages from platforms and route them to the agent runtime.

```
Platform (Telegram Bot API, WhatsApp Baileys WS, Discord, etc.)
    │
    ▼
Channel plugin (runs INSIDE gateway process, unsandboxed)
    │  holds credentials directly (bot tokens, session keys)
    │  receives via platform-native transport
    │
    ▼
Gateway message router
    │  resolves agent + session via bindings/routing rules
    │  applies DM policy, group allowlists, mention gating
    │
    ▼
Agent runtime (pi-agent-core, embedded in gateway process)
    │  acquires session write lock
    │  assembles system prompt (bootstrap files, skills, context)
    │  calls LLM provider API (outbound HTTPS from gateway process)
    │  receives model response with tool calls
    │
    ▼
Tool dispatch loop (see §2 below)
    │  after each tool call, checks message queue
    │  model may call more tools or produce final response
    │
    ▼
Channel plugin outbound
    │  sends reply via platform API (from gateway process)
    │
    ▼
User receives reply
```

**Key insight:** The agent runtime never directly touches the network for
inbound messages. Channel plugins and the LLM API client both run inside the
gateway process and make outbound calls on the agent's behalf.

### 2. Tool call dispatch (agent → tools)

When the model produces a tool call, `pi-agent-core` dispatches it based on
tool type and sandbox configuration. Tools fall into three execution locations:

#### Sandbox tools (Docker container, isolated)

These tools run inside a Docker container when `sandbox.mode != "off"`:

| Tool | What it does | Sandbox behavior |
|------|-------------|-----------------|
| `exec` | Shell command execution | `sh -lc` inside container; no host env; no network (`docker.network=none`) |
| `read` | Read file contents | Reads from sandbox filesystem only |
| `write` | Write file | Writes to sandbox filesystem only |
| `edit` | Edit file in place | Edits sandbox filesystem only |
| `apply_patch` | Multi-hunk file edits | Sandbox filesystem only |
| `process` | Manage background exec sessions | Manages sandbox processes |
| `browser` | CDP-controlled Chromium | Runs on gateway by default; can target sandbox with config |

**IPC mechanism:** The docs do not specify the exact IPC (likely `docker exec`
or a similar container runtime API). The gateway dispatches the tool call into
the container, the container executes it, and the result (stdout/stderr/file
contents) flows back to the gateway process. Results are sanitized for size
and image payloads before being fed back to the model.

**Network isolation:** With `docker.network=none` (the default), the sandbox
container has zero network access — no loopback, no LAN, no DNS. The agent
cannot reach the gateway's WebSocket API at `127.0.0.1:18789`, cannot call
external APIs, and cannot exfiltrate data via network.

**Filesystem isolation:** The container sees only its isolated workspace
(`~/.openclaw/sandboxes/`), optionally with the agent workspace mounted at
`/workspace` (if `workspaceAccess=rw`). No access to `~/.openclaw/credentials/`
or `~/.openclaw/openclaw.json`.

**Environment isolation:** The sandbox does NOT inherit `process.env` from the
gateway. Environment variables must be explicitly passed via
`agents.defaults.sandbox.docker.env`.

#### Gateway-process tools (unsandboxed, same process)

These tools always run inside the gateway process, regardless of sandbox config:

| Tool | What it does | Network access | External calls |
|------|-------------|---------------|----------------|
| `web_search` | Brave Search API query | **Yes** — outbound HTTPS from gateway | Brave Search API (requires API key in gateway config) |
| `web_fetch` | Fetch URL content (HTML→markdown) | **Yes** — outbound HTTPS from gateway | Arbitrary URLs; falls back to Firecrawl for JS-heavy sites |
| `message` | Cross-channel message send | **Yes** — via channel plugins | Platform APIs (Telegram, WhatsApp, Discord, etc.) |
| `cron` | Schedule/manage recurring agent runs | No external calls | Internal gateway job scheduler |
| `gateway` | Restart gateway, read/write config | No external calls | Internal gateway admin |
| `nodes` | Interact with paired companion devices | WebSocket to nodes | Node devices |
| `image` | Generate/process images | **Yes** — outbound to image API | Image generation providers |
| `canvas` | Control node Canvas surface | WebSocket to nodes | Node devices |

**These tools bypass the sandbox entirely.** They execute with full gateway
privileges — access to host env vars, credentials, network, and filesystem.
This is why `gateway`, `cron`, and `nodes` must be on the tool deny list for
Slopbox VMs.

**Critical for Slopbox:** `web_search` and `web_fetch` make outbound HTTP
requests FROM THE GATEWAY PROCESS. They are NOT affected by
`docker.network=none`. Even with full sandbox enabled, these tools can reach
the internet. This is the agent's legitimate path to external information, but
it also means the gateway's outbound IP makes requests on the agent's behalf.

#### Elevated exec (intentional sandbox escape)

When `tools.elevated.enabled=true` and the agent (or user) requests elevated
mode, `exec` runs on the gateway host instead of the sandbox. This is a
deliberate escape hatch — disabled by default, must be explicitly enabled, and
is gated by sender allowlists.

**Must be disabled for Slopbox** (`tools.elevated.enabled: false`).

### 3. LLM provider API calls (agent → model)

The `pi-agent-core` runtime makes outbound HTTPS calls to LLM provider APIs
(Anthropic, OpenAI, etc.) directly from the gateway process. This is NOT a
tool call — it's the core inference loop.

```
Agent runtime (inside gateway)
    │
    │  resolves model + auth profile
    │  (reads from agents.defaults.model config
    │   + ~/.openclaw/agents/<id>/agent/auth-profiles.json)
    │
    ▼
Outbound HTTPS to LLM provider
    │  e.g., https://api.anthropic.com/v1/messages
    │  with API key from auth profile or env var
    │
    ▼
Streaming response (SSE / chunked)
    │  pi-agent-core processes stream events
    │  emits assistant deltas to connected clients
    │
    ▼
Tool calls extracted from model response
    │  dispatched via tool routing (§2)
    │  results fed back to model for next turn
```

**Key insight for Slopbox:** LLM API keys live on the gateway host (env vars
or `auth-profiles.json`). The sandbox cannot see them. But the gateway process
needs them to function. Options for secret management:
- Inject as host env vars at provision time (simplest, safe with sandbox)
- Forward proxy injects auth headers (most secure, no keys on VPS at all)
- Short-lived tokens rotated by control plane

### 4. External service interaction summary

| Service | Who calls it | Where it runs | Network path |
|---------|-------------|---------------|-------------|
| LLM provider API (Anthropic, OpenAI) | pi-agent-core | Gateway process | Direct outbound HTTPS from VM |
| Brave Search API | `web_search` tool | Gateway process | Direct outbound HTTPS from VM |
| Arbitrary URLs | `web_fetch` tool | Gateway process | Direct outbound HTTPS from VM |
| Firecrawl (JS rendering fallback) | `web_fetch` tool | Gateway process | Direct outbound HTTPS from VM |
| Platform messaging APIs | Channel plugins | Gateway process | Direct outbound HTTPS/WS from VM |
| Image generation APIs | `image` tool | Gateway process | Direct outbound HTTPS from VM |
| Docker container runtime | Tool dispatch | Gateway → Docker IPC | Local container API |
| Companion nodes | `nodes`/`canvas` tools | Gateway process | WebSocket (LAN/Tailscale) |
| Browser (CDP) | `browser` tool | Gateway process (default) | Local CDP connection (:18791+) |
| Control plane (Slopbox) | Gateway RPC | Gateway process | Inbound WebSocket from cb-api (config.patch, /tools/invoke) |

### 5. Slopbox-specific communication topology

On a Slopbox VM, the full picture looks like this:

```
                    Internet
                       │
          ┌────────────┼────────────────┐
          │            │                │
          ▼            ▼                ▼
    LLM Provider   Platform APIs    Brave/Firecrawl
    (Anthropic,    (Telegram,       (web_search,
     OpenAI)        WhatsApp)        web_fetch)
          │            │                │
          │            │                │
+=========│============│================│================+
| VM      │            │                │                |
|         ▼            ▼                ▼                |
|  +------+------------+----------------+-------------+  |
|  |              Gateway Process                     |  |
|  |                                                  |  |
|  |  Agent Runtime ←→ Tool Dispatch                  |  |
|  |       │                │                         |  |
|  |       │          ┌─────┴─────┐                   |  |
|  |       │          ▼           ▼                   |  |
|  |       │    Gateway tools  Sandbox tools          |  |
|  |       │    (web_search,   (exec, read,           |  |
|  |       │     web_fetch,     write, edit)           |  |
|  |       │     message)          │                   |  |
|  |       │                       │ Docker IPC        |  |
|  +-------│-----------------------│------------------+  |
|          │                       ▼                     |
|          │              +------------------+           |
|          │              | Docker Sandbox   |           |
|          │              | network=none     |           |
|          │              | no host env      |           |
|          │              | isolated fs      |           |
|          │              +------------------+           |
+========================================================+
         ▲
         │  Control plane connects via WebSocket (:18789)
         │  config.patch, /tools/invoke, web.login.*
         │
    cb-api (gateway proxy or direct RPC)
```

**The agent's outbound communication channels are:**

1. **Tool calls to sandbox** — isolated, no network, no secrets
2. **Gateway-process tools** (`web_search`, `web_fetch`, `message`) — have
   full network access, run with gateway privileges. These are the agent's
   only legitimate paths to the internet.
3. **LLM API calls** — made by pi-agent-core, not by the agent directly.
   The agent cannot control which provider is called or see the API key.

**The agent CANNOT:**

- Make arbitrary HTTP requests (sandbox has no network)
- Access gateway credentials or config (sandbox filesystem is isolated)
- Call the gateway WebSocket API (sandbox has no loopback access)
- Modify gateway configuration (sandbox has no network; `gateway` tool denied)
- Start/stop channels (sandbox has no network; done via tool policy)
- Escape to the host (`tools.elevated.enabled: false`)

### 6. Control UI / Dashboard (user → gateway)

The Control UI is a Vite + Lit SPA served by the gateway at `GET /` on port
`18789`. It is the primary user-facing dashboard and communicates with the
gateway entirely via WebSocket RPC on the same port.

Ref: [OpenClaw docs — Control UI](https://docs.openclaw.ai/web/control-ui)

#### Connection model

```
User's browser
    │
    ▼
HTTPS to gateway (:18789)
    │  serves SPA static assets (Vite bundle)
    │
    ▼
WebSocket handshake (wss://<host>:18789/)
    │  connect challenge → auth (token or password) → device pairing
    │  gateway returns snapshot (presence, health, stateVersion, limits)
    │
    ▼
Full-duplex WebSocket RPC
    │  req/res pattern: { type: "req", id, method, params }
    │  event streaming:  { type: "event", event, payload }
```

#### Authentication & device pairing

- New browser profiles require **one-time device pairing approval**
- Loopback connections (`127.0.0.1`) auto-approve; remote connections require
  explicit approval via CLI (`openclaw devices approve <requestId>`)
- Auth mode is token (recommended) or password — configured via
  `gateway.auth.mode`
- Passwords are kept in memory only; tokens can be persisted in browser
- Insecure HTTP contexts block device identity unless `allowInsecureAuth`
  is set (not recommended)

#### What the UI can do

| Capability | RPC methods | Security concern for Slopbox |
|-----------|-------------|------------------------------|
| **Chat** | `chat.send`, `chat.history`, `chat.inject` | **Safe.** Primary user interaction. Messages go through agent pipeline with full sandbox + tool policy. |
| **Stream tool output** | Event subscription (`agent`, `chat` events) | **Safe.** Read-only observation of agent runs with live output cards. |
| **Abort agent runs** | `chat.abort` or `/stop` | **Safe.** Stops current run, preserves partial transcript. |
| **Session management** | `sessions.*` | **Safe.** List/reset sessions, toggle thinking/verbose. |
| **Channel status** | `channels.*` | **Caution.** Can start/stop/re-pair channels (WhatsApp QR login, etc.). May need restriction. |
| **Cron jobs** | `cron.*` | **Caution.** Can create/edit/delete/run cron jobs that trigger agent runs. Useful for automation but adds attack surface. |
| **Skills management** | `skills.*` | **Caution.** Can enable/disable/install skills, manage API keys. |
| **Config editing** | `config.*` | **DANGEROUS.** Can modify `openclaw.json` — disable sandbox, change tool policy, modify auth, alter channel credentials. **Must be blocked or restricted for end users.** |
| **Exec approvals** | `exec.approvals.*`, `exec.approval.resolve` | **DANGEROUS.** Can approve pending host exec commands and modify allowlists. **Must be blocked.** |
| **Gateway updates** | `update.run` | **DANGEROUS.** Can modify the gateway binary/package. **Must be blocked.** |
| **Log tailing** | `logs.tail` | **Caution.** Read-only but may expose sensitive data in logs. |
| **System status** | `status`, `health`, `models.list` | **Safe.** Read-only. |

#### Slopbox implications

For Slopbox, the Control UI is the user's web-based chat interface. It must be
exposed (via reverse proxy, Tailscale Serve, or direct binding) but several
RPC methods must be restricted:

**Must block:** `config.*`, `exec.approvals.*`, `exec.approval.resolve`,
`update.run` — these let the user disable sandbox, whitelist host exec,
or modify the gateway binary.

**Should restrict:** `channels.*` (start/stop/login), `skills.*`
(install/enable), `cron.*` (schedule arbitrary runs) — useful but expand
attack surface.

**Allow freely:** `chat.*`, `sessions.*`, `status`, `health`, `models.list`,
agent event streaming.

**Resolved:** OpenClaw does not support per-method RPC authorization natively.
Slopbox uses a reverse proxy (`cb-api/src/gateway_proxy.rs`) that inspects
WebSocket text frames and filters RPC methods. See the "Transport: HTTP vs
WebSocket" section above for details on how frame interception works.

---

## Transport: HTTP vs WebSocket

The gateway multiplexes both HTTP and WebSocket on a single port (default `18789`).

### HTTP endpoints (REST)

| Endpoint | Purpose | Auth |
|----------|---------|------|
| `POST /v1/chat/completions` | OpenAI-compatible chat API | Bearer token |
| `POST /tools/invoke` | Direct tool invocation (bypasses agent) | Bearer token |
| `POST /hooks/wake` | System event trigger | Hook token (separate from gateway token) |
| `POST /hooks/agent` | Isolated agent run trigger | Hook token |
| `POST /hooks/<name>` | Custom webhook endpoints | Hook token |
| `GET /` | Control UI SPA (Vite + Lit) | Gateway auth |
| `GET /__openclaw__/*` | Canvas editor, UI assets | Gateway auth |

### WebSocket endpoint

Single endpoint: `ws://<host>:18789/` (or `wss://` with TLS). All RPC
communication flows through this connection using **JSON text frames**. No
binary frames are used for RPC.

Every client request is a text frame containing:
```json
{ "type": "req", "id": "...", "method": "...", "params": {} }
```

Every gateway response is:
```json
{ "type": "res", "id": "...", "ok": true, "payload": {} }
```

Server-pushed events:
```json
{ "type": "event", "event": "...", "payload": {} }
```

The Control UI loads via HTTP (`GET /`) then upgrades to WebSocket for all
subsequent operations. Config editing, exec approvals, channel management,
and chat are all WebSocket RPC — there are no REST endpoints for these.

### TLS modes

| Mode | TLS termination | Gateway listens on |
|------|----------------|-------------------|
| **Loopback** (default) | None — plain `ws://` and `http://` | `127.0.0.1:18789` |
| **Self-signed** (auto) | Gateway generates self-signed cert | Bind address; advertises SHA-256 fingerprint via mDNS |
| **Tailscale Serve** | Tailscale terminates TLS (Let's Encrypt) | Loopback (plain HTTP); Tailscale proxies HTTPS externally |
| **Tailscale Funnel** | Tailscale terminates TLS (public internet) | Same as Serve; limited to ports 443, 8443, 10000 |
| **Reverse proxy** | Proxy terminates TLS | Loopback (plain HTTP); proxy handles certs |

The gateway supports plain HTTP/WS as the default for loopback. There is no
native ACME / Let's Encrypt integration — public TLS is delegated to Tailscale
or a reverse proxy.

### WebSocket frame interception at a reverse proxy

For Slopbox, the cb-api gateway proxy terminates TLS from users and connects
to the gateway over plain `ws://` on loopback. This makes WebSocket frame
inspection straightforward:

```
User's browser
    │
    │  HTTPS / WSS (TLS terminated here)
    ▼
cb-api gateway proxy (Rust, axum)
    │
    │  Plain ws:// over loopback — all frames visible
    ▼
OpenClaw gateway (:18789, bound to 127.0.0.1)
```

**Why this works:**

1. **JSON text frames only** — every RPC message is a parseable JSON text frame
   with a `method` field. The proxy parses each frame, extracts `method`, and
   decides whether to forward or reject.

2. **No TLS between proxy and gateway** — both run on the same VM, communicate
   over loopback. The proxy sees all frames in cleartext.

3. **Trusted-proxy auth** — the gateway supports `gateway.auth.mode: "trusted-proxy"`
   with `gateway.trustedProxies: ["127.0.0.1"]`. The proxy authenticates users
   and injects identity headers; the gateway trusts requests from whitelisted IPs.

4. **Alternatively, token injection** — with `gateway.auth.mode: "token"`, the
   proxy injects the Bearer token on behalf of the user during the WebSocket
   handshake, keeping the gateway token secret from end users. This is the
   approach cb-api currently uses.

**Filtering implementation** (already implemented in `cb-api/src/gateway_proxy.rs`):

- Parse each incoming WebSocket text frame as JSON
- If `type == "req"`, check `method` against a blocklist
- Blocked methods: `config.*`, `exec.approvals.*`, `exec.approval.resolve`, `update.run`
- For blocked methods, respond with a synthetic error frame
- Forward all other frames transparently

**Caveats:**

- Frame reassembly: WebSocket frames can be fragmented. The proxy must reassemble
  continuation frames before parsing. Most WebSocket libraries handle this
  transparently (tokio-tungstenite does).
- Binary frames: Should be forwarded without inspection (they carry non-RPC data
  like terminal output). The existing proxy already does this.
- Bidirectional: The proxy can also filter gateway-to-client events (e.g.,
  suppress `logs.tail` output) but this is not currently implemented.

### Recommended gateway config for Slopbox VMs

```json
{
  "gateway": {
    "bind": "loopback",
    "port": 18789,
    "auth": { "mode": "token" },
    "bonjour": { "mode": "off" },
    "controlUi": { "enabled": false }
  }
}
```

The Control UI is disabled because users access the agent through the Slopbox
web app (which proxies WebSocket RPC through cb-api with method filtering).
The gateway token is injected by the control plane at provision time and never
exposed to users.

---

## Port & Binding

| Setting | Default | Notes |
|---------|---------|-------|
| Port | `18789` | Override: `--port` flag > `OPENCLAW_GATEWAY_PORT` env > `gateway.port` config |
| Bind | `loopback` | Options: `loopback`, `lan`, `tailnet`, `custom` |
| TLS | Auto (self-signed) | Control UI served over HTTPS when enabled |

Non-loopback binding requires `gateway.auth` to be configured.

---

## Authentication

All endpoints (HTTP and WebSocket) require authentication when configured.
Loopback connections may auto-approve for device pairing but still need auth tokens.

| Mode | Config | How it works |
|------|--------|-------------|
| **Token** (recommended) | `gateway.auth.mode: "token"` | Bearer token in `Authorization` header or WS connect params |
| **Password** | `gateway.auth.mode: "password"` | Password in connect params (not persisted by UI) |
| **Trusted proxy** | `gateway.auth.mode: "trusted-proxy"` | Reverse proxy injects identity header; gateway validates source IP |

Env vars: `OPENCLAW_GATEWAY_TOKEN`, `OPENCLAW_GATEWAY_PASSWORD`

---

## HTTP Endpoints

### `POST /v1/chat/completions` — OpenAI-compatible API

- **Auth**: Bearer token
- **Purpose**: Send chat messages, receive agent responses
- **Execution model**: Runs through the standard agent pipeline (tools, sandbox, policies all apply)
- **Agent routing**: Via `model` field (`"openclaw:<agentId>"`) or `x-openclaw-agent-id` header
- **Session**: Stateless by default (new session per request); persistent via `user` field
- **Streaming**: SSE with `stream: true`
- **Security**: Same sandboxing/tool policy as any other agent run. No direct host access.

### `POST /tools/invoke` — Direct tool invocation

- **Auth**: Bearer token
- **Purpose**: Invoke any tool directly, bypassing the agent entirely
- **Available tools**: All tools not on the deny list
- **Default deny list**: `sessions_spawn`, `sessions_send`, `gateway`, `whatsapp_login`
- **NOT denied by default**: `exec`, `read`, `write`, `edit`, `apply_patch`, `process`, `browser`, `web_search`, `web_fetch`, `memory_*`, `message`, `cron`, `image`
- **Policy**: Filtered through tool policy chain (profile, allow/deny, agent-specific rules). Returns 404 if tool not allowed.
- **SECURITY CRITICAL**: This endpoint allows direct shell execution and file access on the host (or sandbox, depending on config). Anyone with the auth token can run arbitrary commands.

### `POST /hooks/wake` — System event trigger

- **Auth**: Hook token (`Authorization: Bearer <hook-token>` or `x-openclaw-token`)
- **Purpose**: Enqueue a system event for the main session
- **Payload**: `{ "text": "...", "mode": "now" | "next-heartbeat" }`
- **Response**: `200 OK`
- **Security**: Requires dedicated hook token (separate from gateway auth token)

### `POST /hooks/agent` — Isolated agent run

- **Auth**: Hook token
- **Purpose**: Trigger an agent turn with optional delivery to chat channels
- **Payload**: `{ "message": "...", "agentId": "...", "channel": "...", "to": "...", "deliver": true, ... }`
- **Response**: `202 Accepted` (async)
- **Security**: `allowedAgentIds` restricts which agents can be triggered. `allowRequestSessionKey` disabled by default.

### `POST /hooks/<name>` — Custom webhook endpoints

- **Auth**: Hook token
- **Purpose**: Custom-mapped endpoints defined in config
- **Security**: Same hook token auth as wake/agent

### `GET /` — Control UI

- **Auth**: Gateway auth (token/password/trusted-proxy)
- **Purpose**: Vite + Lit SPA for chat, config, sessions, channels, cron, logs
- **Base path**: Configurable via `gateway.controlUi.basePath`
- **Communication**: Uses WebSocket on same port for all RPC
- **Device pairing**: New browser profiles require approval (auto-approved on loopback)

### Static assets

- `/__openclaw__/*` — Canvas editor, UI assets
- Standard Vite SPA routing for Control UI paths

---

## WebSocket API

Connected via `ws://<host>:18789/` (or `wss://` with TLS).

### Connection handshake

```
1. Gateway  → Client:  { type: "connect.challenge", nonce: "..." }
2. Client   → Gateway: { type: "req", method: "connect", params: {
                           role: "operator" | "node",
                           scopes: [...],
                           device: { id, name, ... },
                           auth: { token: "..." },
                           signedNonce: "..."
                         }}
3. Gateway  → Client:  { type: "res", ok: true, payload: {
                           snapshot: { presence, health, stateVersion, uptimeMs, limits }
                         }}
```

### Request/Response pattern

```
Client  → Gateway: { type: "req", id: "...", method: "...", params: {...} }
Gateway → Client:  { type: "res", id: "...", ok: true|false, payload|error }
```

Idempotency keys required for side-effecting methods (`send`, `agent`).

### Event streaming

```
Gateway → Client: { type: "event", event: "...", payload: {...} }
```

Event types: `agent`, `chat`, `presence`, `tick`, `health`, `heartbeat`, `shutdown`

---

## WebSocket RPC Methods

### Agent execution

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `agent` | Trigger an agent run. Returns `runId` immediately. | Runs through full agent pipeline with sandbox/tools policy |
| `agent.wait` | Wait for agent run completion. Polls lifecycle events. | Read-only observation of a run |

### Chat operations

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `chat.history` | Retrieve chat transcript for a session | Read-only |
| `chat.send` | Send a message into a session | Triggers agent processing |
| `chat.inject` | Add assistant note to transcript without triggering agent | Write to session transcript |

### Channel management

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `channels.*` | Channel status, start, stop, QR login | Can start/stop channel connections |

### Session management

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `sessions.*` | List, configure, reset sessions | Can modify session state |
| `sessions.spawn` | Create sub-agent sessions | On tools-invoke deny list |
| `sessions.send` | Cross-session messaging | On tools-invoke deny list |

### Configuration

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `config.*` | Read/write `openclaw.json` config | **CAN MODIFY GATEWAY CONFIG** including sandbox settings, tool policies, auth |

### Execution control

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `exec.approval.resolve` | Approve/deny pending exec requests | Grants host execution |
| `exec.approvals.*` | Manage exec allowlists | Modifies security policy |

### Device management

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `device.token.rotate` | Refresh device auth token | |
| `device.token.revoke` | Invalidate device token | |
| `system-presence` | List connected devices | |

### Infrastructure

| Method | Purpose | Security notes |
|--------|---------|---------------|
| `status` | Gateway status | Read-only |
| `health` | Health check with channel probes | Read-only |
| `models.list` | Available LLM models | Read-only |
| `logs.tail` | Live log streaming | Read-only but may expose sensitive data |
| `update.run` | Update gateway package/binary | **Modifies gateway installation** |
| `skills.*` | Manage skills (enable/disable/install) | Can modify agent capabilities |
| `skills.bins` | Fetch skill executables | |
| `cron.*` | List, create, execute, enable/disable cron jobs | Can schedule arbitrary agent runs |
| `node.list` | List paired nodes | |

---

## mDNS/Bonjour Discovery

Service type: `_openclaw-gw._tcp`

Published TXT records (unauthenticated, treat as UX hints only):
- `role=gateway`
- `lanHost=<hostname>.local`
- `gatewayPort=18789`
- `sshPort=22`
- `gatewayTls=1`, `gatewayTlsSha256=<hash>` (when TLS enabled)
- `canvasPort=<port>`
- Optional: `cliPath`, `tailnetDns`

Disable: `gateway.bonjour.mode: "off"` or `OPENCLAW_DISABLE_BONJOUR=1`

---

## Slopbox Security Analysis

### Endpoints that grant direct host access

| Endpoint / Method | Risk | Our mitigation |
|-------------------|------|----------------|
| `POST /tools/invoke` (exec) | Direct shell execution on host | `network=none` in sandbox; auth token required |
| `config.*` RPC | Can disable sandbox, change tool policy, modify auth | `network=none`; auth token required |
| `exec.approval.resolve` RPC | Can approve pending exec on host | `network=none`; auth token required |
| `update.run` RPC | Can modify gateway binary/package | `network=none`; auth token required |
| `exec.approvals.*` RPC | Can add commands to exec allowlist | `network=none`; auth token required |
| `cron.*` RPC | Can schedule agent runs (which use tools) | `network=none`; auth token required |
| Elevated exec (via agent) | Host exec escape hatch | `tools.elevated.enabled=false` |

### Endpoints that are safe (agent pipeline)

| Endpoint | Why safe |
|----------|---------|
| `POST /v1/chat/completions` | Goes through agent pipeline; sandbox + tool policy apply |
| `POST /hooks/agent` | Same — triggers agent run with standard policies |
| `chat.send` RPC | Same — triggers agent processing |

### Endpoints we should disable/restrict for Slopbox

For maximum security on Slopbox VPS images:

```json5
{
  "gateway": {
    "bind": "loopback",
    "auth": { "mode": "token" },
    // Disable mDNS advertisement
    "bonjour": { "mode": "off" },
    // Disable Control UI (no browser access needed on headless VPS)
    "controlUi": { "enabled": false }
  },
  "hooks": {
    // Disable webhook endpoints (we use our own channel, not hooks)
    "enabled": false
  }
}
```

### The network=none guarantee

With `sandbox.docker.network=none`, the Docker container has:
- No loopback access to `127.0.0.1:18789` (gateway)
- No LAN/internet access
- No DNS resolution
- No ability to call any HTTP/WebSocket endpoint

The agent's ONLY communication path is through the OpenClaw channel system —
specifically our slopbox channel extension running in the unsandboxed gateway
process. This extension is the sole bridge between the sandboxed agent and the
outside world.
