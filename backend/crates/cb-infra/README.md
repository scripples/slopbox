# cb-infra

Provider-agnostic VPS management for Cludbox.

## Purpose

Defines the `VpsProvider` trait and implements it for Fly.io and Hetzner Cloud. Application code works against the trait; the concrete provider is selected at startup via environment variable. Storage is provider-managed (comes with the VPS).

## VpsProvider Trait

```rust
#[async_trait]
pub trait VpsProvider: Send + Sync + 'static {
    async fn create_vps(&self, spec: &VpsSpec) -> Result<VpsInfo>;
    async fn start_vps(&self, id: &VpsId) -> Result<()>;
    async fn stop_vps(&self, id: &VpsId) -> Result<()>;
    async fn destroy_vps(&self, id: &VpsId) -> Result<()>;
    async fn get_vps(&self, id: &VpsId) -> Result<VpsInfo>;
    fn name(&self) -> &str;
    fn metered_resources(&self) -> MeteredResources { ... } // default: delegates to name()
}
```

## Metered Resources

Different providers meter different resource axes:

| Provider | Bandwidth | CPU | Memory | Rationale |
|----------|-----------|-----|--------|-----------|
| Fly.io | Yes | No | No | Fixed-allocation VPS |
| Hetzner | Yes | No | No | Fixed-allocation VPS |
| Sprites | Yes | Yes | Yes | Elastic / shared resources |
| K8s | Yes | Yes | Yes | Elastic / shared resources |

The `MeteredResources` struct captures this policy. The standalone `metered_resources_for(provider: &str)` function maps provider name strings to their policy — used by the proxy and monitor which only have the DB-stored `vps.provider` string.

Unknown providers default to `ALL` (safest: over-enforce rather than under-enforce).

## Types

| Type | Fields | Description |
|------|--------|-------------|
| `VpsId(String)` | — | Wrapper for provider VPS ID |
| `VpsSpec` | `name`, `image`, `cpu_millicores`, `memory_mb`, `env`, `files` | VPS creation parameters |
| `FileMount` | `guest_path`, `raw_value` | File to inject into VPS |
| `VpsInfo` | `id`, `state`, `address` | VPS status response |
| `VpsState` | — | Enum: Starting, Running, Stopped, Destroyed, Unknown |
| `MeteredResources` | `bandwidth`, `cpu`, `memory` | Which resources a provider meters (bools) |

## FlyProvider

Uses the `fly-api` crate to manage Fly Machines.

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `FLY_API_TOKEN` | Yes | — | Fly.io API token |
| `FLY_APP_NAME` | No | `cludbox-agents` | Fly app for machines |
| `FLY_REGION` | No | `iad` | Default region |

### CPU Mapping

Maps `cpu_millicores` to Fly guest configs:

| Millicores | Fly CPUs | CPU Kind |
|------------|----------|----------|
| ≤1000 | 1 | `shared` |
| ≤2000 | 2 | `shared` |
| ≤4000 | 4 | `performance` |
| _ | 8 | `performance` |

### Notes

- VPS address derived from `private_ip` or falls back to Fly internal DNS (`{app}.internal`).
- Deletes are 404-tolerant (idempotent).

## HetznerProvider

Uses the `hcloud` crate (v0.25) to manage Hetzner Cloud servers.

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `HETZNER_API_TOKEN` | Yes | — | Hetzner Cloud API token |
| `HETZNER_LOCATION` | No | `fsn1` | Datacenter location |
| `HETZNER_NETWORK_ID` | No | — | Private network ID (integer) |
| `HETZNER_FIREWALL_ID` | No | — | Firewall ID to attach (integer) |
| `HETZNER_SSH_KEY_NAMES` | No | — | Comma-separated SSH key names |

### Server Type Mapping

Maps `cpu_millicores` to Hetzner server types:

| Millicores | Server Type |
|------------|-------------|
| ≤2000 | `cx22` |
| ≤4000 | `cx32` |
| ≤8000 | `cx42` |
| _ | `cx52` |

### Notes

- VPSes created with `ubuntu-24.04` image + cloud-init user data (not `spec.image`).
- Cloud-init writes env vars to `/etc/cludbox/env` and files to their `guest_path`, then starts `cludbox-agent` systemd service.
- VPS address derived from private network IP.
- Deletes handle 404 gracefully.

## Factory

```rust
pub fn create_provider() -> Result<Arc<dyn VpsProvider>>
```

Reads `VPS_PROVIDER` env var (falls back to `INFRA_BACKEND`, defaults to `"fly"`). Returns the appropriate provider wrapped in `Arc<dyn VpsProvider>`.

## Error Types

```rust
pub enum Error {
    Fly(fly_api::Error),         // Fly API errors
    HetznerApi(String),          // Hetzner API errors
    InvalidId(String),           // ID parsing failures
    MissingEnv(String),          // Missing environment variable
    UnknownProvider(String),     // Unknown VPS_PROVIDER value
}
```

## Dependencies

- `fly-api` — Fly.io Machines API client (workspace crate)
- `hcloud` 0.25 — Hetzner Cloud SDK
- `async-trait` — Async trait support
- `thiserror` — Error derive
- `dotenvy` — Environment variable loading
- `tracing` — Logging
