# Cludbox Development Guidelines

Always read READMEs first, and then dive into crates and specific implementations if it appears necessary.

view @openclaw-nots/README.md for high level openclaw notes.
view @openclaw-nots/PRD.md for prodcut requiremnt document. 

Openclaw docs:
https://docs.openclaw.ai/


Cludbox is an agent-as-a-service platform based on OpenClaw. One VPS per agent (Fly.io or Hetzner). One plan, one agent to start. Security requirements:

1. **VM-level isolation.** One VPS per agent. No cross-instance filesystem or memory access.

2. **Intra-VM sandboxing.** OpenClaw's Docker sandbox (`sandbox.mode=all`, `docker.network=none`) isolates agent tool execution (exec, read, write, edit) from the gateway process. Agent cannot access host env vars, `~/.openclaw/` config/credentials, or the network from inside the sandbox. Tool policy (`tools.deny`, `tools.elevated.enabled: false`) blocks dangerous gateway-level tools. See `openclaw-notes/GATEWAY.md` §2 for full tool dispatch map.

3. **No direct host access for users.** Users interact only via OpenClaw channels (Telegram, WhatsApp, etc.) or the Control UI. No SSH, no CLI, no filesystem access. The Control UI exposes WebSocket RPC — dangerous methods (`config.*`, `exec.approvals.*`, `update.run`) must be blocked at the proxy layer. See `openclaw-notes/GATEWAY.md` §6.

4. **Resource metering.** CPU, RAM, bandwidth, and disk metered monthly. Provider metrics APIs (Fly Prometheus, Hetzner REST) are the primary source. Overages enforced by background monitor. See `openclaw-notes/PRD.md` §8.

5. **Secret isolation.** The agent is isolated from all keys by OpenClaw's sandbox architecture. LLM API keys live as host env vars — invisible to the sandboxed agent. Channel credentials live in `~/.openclaw/credentials/` — inaccessible from the sandbox filesystem. Gateway-process tools (`web_search`, `web_fetch`, `message`) make external calls on the agent's behalf without exposing keys. No custom proxy or JWT layer needed. See `openclaw-notes/GATEWAY.md` §3–§4.

## Documentation

- Every crate in `crates/` must have a `README.md` describing its purpose, public API, dependencies, configuration, and known limitations.
- The project root must have a `README.md` covering overall architecture and crate relationships.
- READMEs must be updated when a crate's public interface changes.
- Check the READMEs first when inspecting this repo's component structures.

## Build

- Edition: Rust 2024
- Build: `cargo check --workspace`
- Lint: `cargo clippy --workspace -- -D warnings`
- No tests yet.

## Conventions

- sqlx queries use `query_as!` / `query!` with compile-time-unchecked.
- Provider config is self-contained — each provider reads its own env vars via `from_env()`.
- Factory function `cb_infra::create_provider()` reads `VPS_PROVIDER` env var.

