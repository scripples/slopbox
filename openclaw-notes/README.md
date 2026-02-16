# OpenClaw Architecture Notes

Reference notes for Slopbox integration. Focused on component relationships,
communication models, and security boundaries.

**See also:** [PRD.md](PRD.md) for the full Slopbox product requirements document,
and [GATEWAY.md](GATEWAY.md) for the complete OpenClaw gateway endpoint reference.

---

## Component Overview

| Component | Description |
|-----------|-------------|
| **Gateway** | Central long-lived Node.js daemon. Owns all channel connections, runs the embedded agent runtime, exposes a typed WebSocket control API (default `ws://127.0.0.1:18789`), and serves the web UI. One gateway per host. |
| **Agent** | Embedded AI runtime (`pi-mono`-derived) running *inside* the gateway process. Processes messages in a serialized per-session loop, calling tools to interact with the world. Not a separate process. |
| **Channels** | Plugin extensions that connect the gateway to messaging platforms (WhatsApp via Baileys, Telegram via grammY, Discord, Slack, Signal, iMessage, etc.). Run inside the gateway process. |
| **CLI** | The `openclaw` command-line tool. Used to start/stop the gateway, manage configuration, send messages, and administer sandbox containers. Separate process from the gateway. |
| **Nodes** | Mobile (iOS/Android) or headless companion devices. Connect to the gateway via WebSocket with device-based pairing. Provide cameras, microphones, location, and remote exec. |
| **Tools** | Functions the agent can call during a turn: `exec` (shell), `read`/`write`/`edit` (filesystem), `browser`, `web_search`/`web_fetch`, `message` (cross-channel), `cron`, etc. |
| **Hooks** | Internal event-driven TypeScript automation scripts (NOT HTTP endpoints). Triggered by agent commands, lifecycle events, and gateway startup. Run inside the gateway process. |
| **Extensions** | npm-packaged plugins discovered from `extensions/` directories. Can register channels, tools, hooks, HTTP routes, and gateway methods via `OpenClawPluginApi`. |
| **Control UI** | Browser-based dashboard served by the gateway on the same port. Chat, config editing, session management. |

---

## Process Model

```
                    +-----------------------------------------+
                    |           Gateway Process               |
                    |                                         |
                    |  +----------+   +-------------------+   |
                    |  | Channels |   | Agent Runtime     |   |
                    |  | (Baileys,|   | (pi-mono)         |   |
                    |  |  grammY, |   |                   |   |
                    |  |  etc.)   |   | Tool dispatch:    |   |
                    |  +----+-----+   | exec, read, write |   |
                    |       |         | edit, browser...  |   |
                    |       |         +--------+----------+   |
                    |       |                  |              |
                    |  WebSocket API (18789)   |              |
                    +-------+------------------+--------------+
                            |                  |
             +--------------+------+     +-----v---------+
             |                     |     |  Tool Target   |
        +----v---+  +------v--+   |     |  (host or      |
        | CLI    |  | Web UI  |   |     |   Docker       |
        | (sep.  |  | Control |   |     |   sandbox)     |
        | proc.) |  | UI      |   |     +----------------+
        +--------+  +---------+   |
                                  |
                           +------v------+
                           | Nodes       |
                           | (mobile/    |
                           |  headless)  |
                           +-------------+
```

**Everything runs in one process.** The gateway, agent runtime, channels, hooks, and
extensions all share the same Node.js process. The CLI is a separate process that
connects to the gateway via WebSocket. Nodes are separate devices connecting via
WebSocket.

---

## Communication Flow

### Inbound message (platform -> agent)

```
Telegram/WhatsApp/etc.
    |
    v
Channel plugin (inside gateway process)
    |  receives message via platform-specific transport
    |  (Baileys WebSocket for WhatsApp, Bot API polling for Telegram)
    |
    v
Gateway message router
    |  determines target agent via bindings/routing rules
    |  resolves session (per-sender, per-peer, main, etc.)
    |
    v
Agent runtime (embedded, same process)
    |  serialized per-session processing
    |  model API call with tools + context
    |
    v
Tool dispatch (if model calls tools)
    |  exec -> shell command (host or sandbox)
    |  read/write/edit -> filesystem (host or sandbox)
    |  message -> cross-channel send
    |  browser -> CDP-controlled Chromium
    |
    v
Agent response
    |
    v
Channel plugin outbound.sendText()
    |
    v
Platform API (sends reply to user)
```

### Agent tool execution

```
Agent runtime (inside gateway process)
    |
    |-- exec tool call --->  WHERE depends on sandbox config:
    |                        - sandbox.mode=off:      host filesystem, same user
    |                        - sandbox.mode=non-main: Docker container (non-main sessions)
    |                        - sandbox.mode=all:      Docker container (all sessions)
    |
    |-- read/write/edit -->  Same routing as exec (host or sandbox)
    |
    |-- elevated exec --->   Always on host (intentional escape hatch)
    |                        Requires tools.elevated.enabled=true
    |
    |-- gateway tools ---->  Always on host (cron, nodes, gateway restart)
    |                        These are gateway-backed, not sandboxed
```

### CLI -> Gateway

```
openclaw <command>
    |
    v
WebSocket connection to gateway (ws://127.0.0.1:18789)
    |  authenticate via token/password/auto-pair
    |
    v
RPC: { type: "req", id, method, params }
    |  methods: agent, send, health, status, config.get, etc.
    |
    v
Response: { type: "res", id, ok, payload }
```

---

## Filesystem Layout

### Gateway-owned paths (`~/.openclaw/`)

| Path | Contents | Sensitivity |
|------|----------|-------------|
| `~/.openclaw/openclaw.json` | Main configuration (JSON5). Channels, models, sandbox, tools, auth. | HIGH — contains gateway tokens, provider API keys (via env refs) |
| `~/.openclaw/credentials/` | Channel credentials, pairing allowlists, device tokens | CRITICAL — WhatsApp session keys, bot tokens, OAuth secrets |
| `~/.openclaw/.env` | Global environment variables | HIGH — API keys, tokens |
| `~/.openclaw/agents/<id>/sessions/` | Session transcript JSONL files | MEDIUM — conversation history |
| `~/.openclaw/agents/<id>/agent/auth-profiles.json` | Per-agent OAuth tokens, API keys | CRITICAL — agent-specific secrets |
| `~/.openclaw/extensions/` | Installed channel/hook extensions | LOW — code, not secrets |
| `~/.openclaw/hooks/` | Managed hook scripts | LOW — automation code |
| `~/.openclaw/sandboxes/` | Sandbox container working directories | LOW — ephemeral |
| `/tmp/openclaw/` | Log files | LOW |

### Agent workspace (`~/.openclaw/workspace` by default)

| Path | Contents | Sensitivity |
|------|----------|-------------|
| `AGENTS.md` | Agent instructions/memory (loaded at session start) | LOW — user-editable |
| `SOUL.md` | Persona definition and boundaries | LOW |
| `USER.md` | User profile and preferences | LOW |
| `IDENTITY.md` | Agent name and emoji | LOW |
| `TOOLS.md` | Tool guidance notes | LOW |
| `BOOTSTRAP.md` | First-run setup instructions | LOW |
| `MEMORY.md` | Long-term curated memory | LOW |
| `memory/` | Daily memory logs | LOW |
| `skills/` | Workspace-specific skill overrides | LOW |

**The workspace is the default cwd, NOT a hard sandbox.** Without sandboxing enabled,
absolute paths in tool calls can reach the entire host filesystem, including
`~/.openclaw/credentials/`.

---

## Security Boundaries

### Three security layers (evaluated in order)

```
1. Tool Policy (WHICH tools are available)
   |  allow/deny lists, profiles (minimal/coding/messaging/full)
   |  Tool policy is a hard stop: denied tools cannot be used regardless
   |  of sandbox or elevated settings.
   |
   v
2. Sandbox (WHERE tools execute)
   |  off:      host filesystem (default!)
   |  non-main: Docker for non-main sessions
   |  all:      Docker for everything
   |
   v
3. Elevated Mode (ESCAPE HATCH for exec only)
      When sandboxed, elevated exec runs on host.
      Requires tools.elevated.enabled=true.
      Does NOT grant extra tools — only changes execution location.
```

**Tool policy always wins.** If `exec` is denied, neither sandbox nor elevated
can override that. Elevated only changes where an already-allowed exec runs.

### What is NOT sandboxed (even when sandbox.mode=all)

- The gateway process itself (always on host)
- Channel plugins (run inside gateway process)
- Hook scripts (run inside gateway process)
- Extension code (run inside gateway process)
- Gateway-backed tools: cron, nodes, gateway restart
- Elevated exec (intentional host escape)

### What IS sandboxed (when enabled)

- Agent tool calls: exec, read, write, edit, apply_patch, process
- Browser (optional separate sandbox container)
- Workspace filesystem access (configurable: none/ro/rw)

### Environment variable isolation

**With Docker sandbox (`sandbox.mode=all`):**

The docs explicitly state: *"Sandbox exec does **not** inherit host `process.env`."*

This means when the agent calls `exec("env")` or `exec("echo $ANTHROPIC_API_KEY")`,
those commands run inside a Docker container that has a **clean environment** — no
gateway env vars leak in. To pass specific vars into the sandbox, you must explicitly
use `agents.defaults.sandbox.docker.env`, which gives us precise control over what
the agent can see.

**Without sandbox (`sandbox.mode=off`):**

Exec runs directly on the host. The agent can:
- `exec("env")` — sees ALL gateway process env vars (API keys, tokens, etc.)
- `exec("cat /proc/self/environ")` — same
- `read("~/.openclaw/.env")` — reads the env file directly
- `read("~/.openclaw/openclaw.json")` — reads full gateway config

**This is the fundamental reason sandboxing matters for Slopbox.**

### Filesystem isolation

**With Docker sandbox + `workspaceAccess=none` (default):**

- Agent tools operate in `~/.openclaw/sandboxes/` — completely isolated directory
- Agent CANNOT access `~/.openclaw/openclaw.json`, `~/.openclaw/credentials/`,
  or any other gateway files
- Skills are mirrored into the sandbox workspace automatically

**With Docker sandbox + `workspaceAccess=rw`:**

- Agent workspace (`~/.openclaw/workspace`) mounted at `/workspace` in container
- Agent can read/write workspace files (AGENTS.md, SOUL.md, etc.)
- Agent still CANNOT access `~/.openclaw/openclaw.json` or `~/.openclaw/credentials/`
- Only the specific workspace directory is bind-mounted, not the parent

**Without sandbox:**

- Workspace is just the default cwd — NOT a chroot
- Absolute paths in tool calls reach the entire host filesystem
- Agent can `read("~/.openclaw/credentials/whatsapp.json")` etc.

### Key security observations for Slopbox

1. **Agent and gateway share a filesystem by default.** Without sandboxing, the
   agent's `read` tool can access `~/.openclaw/credentials/`, `openclaw.json`,
   and all other gateway files. This is the primary threat vector.

2. **Sandboxing is OFF by default.** Must be explicitly enabled.

3. **With sandbox enabled, env vars and filesystem are both isolated.** The Docker
   container gets a clean env (no host vars) and an isolated filesystem (no access
   to `~/.openclaw/` parent directory). This is the primary mechanism for
   preventing the agent from seeing gateway secrets.

4. **Channel extension code runs in the gateway process**, not in the sandbox.
   This is actually good for us — our channel extension (which needs credentials
   for the HMAC handshake) runs at gateway privilege level, isolated from the
   agent's tool execution sandbox.

5. **The sandbox has no network by default.** `sandbox.docker.network` defaults
   to `"none"`. Package installs and outbound requests from sandboxed tools fail
   unless network is explicitly configured.

6. **Configuration files have 600/700 permissions** (owner-only), but this only
   matters if the agent runs as a different OS user. Inside the same process,
   the agent runtime can read anything the gateway process can.

7. **Per-agent isolation in multi-agent mode** gives each agent a separate
   workspace, session store, and auth profile. But they still share the same
   gateway process and `~/.openclaw/` directory.

8. **Host exec is explicitly filtered.** Even in unsandboxed mode, host exec
   rejects `env.PATH` overrides and loader variables (`LD_*`/`DYLD_*`) to
   prevent binary hijacking. But this does NOT prevent reading env vars.

---

## Slopbox Architecture

### Design principle

OpenClaw's Docker sandbox provides **sufficient isolation** between the agent and
the gateway process. The agent cannot read gateway env vars, cannot access
`~/.openclaw/` config/credentials, and has no network access from inside the
sandbox. This means:

- **Channel credentials (Telegram, WhatsApp, etc.) live directly on the VM**
  in `~/.openclaw/credentials/`, managed by the gateway process.
- **No WebSocket proxy or custom channel extension is needed.** Standard OpenClaw
  channels (grammY for Telegram, Baileys for WhatsApp, etc.) work natively.
- **The control plane manages VMs and gateway configuration**, not message relay.

### VM isolation model

Each agent runs in a dedicated VM (Hetzner Cloud or Fly.io Machine). Inside the VM:

```
+----------------------------------------------------------+
|  VM (one per agent)                                      |
|                                                          |
|  +----------------------------------------------------+  |
|  | OpenClaw Gateway Process (unsandboxed)              |  |
|  |                                                     |  |
|  |  - Channel plugins (Telegram, WhatsApp, etc.)       |  |
|  |  - Agent runtime (embedded pi-mono)                 |  |
|  |  - WebSocket API (:18789, firewalled)               |  |
|  |  - Config RPC (config.patch, config.set)            |  |
|  |  - Credentials at ~/.openclaw/credentials/          |  |
|  |  - Config at ~/.openclaw/openclaw.json              |  |
|  |  - LLM API keys in host env vars                    |  |
|  +------+---------------------------------------------+  |
|         |                                                |
|         | tool dispatch (sandbox.mode=all)               |
|         v                                                |
|  +----------------------------------------------------+  |
|  | Docker Sandbox (hypervisor + virtio-fs)             |  |
|  |                                                     |  |
|  |  - Agent tool execution (exec, read, write, edit)   |  |
|  |  - NO host env vars (clean environment)             |  |
|  |  - NO access to ~/.openclaw/ (isolated fs)          |  |
|  |  - NO network (docker.network=none)                 |  |
|  |  - Workspace mounted at /workspace (rw)             |  |
|  +----------------------------------------------------+  |
|                                                          |
|  Metrics: Provider Metrics API (external to VM)          |
|    Fly: Prometheus API — CPU, RAM, disk, network         |
|    Hetzner: REST API — CPU, disk, network (no RAM)       |
+----------------------------------------------------------+
```

### Message flow (no proxy)

```
User (Telegram/WhatsApp)
    |
    v
Platform API (Telegram Bot API, WhatsApp Baileys)
    |
    v
OpenClaw Gateway (on the VM, holds credentials directly)
    |  channel plugin receives message natively
    |  routes to agent session
    |
    v
Agent runtime (embedded in gateway, same process)
    |  model API call (Anthropic/OpenAI, keys in host env)
    |  tool calls dispatched to Docker sandbox
    |
    v
Agent response
    |
    v
Channel plugin outbound (gateway process)
    |  sends reply via native platform API
    |
    v
User receives reply
```

**No intermediary.** The gateway talks to platforms directly. Credentials never
enter the sandbox. The agent never sees them.

### Control plane (cb-api)

The control plane does NOT relay messages. It manages infrastructure and config.

**User-facing functions:**

| Function | How it works |
|----------|-------------|
| Provision VM | Create VM via VPS provider (Hetzner/Fly), inject OpenClaw image + base config |
| Start / Stop / Destroy VM | VPS lifecycle via provider API |
| Configure channels | Write channel config via gateway `config.patch` RPC; WhatsApp QR via `web.login.*` RPC |
| Configure agent persona | Write workspace files via gateway `/tools/invoke` (write tool, sandbox workspace mount) |
| Configure model/provider | Set `agents.defaults.model` via gateway `config.patch` RPC |
| View agent status | Query gateway health endpoint via HTTP |

**Admin functions:**

| Function | How it works |
|----------|-------------|
| Overage enforcement | Monitor usage, stop/destroy VMs that exceed plan limits |
| Plan management | CRUD on plans, VPS configs, usage limits |
| User management | CRUD on users, plan assignments |

### Gateway configuration lockdown

The OpenClaw gateway has powerful config/admin capabilities that must be
restricted on Slopbox VMs. See `GATEWAY.md` for full endpoint reference.

**Locked down via tool policy:**

| Tool / Capability | Risk | Mitigation |
|-------------------|------|------------|
| `gateway` tool | Agent can restart/reconfigure gateway | `tools.deny: ["gateway"]` |
| `cron` tool | Agent can schedule arbitrary runs | `tools.deny: ["cron"]` |
| `nodes` tool | Agent can interact with paired devices | `tools.deny: ["nodes"]` |
| Elevated exec | Agent can escape sandbox to host | `tools.elevated.enabled: false` |

**Locked down via sandbox:**

| Attack vector | Risk | Mitigation |
|---------------|------|------------|
| `exec("env")` | Reads host env vars (API keys, tokens) | Sandbox doesn't inherit host `process.env` |
| `read("~/.openclaw/openclaw.json")` | Reads gateway config | Sandbox filesystem isolated to `/workspace` |
| `read("~/.openclaw/credentials/...")` | Reads channel secrets | Same — no access to `~/.openclaw/` |
| HTTP to `127.0.0.1:18789/tools/invoke` | Bypasses sandbox entirely | `docker.network=none` — no network at all |
| HTTP to `127.0.0.1:18789` + `config.*` RPC | Modifies gateway config | Same — no network |

**Locked down via gateway config:**

| Setting | Value | Purpose |
|---------|-------|---------|
| `gateway.bind` | `loopback` | Not reachable from outside VM (exposed to user via reverse proxy / Tailscale with auth) |
| `gateway.auth.mode` | `token` | Requires auth even on loopback |

**Enabled but restricted:**

| Feature | Status | Notes |
|---------|--------|-------|
| **Control UI** | Enabled | User-facing chat + status dashboard. `config.*` RPC methods must be restricted — users should not be able to modify sandbox settings, tool policy, or gateway auth via the UI. Config changes go through the control plane API only. |
| **Hooks** | Enabled | `/hooks/wake` and `/hooks/agent` allow automation (cron triggers, control plane triggering agent actions, external integrations). Secured via dedicated hook token + `allowedAgentIds`. |
| **mDNS/Bonjour** | Default | Low risk — advertises on local network segment only, which on a cloud VM is effectively nothing. Leave at default. |

**What needs restriction in the Control UI / gateway RPC:**

The Control UI talks to the gateway via WebSocket RPC. Some methods are dangerous
if exposed to end users:

| RPC method | Risk | Recommendation |
|------------|------|----------------|
| `config.set` / `config.*` | User can disable sandbox, change tool policy, modify auth | **Block or restrict to admin.** Config changes must go through control plane. |
| `exec.approvals.*` | User can whitelist host exec commands | **Block.** Approval policy is set at provision time. |
| `update.run` | User can modify the gateway binary | **Block.** Updates managed by control plane. |
| `channels.*` (start/stop/login) | User can re-pair WhatsApp, stop channels | **Allow with caution.** May need channel-specific restrictions. |
| `chat.*` | User sends messages, views history | **Allow.** This is the primary user interaction. |
| `sessions.*` | User views/manages sessions | **Allow.** Normal usage. |
| `status`, `health` | Read-only status | **Allow.** |
| `cron.*` | User manages scheduled tasks | **Allow with limits.** Useful for automation. |

### Control UI access model and RPC gating

**The user never connects to the gateway directly.** The gateway binds to
`0.0.0.0:18789` on the VM but is firewalled to the control plane only. Users
access the Control UI through the cb-api gateway proxy:

```
User browser
    │
    ▼
our-app.com/agents/{id}/gateway/* (Convex session auth)
    │
    ▼
cb-api gateway proxy (gateway_proxy.rs)
    │  1. Validates user session (Convex auth DB)
    │  2. Resolves agent → VPS, checks ownership + VPS state
    │  3. Opens upstream WebSocket to VM gateway
    │  4. Intercepts the connect handshake:
    │     - Injects the real gateway token into auth params
    │     - Recomputes the HMAC signedNonce with the real token
    │  5. Relays frames bidirectionally with filtering
    │
    ▼
Gateway WebSocket (:18789 on VM)
```

**The gateway token never reaches the browser.** The user authenticates with
their Convex session. The proxy replaces the auth payload in the WebSocket
`connect` handshake with the real gateway token before forwarding upstream.
The user cannot extract the token from the proxied connection.

**The VM address never reaches the browser.** The proxy resolves it server-side
from the VPS database record. The browser only sees `our-app.com/agents/{id}/...`.

**Is the Control UI safe to access through the proxy? Yes**, because the proxy
applies two layers of filtering:

**1. HTTP endpoint blocking** — `POST /tools/invoke` is rejected at the HTTP
proxy layer. This endpoint allows direct tool invocation (shell exec, file
read/write) that would bypass the agent pipeline and sandbox. Blocked
unconditionally.

**2. WebSocket RPC method blocking** — The proxy parses every client→gateway
text frame as JSON, extracts the `method` field, and checks it against a
blocklist before forwarding. Blocked methods get an error response
(`code: -32601`) sent back to the client without the frame ever reaching the
gateway. Currently blocked:

| Blocked method pattern | What it prevents |
|------------------------|------------------|
| `config.*` | Disabling sandbox, changing tool policy, modifying auth, altering channel config |
| `exec.approvals.*` | Whitelisting host exec commands |
| `exec.approval.resolve` | Approving pending host exec requests |
| `update.run` | Modifying the gateway binary/package |

**This is sufficient for gating dangerous RPC endpoints.** The OpenClaw
WebSocket RPC protocol uses plain JSON text frames with a predictable
`{ type: "req", method: "..." }` structure. The proxy does not need to
understand the gateway's internal state — it only needs to match the `method`
string. The filtering is already implemented in `cb-api/src/gateway_proxy.rs`
(`is_blocked_method()`).

**Gateway→client frames are not filtered** and don't need to be. The gateway
sends `{ type: "res" }` responses and `{ type: "event" }` streams. These are
read-only data (chat transcripts, agent output, status updates). There is no
server-initiated frame that could cause the client to gain elevated access.

**What passes through to the user:**

| RPC method | Status | Rationale |
|------------|--------|-----------|
| `chat.*` | Allowed | Primary user interaction (send, history, inject, abort) |
| `sessions.*` | Allowed | Session management (list, reset, configure) |
| `channels.*` | Allowed | Channel status, QR login, start/stop |
| `cron.*` | Allowed | Schedule/manage agent automation |
| `skills.*` | Allowed | Enable/disable agent skills |
| `status`, `health` | Allowed | Read-only status |
| `models.list` | Allowed | Read-only model listing |
| `agent`, `agent.wait` | Allowed | Trigger/observe agent runs (sandbox + tool policy apply) |
| `logs.tail` | Allowed | Read-only but may expose sensitive data in logs — review |

**Note on `config.patch` and `config.set` for control plane use:** These
methods are blocked for *users* at the proxy layer, but the control plane
itself connects to the gateway directly (not through its own proxy). When
cb-api needs to write config (e.g. adding a channel), it opens a direct
WebSocket to the VM gateway, authenticates with the gateway token, and calls
`config.patch` — bypassing the proxy blocklist entirely. This is the intended
two-tier access model: users get filtered access, the control plane gets full
access.

### Recommended `openclaw.json` for Slopbox VMs

```json5
{
  "agents": {
    "defaults": {
      "workspace": "~/.openclaw/workspace",
      "sandbox": {
        "mode": "all",
        "scope": "session",
        "workspaceAccess": "rw",
        "docker": {
          "network": "none",
          "env": {
            // Only pass through what the agent needs inside the sandbox.
            // All gateway-level secrets are deliberately excluded.
          }
        }
      }
    }
  },

  "tools": {
    "profile": "coding",
    "deny": ["gateway", "nodes"],
    "elevated": { "enabled": false }
  },

  "gateway": {
    "bind": "loopback",
    "auth": { "mode": "token", "token": "${OPENCLAW_GATEWAY_TOKEN}" }
  },

  "hooks": {
    "enabled": true,
    "token": "${OPENCLAW_HOOKS_TOKEN}",
    "allowedAgentIds": ["main"]
  }

  // Channel config written by control plane at provision/config time.
  // e.g. channels.telegram, channels.whatsapp with native OpenClaw settings.
}
```

### Resource monitoring

Resource metering uses **provider metrics APIs** (external, tamper-proof).
See [PRD.md](PRD.md) section 8 for full design.

**Provider Metrics APIs:**

| Provider | CPU | RAM | Disk | Network |
|----------|-----|-----|------|---------|
| Fly.io | Prometheus (`fly_instance_cpu`) | Prometheus (`fly_instance_memory_*`) | Prometheus (`fly_instance_disk_*`) | Prometheus (`fly_instance_net_*`) |
| Hetzner | REST (`/servers/{id}/metrics`) | **Not available** | REST (IOPS + bandwidth) | REST (bandwidth + packets) |

**Note:** Hetzner lacks RAM metrics. For Hetzner VMs, RAM usage is estimated
from the VPS config tier (fixed allocation). Fly provides complete coverage
via the Prometheus API.

Effective values accumulate in `vps_usage_periods`, checked against plan limits by
the background monitor (`spawn_monitor`). Overage enforcement: stop VM + notify
user when usage exceeds limits AND overage cost exceeds overage budget.

---

## Channel Credential Management

How OpenClaw handles credentials for each channel, and what that means for
Slopbox's control plane.

### Credential storage model

OpenClaw uses **two distinct storage locations** for credentials:

1. **`openclaw.json` (inline config)** — Tokens stored directly in the config
   file as string values. This is the primary method for Telegram, Discord,
   Slack, and Google Chat.

2. **`~/.openclaw/credentials/` (filesystem)** — Used only by WhatsApp (Baileys
   session state). Stored at `~/.openclaw/credentials/whatsapp/<accountId>/creds.json`.
   Legacy path: `~/.openclaw/credentials/` for default account (auto-migrated).

**LLM provider credentials** are separate: stored in
`~/.openclaw/agents/<agentId>/agent/auth-profiles.json` or as host env vars
(`ANTHROPIC_API_KEY`, etc.) / `~/.openclaw/.env`.

### Per-channel credential patterns

| Channel | Primary credential | Config path | Env var fallback | Filesystem state |
|---------|--------------------|-------------|------------------|------------------|
| **Telegram** | Bot token (from BotFather) | `channels.telegram.botToken` or `channels.telegram.tokenFile` | `TELEGRAM_BOT_TOKEN` (default account only) | None |
| **WhatsApp** | Baileys session (QR pairing) | `channels.whatsapp.accounts.<id>` (enable/settings only) | None | `~/.openclaw/credentials/whatsapp/<accountId>/creds.json` |
| **Discord** | Bot token (from Developer Portal) | `channels.discord.token` | `DISCORD_BOT_TOKEN` (default account only) | None |
| **Slack** | Bot token + app token (or signing secret) | `channels.slack.botToken`, `channels.slack.appToken` | `SLACK_BOT_TOKEN`, `SLACK_APP_TOKEN` (default account only) | None |
| **Google Chat** | Service account JSON | `channels.googlechat.serviceAccount` (inline) or `channels.googlechat.serviceAccountFile` | `GOOGLE_CHAT_SERVICE_ACCOUNT` or `GOOGLE_CHAT_SERVICE_ACCOUNT_FILE` | Optional file via `serviceAccountFile` |
| **Mattermost** | Bot token | `channels.mattermost.botToken` | None documented | None |

**Resolution order:** Config values win over env var fallbacks. Env vars only
apply to the default account. Multi-account setups must use
`channels.<channel>.accounts.<id>.<field>`.

### How the UI handles credential setup

**Control UI RPC methods for channels:**

| RPC | Purpose |
|-----|---------|
| `channels.status` | View channel connectivity and state |
| `web.login.*` | QR-based login flows (WhatsApp pairing) |
| `config.patch` | Write channel config (tokens, settings) — JSON merge patch semantics |
| `config.set` | Single-key config updates |

**For token-based channels (Telegram, Discord, Slack):**

The Control UI exposes a config form (rendered from `config.schema`, including
channel schemas). The user enters the token, the UI calls `config.patch` to
write it into `openclaw.json`. The gateway hot-reloads the config change and
starts the channel — no restart needed.

**For WhatsApp (QR pairing):**

WhatsApp is different because Baileys uses a multi-step QR pairing protocol,
not a static token. The flow:

1. User triggers QR login via Control UI (calls `web.login.*` RPC or
   `openclaw channels login --channel whatsapp`)
2. Gateway generates QR code, displayed in UI
3. User scans QR with WhatsApp mobile app
4. Baileys completes pairing, writes session state to
   `~/.openclaw/credentials/whatsapp/<accountId>/creds.json`
5. Gateway owns the WhatsApp socket and reconnection loop from this point

The QR pairing is handled entirely within the gateway process. The credential
files are written by the gateway itself (Baileys library), not by external
tooling.

**DM pairing (all channels):**

After channel connection, new senders must be approved:
- Default DM policy is `"pairing"` (unknown senders get a pairing request)
- Approvals via CLI: `openclaw pairing list <channel>` / `openclaw pairing approve <channel> <CODE>`
- Pairing requests expire after 1 hour, max 3 pending per channel
- Approved pairings persist in the channel's allow-store

### Config hot-reload behavior

The gateway watches `~/.openclaw/openclaw.json` and applies changes
automatically. Three reload modes:

| Mode | Behavior |
|------|----------|
| **Hybrid** (default) | Hot-applies safe changes instantly. Auto-restarts for critical ones (port, bind, TLS, plugins). |
| **Hot** | Hot-applies safe changes only. Logs warnings for restart-needed changes. |
| **Restart** | Full gateway restart on any config change. |
| **Off** | Disables file watching entirely. |

**Channel config changes hot-apply without restart.** Gateway server settings
(port, bind, TLS) and infrastructure (discovery, plugins) require restart.

### Implications for Slopbox control plane

**Most channels don't need filesystem writes at all.** Telegram, Discord, Slack,
and Google Chat credentials are stored inline in `openclaw.json`. The control
plane writes them via `config.patch` RPC over the gateway WebSocket connection.

**WhatsApp is the exception — but the gateway handles it.** WhatsApp credential
files are written by the Baileys library inside the gateway process during QR
pairing. The control plane just proxies the `web.login.*` QR flow to the
user's browser (which the gateway proxy already does).

**Config changes are hot-reloaded.** No gateway restart needed for channel
additions. The control plane calls `config.patch` via WebSocket RPC, the
gateway picks up the change immediately.

**How each operation is handled:**

| Operation | Mechanism |
|-----------|-----------|
| Add Telegram/Discord/Slack | `config.patch` RPC (merge channel block into openclaw.json) |
| Add WhatsApp | `web.login.*` RPC (gateway runs QR pairing, writes creds itself) |
| Change agent persona | `/tools/invoke` `write` tool (via sandbox workspace mount) |
| Update openclaw.json | `config.patch` RPC (JSON merge patch) |
| Health check | Gateway HTTP `GET /` with Bearer auth |

---

### Open questions

1. **Model API keys on the VPS**: The gateway process needs LLM provider keys.
   With sandbox enabled, agent tools can't see them. Options:
   - **Inject as host env vars at provision time** (simplest, safe with sandbox)
   - **Forward proxy injects auth headers** (most secure, no keys on VPS)
   - **Short-lived tokens** rotated by control plane

2. **Docker-in-VM feasibility**: OpenClaw sandbox requires Docker. Hetzner VMs
   have full kernels (Docker works natively). Fly Machines are Firecracker
   microVMs — Docker support needs investigation. May restrict sandbox mode to
   Hetzner or find alternative container runtime for Fly.
