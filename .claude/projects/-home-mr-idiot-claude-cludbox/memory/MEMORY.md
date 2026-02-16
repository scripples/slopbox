# Slopbox Project Memory

## Project Overview
- SaaS platform renting sandboxed AI agents as containerized apps
- Rust workspace with 5 crates: cb-api, cb-db, cb-infra, fly-api, sprites-api
- Next.js frontend in /frontend
- Isolation: Fly.io Machines, Hetzner Cloud, or Sprites.dev (one container per agent)
- Provider-agnostic VPS lifecycle via VpsProvider trait
- OpenClaw runs natively on VMs with Docker sandbox isolation
- Control plane manages VMs via gateway-native RPC (config.patch, /tools/invoke) and sprites exec

## Architecture Decisions
- **Multi-provider VPS** — `VpsProvider` trait in cb-infra, with FlyProvider, HetznerProvider, and SpritesProvider impls
- **Provider config is self-contained** — each provider reads its own env vars via `from_env()`, NOT from AppConfig
- **ProviderName enum** — `cb_infra::ProviderName` (Fly, Hetzner, Sprites) with serde, Display, FromStr
- **ProviderRegistry** — `cb_infra::build_providers()` constructs all available providers at startup
- **Config injection** — openclaw.json + workspace files injected at VPS provision time via FileMount
- **Config targeting** — sprites: exec-based file write + service restart; hetzner: gateway HTTP API; fly: gateway RPC (future)
- **Gateway proxy** — cb-api proxies user WebSocket connections to gateway, intercepts handshake to inject gateway token, filters dangerous RPC methods (config.*, exec.approvals.*, update.run)
- **HMAC proxy auth** — agent outbound traffic via forward proxy with Basic auth (agent_id:gateway_token)
- **JWT auth** — Auth.js in Next.js issues HS256 JWTs, cb-api validates with shared JWT_SECRET
- **User role/status** — UserRole (user/admin), UserStatus (pending/active/frozen), enforced by middleware
- **Admin panel** — /admin/users and /admin/vpses routes, admin middleware checks role=admin
- **CORS** — tower-http CorsLayer configured with FRONTEND_ORIGIN
- Edition: Rust 2024

## Crate Structure (5 crates)
- cb-api: Control plane API (axum) — agent/VPS lifecycle, config targeting, gateway proxy, usage checks, forward proxy, background monitor, admin routes
- cb-db: models + migrations (sqlx) — Plan, VpsConfig, User (with role/status), Vps, Agent, AgentChannel
- cb-infra: VpsProvider trait + FlyProvider + HetznerProvider + SpritesProvider
- fly-api: Typed Fly.io Machines API client (reqwest, serde)
- sprites-api: Typed Sprites API client — sprites CRUD, exec, checkpoints, network policy, services

## Key API Endpoints (cb-api)
- VPS lifecycle: POST/DELETE /agents/{id}/vps, POST start/stop
- Config targeting: PUT /agents/{id}/config, PUT /agents/{id}/workspace/{filename}
- Agent lifecycle: POST /agents/{id}/restart, GET /agents/{id}/health
- Channels: POST/GET/DELETE /agents/{id}/channels
- Gateway proxy: ANY /agents/{id}/gateway/{*path}, WS /agents/{id}/gateway/ws
- Usage: GET /agents/{id}/usage
- Admin: GET /admin/users, PUT /admin/users/{id}/status, PUT /admin/users/{id}/role, GET /admin/vpses, POST /admin/vpses/{id}/stop, POST /admin/vpses/{id}/destroy

## Key Dependencies & Versions
- axum 0.8 (in cb-api, with ws feature)
- sqlx 0.8 with compile-time-unchecked queries
- reqwest 0.12 (in fly-api, cb-api, sprites-api)
- hcloud 0.25 (in cb-infra for Hetzner Cloud API)
- tokio-tungstenite 0.26 (in cb-api for gateway WebSocket proxy)
- async-trait 0.1 (for VpsProvider trait + MetricsCollector trait)
- jsonwebtoken 9 (JWT validation in cb-api)
- tower-http 0.6 (trace + cors features)
- serde 1 (serialization everywhere)

## Auth Model
- Auth.js (NextAuth v5 beta) manages OAuth + sessions in PostgreSQL
- JWT strategy: Auth.js issues session JWTs, /api/token route signs HS256 JWT with JWT_SECRET
- API routes: JWT Bearer token (Authorization: Bearer <jwt>)
- Gateway proxy: JWT via query param (?token=<jwt>) for WebSocket, Bearer header for HTTP
- Admin middleware: checks user.role == Admin && user.status == Active
- Status middleware: checks user.status == Active (applied to most routes)
- /users/me and /plans accessible to pending users

## Build Status
- All 5 crates compile cleanly (zero warnings)
- Clippy clean (zero warnings)
- No tests yet

## Migrations
- 001-012: original schema (plans, vps_configs, users, vpses, agents, auth, usage, channels)
- 013: user_role + user_status enums + columns on users table
- 014: seed data (sprites-standard + hetzner-standard VpsConfigs, demo plan)

## Frontend Structure
- Next.js 15, React 19, Tailwind CSS 4, Auth.js v5 beta
- /login — Google + GitHub OAuth
- /pending — awaiting admin approval
- /dashboard — agent list + create
- /dashboard/agents/[id] — agent detail, VPS actions
- /dashboard/agents/[id]/chat — WebSocket chat with gateway
- /admin/users — user management (approve, freeze, role toggle)
- /admin/vpses — VPS management (stop, destroy)
- /api/token — issues JWT for cb-api
- /api/auth/[...nextauth] — Auth.js handlers

## Deployment
- Backend: Dockerfile (multi-stage rust build) + fly.toml (slopbox-api on Fly.io)
- Frontend: Vercel (Next.js)
- Database: Fly Postgres (shared between cb-api and Auth.js)
- Secrets: JWT_SECRET, SPRITES_API_TOKEN, FRONTEND_ORIGIN, DATABASE_URL
