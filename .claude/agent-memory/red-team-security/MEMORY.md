# Red Team Security Memory

## Security Review Status
- **Last full review**: 2026-02-15
- **Report**: `/security/RED_TEAM.md` (obsolete — focused on removed sidecar)
- **Scope**: All 5 backend crates, OpenClaw integration architecture
- **Note**: Sidecar findings (CRIT-004, HIGH-001) are resolved — crate removed 2026-02-16

## Key Vulnerability Patterns

### 1. Cloud-Init Injection (CRIT-001)
- `cb-infra/src/hetzner.rs:87-110` -- heredoc and echo interpolation without escaping
- Agent name flows from `CreateAgentRequest` -> `build_workspace_files()` -> `FileMount` -> `cloud_init_user_data()`
- User-controlled strings break heredoc boundaries

### 2. SSRF via Forward Proxy (CRIT-002)
- `cb-api/src/proxy.rs` -- no destination filtering on CONNECT or HTTP forwarding
- Agent can reach cloud metadata (169.254.169.254), control plane DB

### 3. Secrets in VM Environment (CRIT-003)
- `vps.rs` injects gateway_token, proxy URL as env vars
- All visible to any host process including gateway (which contains agent runtime)
- Gateway token serves dual purpose: proxy auth AND gateway auth (HIGH-002)

### 4. WebSocket Filter Bypass (CRIT-005)
- `gateway_proxy.rs:387-395` -- binary frames forwarded without RPC method filtering
- Only Text frames are parsed and filtered by `is_blocked_method()`

### 5. Auth Model (HIGH-003)
- BFF pattern: single CONTROL_PLANE_API_KEY + trusted X-User-Id header
- `authenticate_session_cookie()` exists but only used by gateway proxy, not main API

## Architecture Security Notes
- Docker sandbox is SINGLE barrier between agent and all secrets
- Everything runs as root on VM (/root/.openclaw/)
- No defense-in-depth: no SELinux, no AppArmor, no seccomp
- StubCollector means no real metrics collection -- enforcement gaps
- tools_deny user-overridable via UpdateConfigRequest (HIGH-007)
- No CORS, no rate limiting, no request size limits on proxy

## Trust Boundaries
- Agent <-> Gateway Process: Docker sandbox only
- User Browser <-> Gateway: Session cookie + WS RPC filter
- Control Plane <-> Gateway: Gateway token over HTTP (firewalled)
- Agent <-> Internet: Forward proxy with Basic auth

## File Locations (for quick reference)
- Auth middleware: `cb-api/src/auth.rs`
- Forward proxy: `cb-api/src/proxy.rs`
- Gateway proxy: `cb-api/src/gateway_proxy.rs`
- Config builder: `cb-api/src/openclaw_config.rs`
- VPS provisioning: `cb-api/src/routes/vps.rs`
- Config update: `cb-api/src/routes/config.rs`
- Hetzner cloud-init: `cb-infra/src/hetzner.rs`
- DB models: `cb-db/src/models.rs`
