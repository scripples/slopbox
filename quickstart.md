# Cludbox Quickstart

Agent lifecycle walkthrough via the control plane API.

All examples assume authentication is handled and:

```bash
BASE=http://localhost:3000
```

## 1. List available plans

```bash
curl $BASE/plans
```

Pick a plan and note a `vps_config_id` available on it.

## 2. Create an agent

```bash
curl -X POST $BASE/agents \
  -H 'Content-Type: application/json' \
  -d '{"name": "my-researcher"}'
```

```json
{
  "id": "aaaa-...",
  "user_id": "uuuu-...",
  "name": "my-researcher",
  "vps": null,
  "created_at": "...",
  "updated_at": "..."
}
```

Save the agent `id` — every subsequent call uses it.

## 3. Provision a VPS

The `provider` field selects which backend to create the VPS on.

### 3a. Hetzner

```bash
curl -X POST $BASE/agents/{agent_id}/vps \
  -H 'Content-Type: application/json' \
  -d '{"vps_config_id": "cccc-...", "provider": "hetzner"}'
```

Server-side: maps cpu/memory to a Hetzner server type (`cx22`, `cx32`, etc.),
generates cloud-init user data with proxy env vars, calls `servers_api::create_server`.

```json
{
  "id": "vvvv-...",
  "vps_config_id": "cccc-...",
  "name": "agent-aaaa-...",
  "provider": "hetzner",
  "state": "running",
  "address": "10.0.0.5",
  "storage_used_bytes": 0,
  "created_at": "...",
  "updated_at": "..."
}
```

Address is the Hetzner private network IP (if a network is configured).

### 3b. Fly.io

```bash
curl -X POST $BASE/agents/{agent_id}/vps \
  -H 'Content-Type: application/json' \
  -d '{"vps_config_id": "cccc-...", "provider": "fly"}'
```

Server-side: maps cpu/memory to a Fly guest config (shared/performance CPUs),
injects proxy env vars, calls the Fly Machines API to create a Machine.

```json
{
  "id": "vvvv-...",
  "vps_config_id": "cccc-...",
  "name": "agent-aaaa-...",
  "provider": "fly",
  "state": "running",
  "address": "fdaa:0:1::3.vm.cludbox-agents.internal",
  "storage_used_bytes": 0,
  "created_at": "...",
  "updated_at": "..."
}
```

Address is either the Machine's `private_ip` or `{machine_id}.vm.{app}.internal`.

## 4. Check usage

```bash
curl $BASE/agents/{agent_id}/usage
```

Both Hetzner and Fly are fixed-resource providers (bandwidth-only metering),
so `cpu` and `memory` are `null`:

```json
{
  "allowed": true,
  "bandwidth": {"used": 0, "limit": 107374182400, "exceeded": false},
  "storage":   {"used": 0, "limit": 53687091200,  "exceeded": false},
  "cpu": null,
  "memory": null
}
```

Elastic providers (Sprites, K8s — when added) will return `cpu` and `memory`
as `{"used": ..., "limit": ..., "exceeded": ...}`.

## 5. Stop the VPS

```bash
curl -X POST $BASE/agents/{agent_id}/vps/stop
```

| Provider | Backend call |
|----------|-------------|
| Hetzner  | `servers_api::shutdown_server` (graceful ACPI shutdown) |
| Fly.io   | `FlyClient::stop_machine` |

Sets VPS state to `stopped`. Returns the updated VPS object.

## 6. Start the VPS

```bash
curl -X POST $BASE/agents/{agent_id}/vps/start
```

| Provider | Backend call |
|----------|-------------|
| Hetzner  | `servers_api::power_on_server` |
| Fly.io   | `FlyClient::start_machine` |

Sets VPS state to `running`. Returns the updated VPS object.

## 7. Destroy the VPS

```bash
curl -X DELETE $BASE/agents/{agent_id}/vps
```

| Provider | Backend call |
|----------|-------------|
| Hetzner  | `servers_api::delete_server` |
| Fly.io   | `FlyClient::delete_machine` |

Best-effort destruction — if the provider call fails the DB state is still
set to `destroyed` and the agent is unlinked from the VPS.

Returns `204 No Content`.

## 8. Delete the agent

```bash
curl -X DELETE $BASE/agents/{agent_id}
```

If a VPS is still attached and not destroyed, the control plane does a
best-effort destroy (same provider lookup) before deleting the agent row.

Returns `204 No Content`.

## Provider routing

The `provider` field is only required at provisioning time (step 3).
All subsequent operations (start, stop, destroy) read the provider from the
VPS record in the database and look it up in the `ProviderRegistry`.
This means you can run some agents on Fly and others on Hetzner through the
same control plane instance.
