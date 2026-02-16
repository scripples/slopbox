# cb-db

PostgreSQL models and migrations for the Slopbox control plane.

## Purpose

Provides all database models, query methods, and schema migrations. Uses `sqlx` 0.8 with compile-time-unchecked queries (no `DATABASE_URL` required at build time).

## Models

### Plan

Defines resource limits for a user tier.

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | PK |
| `name` | String | Unique |
| `max_agents` | i32 | |
| `max_vpses` | i32 | |
| `max_bandwidth_bytes` | i64 | Monthly limit (default 100 GB) |
| `max_storage_bytes` | i64 | Per-VPS limit (default 50 GB) |
| `max_cpu_ms` | i64 | Monthly CPU-ms limit (default 100 CPU-hours) |
| `max_memory_mb_seconds` | i64 | Monthly memory-MB-seconds limit |
| `overage_bandwidth_cost_per_gb_cents` | i64 | Overage rate: cents per GB bandwidth |
| `overage_cpu_cost_per_hour_cents` | i64 | Overage rate: cents per CPU-hour |
| `overage_memory_cost_per_gb_hour_cents` | i64 | Overage rate: cents per GB-hour memory |
| `created_at` / `updated_at` | DateTime | Auto-managed |

Methods: `insert`, `get_by_id`, `list`, `add_vps_config`, `remove_vps_config`, `overage_cost_cents`

### VpsConfig

Defines a VM size preset (provider, image, CPU, memory, disk). A VpsConfig is either a "Fly config" (Docker image) or a "Hetzner config" (OS image).

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | PK |
| `name` | String | Unique |
| `provider` | String | "fly" or "hetzner" |
| `image` | String | Docker image (Fly) or OS image (Hetzner) |
| `cpu_millicores` | i32 | |
| `memory_mb` | i32 | |
| `disk_gb` | i32 | |

Methods: `insert`, `get_by_id`, `list_for_plan`

Plans link to VpsConfigs via the `plan_vps_configs` join table (many-to-many).

### User

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | PK |
| `email` | String | Unique |
| `name` | Option\<String\> | |
| `plan_id` | Option\<UUID\> | FK → plans, ON DELETE SET NULL |
| `email_verified` | Option\<DateTime\> | Auth.js extension |
| `image` | Option\<String\> | Auth.js extension |

Methods: `insert`, `get_by_id`, `get_by_email`, `set_plan`

### OAuthAccount

Auth.js-managed OAuth credentials. Read-only from Rust.

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | PK |
| `user_id` | UUID | FK → users |
| `type` | String | "oauth" or "credentials" |
| `provider` | String | "github", "google", etc. |
| `provider_account_id` | String | |
| `refresh_token` / `access_token` | Option\<String\> | |
| `expires_at` | Option\<i32\> | |

Methods: `get_by_user_id`

### Session

Auth.js-managed sessions. Read-only from Rust.

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | PK |
| `session_token` | String | Unique |
| `user_id` | UUID | FK → users |
| `expires` | DateTime | |

### Vps

A provisioned VPS instance.

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | PK |
| `user_id` | UUID | FK → users, ON DELETE CASCADE |
| `vps_config_id` | UUID | FK → vps_configs |
| `name` | String | |
| `provider` | String | "fly", "hetzner", etc. |
| `provider_vm_id` | Option\<String\> | Remote VM ID |
| `address` | Option\<String\> | Private IP or internal DNS |
| `state` | VpsState | Provisioning / Running / Stopped / Destroyed |
| `storage_used_bytes` | i64 | |
| `cpu_used_ms` | Option\<i64\> | Cumulative, NULL for fixed-resource providers |
| `memory_used_mb_seconds` | Option\<i64\> | Cumulative, NULL for fixed-resource providers |

Methods: `insert`, `get_by_id`, `list_for_user`, `count_for_user`, `list_by_state`, `update_provider_refs`, `set_state`, `update_usage`

### Agent

A user-facing AI agent, optionally attached to a VPS.

| Field | Type | Notes |
|-------|------|-------|
| `id` | UUID | PK |
| `user_id` | UUID | FK → users, ON DELETE CASCADE |
| `vps_id` | Option\<UUID\> | FK → vpses, UNIQUE (one agent per VPS) |
| `name` | String | |
| `gateway_token` | String | 64-char hex token for proxy/gateway auth |

Methods: `insert`, `get_by_id`, `list_for_user`, `count_for_user`, `assign_vps`, `delete`, `get_by_id_and_token`, `rotate_gateway_token`

### VpsUsagePeriod

Monthly usage counters per VPS. Keyed by `(vps_id, period_start)`.

| Field | Type | Notes |
|-------|------|-------|
| `vps_id` | UUID | FK → vpses, ON DELETE CASCADE |
| `period_start` | NaiveDate | First day of month |
| `bandwidth_bytes` | i64 | Accumulated by proxy |
| `cpu_used_ms` | i64 | Accumulated by monitor |
| `memory_used_mb_seconds` | i64 | Accumulated by monitor |

Methods: `add_bandwidth` (upsert), `add_cpu_memory` (upsert), `get_current` (returns zeroes if no row), `get_user_aggregate` (sums across all user's VPSes)

### AggregateUsage

Summed usage across all of a user's VPSes for a billing period. Returned by `VpsUsagePeriod::get_user_aggregate()`.

| Field | Type | Notes |
|-------|------|-------|
| `bandwidth_bytes` | i64 | Total across all VPSes |
| `cpu_used_ms` | i64 | Total across all VPSes |
| `memory_used_mb_seconds` | i64 | Total across all VPSes |

### OverageBudget

Per-user monthly overage budget in cents. Missing row = $0 budget (no overage allowed).

| Field | Type | Notes |
|-------|------|-------|
| `user_id` | UUID | PK (composite), FK → users ON DELETE CASCADE |
| `period_start` | NaiveDate | PK (composite), first day of month |
| `budget_cents` | i64 | Monthly overage spending cap in cents |
| `created_at` / `updated_at` | DateTime | Auto-managed |

Methods: `get_current` (returns zeroes if no row), `set_budget` (upsert)

## Entity Relationships

```
plans ←──(plan_id)── users ──(user_id)──→ vpses ──(vps_config_id)──→ vps_configs
  │                    │                    │
  └── plan_vps_configs ─┘                   ├── agents (UNIQUE vps_id)
      (join table)      │                   └── vps_usage_periods (monthly)
                        ├── overage_budgets (monthly)
                        ├── accounts (Auth.js)
                        └── sessions (Auth.js)
```

- Users belong to a Plan (optional). Plans define which VpsConfigs are available.
- Each User can have multiple Vpses and Agents.
- Each Agent has at most one Vps (UNIQUE constraint on `agents.vps_id`).
- VpsUsagePeriods track monthly bandwidth/CPU/memory per Vps.
- OverageBudgets track monthly overage spending caps per User.

## Migrations

| # | File | Purpose |
|---|------|---------|
| 001 | `updated_at_trigger.sql` | Reusable `set_updated_at()` trigger function |
| 002 | `plans.sql` | Plans table (with all resource limits) |
| 003 | `vps_configs.sql` | VPS config presets |
| 004 | `plan_vps_configs.sql` | Plan ↔ VpsConfig join table |
| 005 | `users.sql` | Users table (with Auth.js extensions) |
| 006 | `vpses.sql` | VPS instances with `vps_state` enum and usage gauges |
| 007 | `agents.sql` | Agents table (with gateway token) |
| 008 | `auth.sql` | Auth.js tables (accounts, sessions, verification_tokens) |
| 009 | `vps_usage_periods.sql` | Monthly usage tracking per VPS |
| 010 | `plan_overage_rates.sql` | Overage rate columns on plans |
| 011 | `overage_budgets.sql` | Per-user monthly overage budgets |
| 012 | `agent_channels.sql` | Channel credentials per agent |

## Auth.js Tables

The `accounts`, `sessions`, and `verification_tokens` tables are managed by Auth.js (NextAuth) in the frontend. The Rust backend treats them as read-only. The `users` table is shared — Auth.js extended it with `email_verified` and `image` columns.

## Dependencies

- `sqlx` 0.8 (postgres, uuid, chrono, migrate)
- `chrono` — DateTime / NaiveDate
- `serde` — Serialization
- `uuid` — UUID v4 generation
- `rand` 0.9 — Gateway token generation
- `thiserror` — Error types
