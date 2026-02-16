# Runbook: Slopbox from Zero

## Prerequisites

- A GitHub repo with this codebase pushed
- A domain (e.g. `slopbox.dev`) with DNS you control
- `flyctl` CLI installed (`brew install flyctl` / `curl -L https://fly.io/install.sh | sh`)
- A Hetzner Cloud account (https://console.hetzner.cloud)
- `hcloud` CLI installed (`brew install hcloud`)

---

## Phase 1: PostgreSQL on Fly.io

```bash
# 1. Log in to Fly
fly auth login

# 2. Create the Postgres cluster (single-node dev, iad region)
fly postgres create --name slopbox-db --region iad --vm-size shared-cpu-1x --initial-cluster-size 1 --volume-size 1

# Save the connection string it prints — you'll need it.
# Format: postgres://postgres:<password>@slopbox-db.flycast:5432/postgres?sslmode=disable

# 3. Create the application database
fly postgres connect -a slopbox-db -c "CREATE DATABASE slopbox;"
```

---

## Phase 2: Fly.io App for the Control Plane

```bash
# 1. Create the app (must match fly.toml app name)
fly apps create slopbox-api --org personal

# 2. Attach Postgres (creates DATABASE_URL secret automatically)
fly postgres attach slopbox-db -a slopbox-api --database-name slopbox

# 3. Set required secrets
fly secrets set -a slopbox-api \
  JWT_SECRET="$(openssl rand -hex 32)" \
  HETZNER_API_TOKEN="<from Phase 3>" \
  FRONTEND_ORIGIN="https://your-frontend.vercel.app"

# 4. First deploy (from backend/ directory)
cd backend
fly deploy --remote-only
```

After deploy, verify:
```bash
fly logs -a slopbox-api   # should see "listening on 0.0.0.0:8080"
```

---

## Phase 3: Hetzner Cloud Project

```bash
# 1. Create an API token
#    Hetzner Console → your project → Security → API Tokens → Generate API Token
#    Permission: Read & Write
#    Save the token.

# 2. (Optional) Create a firewall
#    Allow inbound: 22/tcp (SSH for debug), 18789/tcp (gateway, from Fly private network only)
#    Allow outbound: all (agents need internet via forward proxy)

# 3. (Optional) Upload an SSH key for emergency debug access
#    Hetzner Console → Security → SSH Keys → Add SSH Key

# 4. (Optional) Create a private network
#    Hetzner Console → Networks → Create Network
#    Subnet: e.g. 10.0.0.0/16 in your chosen location (fsn1)
```

### Build the Hetzner Snapshot (cuts agent boot time from ~3 min to ~30 sec)

```bash
# 1. Create a temporary server
hcloud server create --name snapshot-builder --type cpx11 --image ubuntu-24.04 --location fsn1

# 2. SSH in and install dependencies
hcloud server ssh snapshot-builder

# On the server:
apt-get update && apt-get install -y docker.io
systemctl enable docker && systemctl start docker
curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
apt-get install -y nodejs
npm install -g @anthropic/openclaw
# Verify
openclaw --version && docker --version && node --version
# Clean up apt cache to shrink snapshot
apt-get clean && rm -rf /var/lib/apt/lists/*
exit

# 3. Power off and snapshot
hcloud server shutdown snapshot-builder
# Wait for it to stop
hcloud server create-image --type snapshot --description "openclaw-base-v1" snapshot-builder
# Note the snapshot ID (e.g. 12345678)

# 4. Delete the builder
hcloud server delete snapshot-builder
```

### Update the seed migration to use the snapshot

Edit `backend/crates/cb-db/migrations/014_seed_sprites_demo.sql` — change the Hetzner `image` from `ubuntu-24.04` to the snapshot ID:

```sql
INSERT INTO vps_configs (name, provider, image, cpu_millicores, memory_mb, disk_gb)
VALUES ('hetzner-standard', 'hetzner', '<SNAPSHOT_ID>', 2000, 2048, 20)
ON CONFLICT DO NOTHING;
```

Or update it after migrations run:
```sql
UPDATE vps_configs SET image = '<SNAPSHOT_ID>' WHERE name = 'hetzner-standard';
```

Then set the Hetzner secrets on Fly:
```bash
fly secrets set -a slopbox-api \
  HETZNER_API_TOKEN="<your-token>" \
  HETZNER_LOCATION="fsn1"

# Optional, if you created these:
fly secrets set -a slopbox-api \
  HETZNER_FIREWALL_ID="<firewall-id>" \
  HETZNER_NETWORK_ID="<network-id>" \
  HETZNER_SSH_KEY_NAMES="<key-name>"
```

---

## Phase 4: GitHub Actions (CI + CD)

Three files were created in the repo:

| File | Purpose |
|---|---|
| `backend/rust-toolchain.toml` | Pins Rust 1.93.1 |
| `.github/workflows/ci.yml` | PR + push-to-main: fmt, clippy, check |
| `.github/workflows/deploy-backend.yml` | Push-to-main: `fly deploy --remote-only` |

```bash
# 1. Generate a Fly deploy token
fly tokens create deploy -a slopbox-api

# 2. Add it to GitHub
#    Repo → Settings → Secrets and variables → Actions → New repository secret
#    Name:  FLY_API_TOKEN
#    Value: <the token from step 1>
```

After this, every push to `main` that touches `backend/**` will auto-deploy.

---

## Phase 5: Frontend (Vercel)

Not managed by GitHub Actions. Connect the repo to Vercel:

1. Vercel Dashboard → Add New Project → Import your GitHub repo
2. Set root directory to `frontend/`
3. Set environment variables:
   - `CONTROL_PLANE_API_KEY` — shared secret for BFF → backend auth
   - `NEXTAUTH_SECRET` — `openssl rand -hex 32`
   - `NEXTAUTH_URL` — `https://your-app.vercel.app`
   - `CONTROL_PLANE_URL` — `https://slopbox-api.fly.dev`
4. Vercel auto-deploys on push to `main`

Also set the matching secret on the backend:
```bash
fly secrets set -a slopbox-api CONTROL_PLANE_API_KEY="<same-key-as-vercel>"
```

---

## All Secrets Summary

### Fly.io Secrets (`fly secrets set -a slopbox-api`)

| Secret | Source | Required |
|---|---|---|
| `DATABASE_URL` | Auto-set by `fly postgres attach` | Yes |
| `JWT_SECRET` | `openssl rand -hex 32` | Yes |
| `HETZNER_API_TOKEN` | Hetzner Console → API Tokens | Yes |
| `HETZNER_LOCATION` | Datacenter code (default: `fsn1`) | No |
| `HETZNER_FIREWALL_ID` | Hetzner Console → Firewalls | No |
| `HETZNER_NETWORK_ID` | Hetzner Console → Networks | No |
| `HETZNER_SSH_KEY_NAMES` | Hetzner Console → SSH Keys | No |
| `FRONTEND_ORIGIN` | Your Vercel URL | Yes (for CORS) |
| `CONTROL_PLANE_API_KEY` | Shared with frontend | Yes (for BFF auth) |

### GitHub Secrets (Repo → Settings → Secrets)

| Secret | Source |
|---|---|
| `FLY_API_TOKEN` | `fly tokens create deploy -a slopbox-api` |

### Vercel Environment Variables

| Variable | Source |
|---|---|
| `CONTROL_PLANE_API_KEY` | Same as Fly secret |
| `NEXTAUTH_SECRET` | `openssl rand -hex 32` |
| `NEXTAUTH_URL` | Your Vercel URL |
| `CONTROL_PLANE_URL` | `https://slopbox-api.fly.dev` |

---

## Verification Checklist

```bash
# Backend is running
curl https://slopbox-api.fly.dev          # should respond (or 404, not connection refused)

# Logs show successful startup
fly logs -a slopbox-api
# Look for: "listening on 0.0.0.0:8080"
# Look for: registered providers: [Hetzner]

# Database migrations ran
fly postgres connect -a slopbox-db -d slopbox -c "SELECT name FROM plans;"
# Should show: demo

# CI runs on PR
# Push a branch touching backend/, open PR → green check

# CD runs on merge
# Merge to main → deploy-backend workflow triggers → green check
```
