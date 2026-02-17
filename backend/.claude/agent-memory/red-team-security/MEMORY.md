# Red Team Security Memory

## Key Vulnerability Patterns Found (2026-02-16)
- JWT validation disables `validate_exp` and clears `required_spec_claims` in auth.rs:29-30
- Static ADMIN_API_TOKEN compared with `==` (non-constant-time) in auth.rs:113
- Static token path bypasses all user/role/status checks -- no UserId inserted
- Admin handlers have no audit logging
- `cleanup_stuck` has no time threshold (acknowledged TODO)
- Error responses leak sqlx and infra error details via `self.to_string()`
- `gateway_token` has `Serialize` derived on Agent model -- fragile if DTO bypassed
- JWT passed in query params for WebSocket (logged by TraceLayer)

## File Locations
- Auth middleware: crates/cb-api/src/auth.rs
- Admin routes: crates/cb-api/src/routes/admin.rs
- Route registration: crates/cb-api/src/routes/mod.rs
- Config: crates/cb-api/src/config.rs
- DB models: crates/cb-db/src/models.rs
- Gateway proxy: crates/cb-api/src/gateway_proxy.rs
- Forward proxy: crates/cb-api/src/proxy.rs
- Error types: crates/cb-api/src/error.rs
- Main: crates/cb-api/src/main.rs

## Architecture Notes
- jsonwebtoken v9, HS256, single shared JWT_SECRET
- Admin auth: static token OR JWT+Admin role, handled by admin_middleware
- Regular auth: JWT only via auth_middleware, then status_middleware for active check
- Gateway proxy: authenticates via JWT (header or query param), proxies to VPS
- Forward proxy: authenticates via Basic auth (agent_id:gateway_token), separate hyper server
- CORS: exact origin from FRONTEND_ORIGIN env var, credentials allowed
