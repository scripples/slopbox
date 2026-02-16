# Red Team Security Analysis -- cb-sidecar Deep Dive (OBSOLETE)

> **RESOLVED 2026-02-16:** The cb-sidecar crate has been removed from the
> codebase. All findings in this document are no longer applicable. Config
> targeting is now handled via gateway-native RPC (config.patch, /tools/invoke).
> The sidecar's attack surface (SIDE-CRIT-001 through SIDE-CRIT-005) no longer
> exists. No privileged daemon runs on VMs besides the OpenClaw gateway itself.

**Date**: 2026-02-15
**Scope**: Focused adversarial analysis of `cb-sidecar` (168 lines, single `main.rs`), its interaction surface with the agent, the control plane, and the VM environment.
**Analyst**: Red Team Agent (Claude Opus 4.6)
**Mode**: Adversarial -- every defensive assumption is challenged.

---

## Executive Summary

The cb-sidecar is the most security-critical 168 lines in the entire Slopbox platform. It runs as root, outside the Docker sandbox, on each VM. It can write arbitrary files to the VM filesystem and restart the gateway service. It is authenticated only by a static Bearer token over plaintext HTTP. It listens on `0.0.0.0:9090`, reachable from any process on the VM.

**The sidecar is not just an administrative convenience -- it is an escalation primitive.** Any attacker who can reach port 9090 with the correct Bearer token gains root-equivalent write access to the entire VM. And as this analysis will demonstrate, the agent has multiple viable paths to reach the sidecar, even from inside the Docker sandbox configured with `network=none`.

**Total findings: 5 Critical, 4 High, 4 Medium, 3 Low, 2 Informational.**

---

## Critical Findings

### [SIDE-CRIT-001] Sidecar Reachable from Gateway Process via Loopback -- Agent Sandbox Escape

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:159` (bind `0.0.0.0:9090`)
- **Category**: Sandbox Escape / Privilege Escalation
- **Description**: The sidecar binds to `0.0.0.0:9090`. The OpenClaw gateway process runs on the same VM, outside the Docker sandbox. The agent executes tools inside a Docker container with `network=none`, but the gateway process itself is unsandboxed. The agent interacts with the gateway via its RPC protocol. If the agent can cause the gateway to make an HTTP request to `127.0.0.1:9090`, the sidecar is directly reachable.

- **Attack Scenario**:
  1. The agent is an AI with tool access. The OpenClaw gateway provides tools like `web_fetch`, `Bash` (if not denied), or custom MCP tools.
  2. The `tools.deny` list in the default config denies `["gateway", "nodes"]` (see `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/openclaw_config.rs:91-93`). However, `web_fetch` and other network tools are NOT denied.
  3. `web_fetch` (or any HTTP tool) executes **in the gateway process context**, not inside the Docker sandbox. The gateway process has full loopback access.
  4. The agent instructs: "Fetch `http://127.0.0.1:9090/health`" -- this succeeds because the gateway process is not sandboxed at the network level.
  5. The agent confirms the sidecar is alive. Now it needs the Bearer token.

- **Exploitability**: The agent can reach the sidecar from inside its sandbox via gateway-process-level tools. The only remaining barrier is obtaining the `SIDECAR_SECRET`. See SIDE-CRIT-002 for how this barrier falls.

- **Impact**: If combined with SIDE-CRIT-002, this provides the agent with full root write access to the VM filesystem and the ability to restart services. Complete sandbox escape.

- **Evidence**: The sidecar binds `0.0.0.0`:
  ```rust
  // main.rs:159
  let addr = "0.0.0.0:9090";
  ```
  The gateway listens on `0.0.0.0:18789` (same host):
  ```rust
  // openclaw_config.rs:117
  bind: "0.0.0.0:18789".into(),
  ```
  Both processes share the same network namespace (the VM host). The Docker sandbox has `network=none`, but the gateway itself is not inside Docker.

- **Recommendation**: Bind the sidecar to a Unix socket or a non-routable address that is explicitly excluded from the gateway's network access. Add firewall rules (iptables) to block the gateway user from connecting to port 9090. Run the gateway as a non-root user and use iptables owner matching to enforce this.

---

### [SIDE-CRIT-002] SIDECAR_SECRET Exposed in VM Environment Variables -- Agent Can Read It

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/routes/vps.rs:91-93` (env injection)
- **Category**: Secret Exposure / Credential Theft
- **Description**: The `SIDECAR_SECRET` is injected as an environment variable into the VPS during provisioning. On the VM, this secret is available to **any process** that can read `/proc/1/environ`, `/etc/slopbox/env` (Hetzner), or the environment of any process that inherits it.

- **Attack Scenario** (continuing from SIDE-CRIT-001):
  1. The agent has already confirmed the sidecar is reachable at `127.0.0.1:9090` via `web_fetch`.
  2. The agent uses a Bash tool (if available, even inside Docker sandbox):
     ```bash
     cat /proc/1/environ | tr '\0' '\n' | grep SIDECAR_SECRET
     ```
     This fails from inside Docker (PID namespace isolation). But...
  3. The agent uses `web_fetch` to read `http://127.0.0.1:9090/health` (unauthenticated). This proves the sidecar is alive.
  4. The agent can potentially read the environment via the gateway process itself. If OpenClaw has any tool that can read the host process environment, read arbitrary files on the host, or inspect process state, the secret is exposed.
  5. **Even without reading the env directly**: On Hetzner, the secret is written to `/etc/slopbox/env` via cloud-init (see `hetzner.rs:90`):
     ```
     echo 'SIDECAR_SECRET=<value>' >> /etc/slopbox/env
     ```
     If the Docker container mounts any host path, or if the agent can read host files through any tool that runs in the gateway process (not the container), the file `/etc/slopbox/env` is readable.
  6. The gateway token itself is also in the same env file and is used for proxy auth. The agent already knows this token (it is part of the proxy authentication it uses for outbound traffic). If the agent can brute-force or side-channel the sidecar secret, the game is over.

- **Key Insight**: The `SIDECAR_SECRET` and `OPENCLAW_GATEWAY_TOKEN` are both injected as env vars in the same `HashMap<String, String>` at `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/routes/vps.rs:84-103`. On Hetzner, they are all written to `/etc/slopbox/env` via cloud-init. The gateway process inherits these env vars and passes them to child processes. The Docker sandbox may or may not inherit the full host environment depending on how OpenClaw configures Docker.

- **Impact**: If the SIDECAR_SECRET is readable by the agent (directly or via any gateway-process-level tool), the agent has full control over the sidecar: arbitrary file writes, config overwrites, and service restarts.

- **Evidence**: The env var injection in `vps.rs:84-103`:
  ```rust
  let mut env = HashMap::new();
  env.insert("OPENCLAW_GATEWAY_TOKEN".into(), agent.gateway_token.clone());
  if let Some(ref secret) = vps.sidecar_secret {
      env.insert("SIDECAR_SECRET".into(), secret.clone());
  }
  env.insert("HTTP_PROXY".into(), proxy_url.clone());
  env.insert("HTTPS_PROXY".into(), proxy_url.clone());
  // ...all go into the same env map
  ```

- **Recommendation**:
  - Do NOT inject `SIDECAR_SECRET` into the general VM environment. Write it to a file readable only by the sidecar process (e.g., `/etc/slopbox/sidecar.secret` with `chmod 600` owned by a dedicated sidecar user).
  - Run the sidecar as a separate user (not root), and ensure the gateway process cannot read the sidecar's secret file.
  - On Hetzner, split the cloud-init so that sidecar-specific secrets are written to a separate file with restricted permissions.

---

### [SIDE-CRIT-003] PUT /files/{*path} Enables Arbitrary Root-Level File Writes -- Config Rewrite Attack

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:79-102`
- **Category**: Arbitrary File Write / Sandbox Escape / Privilege Escalation
- **Description**: The `/files/{*path}` endpoint writes arbitrary bytes to any absolute path on the filesystem. It runs as root. The only validation is rejecting paths containing `..`. This means an authenticated caller can write to:
  - `/root/.openclaw/openclaw.json` -- disable sandbox entirely
  - `/etc/systemd/system/openclaw.service` -- replace the service unit
  - `/usr/local/bin/openclaw` -- replace the gateway binary
  - `/etc/crontab` -- schedule arbitrary commands
  - `/root/.ssh/authorized_keys` -- add SSH access
  - `/etc/shadow` -- change root password
  - Any other file on the filesystem

- **Attack Scenario** (the full chain from agent to root):
  1. Agent uses `web_fetch` to confirm sidecar at `http://127.0.0.1:9090/health` (SIDE-CRIT-001).
  2. Agent reads `SIDECAR_SECRET` from gateway env or `/etc/slopbox/env` (SIDE-CRIT-002).
  3. Agent uses `web_fetch` to `PUT http://127.0.0.1:9090/files/root/.openclaw/openclaw.json` with Bearer auth and a modified config:
     ```json
     {
       "agents": {
         "defaults": {
           "sandbox": { "mode": "off" },
           "workspace": "/"
         }
       },
       "tools": {
         "profile": "default",
         "deny": [],
         "elevated": { "enabled": true }
       }
     }
     ```
  4. Agent uses `web_fetch` to `POST http://127.0.0.1:9090/restart-gateway` with Bearer auth.
  5. Gateway restarts with no sandbox. Agent now has unrestricted root access.
  6. Agent can install persistence, exfiltrate all secrets, pivot to other infrastructure.

- **Impact**: Complete VM takeover. Root shell. Full access to all secrets, environment variables, and the ability to impersonate the agent on the proxy (or contact other VPS sidecars if on the same network).

- **Evidence**:
  ```rust
  // main.rs:79-102
  async fn write_file(
      State(state): State<AppState>,
      headers: HeaderMap,
      axum::extract::Path(file_path): axum::extract::Path<String>,
      body: Bytes,
  ) -> Result<StatusCode, StatusCode> {
      authorize(&state, &headers)?;
      if file_path.contains("..") {
          return Err(StatusCode::BAD_REQUEST);
      }
      let abs_path = format!("/{file_path}");
      let path = Path::new(&abs_path);
      if let Some(parent) = path.parent() {
          std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
      }
      std::fs::write(path, &body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
      Ok(StatusCode::NO_CONTENT)
  }
  ```
  No allowlisting. No path restriction beyond `..`. Root privileges. Creates parent directories as needed.

- **Recommendation**:
  - Restrict `/files/*` to an explicit allowlist of paths (e.g., `/root/.openclaw/workspace/*` and `/root/.openclaw/openclaw.json`).
  - Alternatively, scope all writes to a chroot or specific directory prefix.
  - The control plane already has an allowlist in `config.rs:16-24` for workspace files. Apply a similar principle at the sidecar level as defense-in-depth.
  - Drop root privileges after startup (bind port, then drop to a non-root user).

---

### [SIDE-CRIT-004] Config Rewrite via Sidecar Disables All Security Controls

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:56-75` (PUT /config)
- **Category**: Security Control Bypass
- **Description**: The `PUT /config` endpoint accepts any valid JSON and writes it to `/root/.openclaw/openclaw.json`. Combined with `POST /restart-gateway`, this allows complete reconfiguration of the OpenClaw agent. The current config enforces:
  - `sandbox.mode: "all"` -- all tool calls sandboxed
  - `sandbox.scope: "agent"` -- agent-level isolation
  - `docker.network: "none"` -- no network in sandbox
  - `tools.deny: ["gateway", "nodes"]` -- blocked tool categories
  - `elevated.enabled: false` -- no privilege escalation

  An attacker who controls the sidecar can set:
  - `sandbox.mode: "off"` -- disable sandboxing
  - `tools.deny: []` -- allow all tools
  - `elevated.enabled: true` -- enable privilege escalation
  - `docker.network: "bridge"` -- give sandbox network access

- **Attack Scenario**: Same as SIDE-CRIT-003 step 3-5, but using `/config` instead of `/files/*`.

- **Impact**: The sidecar's config write endpoint is a direct gateway to disabling every security boundary the platform enforces on the agent.

- **Evidence**: The `/config` handler performs no schema validation beyond "is it valid JSON":
  ```rust
  // main.rs:64-65
  serde_json::from_slice::<serde_json::Value>(&body)
      .map_err(|_| StatusCode::BAD_REQUEST)?;
  ```

- **Recommendation**:
  - Validate the config against the expected OpenClaw schema.
  - Enforce that critical security fields (`sandbox.mode`, `tools.deny`, `elevated.enabled`, `docker.network`) cannot be weakened via sidecar writes.
  - The sidecar should reject configs that disable sandbox mode or remove mandatory deny-list entries.
  - Consider making the sidecar read-only for security-critical config fields, requiring control plane signature verification.

---

### [SIDE-CRIT-005] Forward Proxy SSRF Enables Agent to Reach Any VPS Sidecar

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/proxy.rs:196-231` (CONNECT handler)
- **Category**: SSRF / Cross-Tenant Attack / Lateral Movement
- **Description**: The forward proxy (`cb-api/src/proxy.rs`) handles `CONNECT` requests with no destination filtering. The agent can tunnel TCP to any IP:port reachable from the control plane, including other VPS sidecars on the same VPC network. While sidecars are protected by their individual `SIDECAR_SECRET`, the combination of SSRF + unauthenticated health/metrics endpoints enables reconnaissance and potential cross-tenant attacks.

- **Attack Scenario**:
  1. Agent uses its proxy credentials to issue `CONNECT <other-vps-ip>:9090`.
  2. The proxy establishes a TCP tunnel to the target VPS's sidecar.
  3. Agent hits `GET /health` (unauthenticated) to confirm sidecar is alive and learn gateway status.
  4. Agent hits `GET /metrics` (unauthenticated) -- currently a stub but could leak information in the future.
  5. If the agent can obtain any other VPS's sidecar secret (e.g., via control plane compromise, DB leak, or cloud-init log exposure), it can write files and restart the gateway on another tenant's VM.
  6. Even without the secret, the agent can attempt timing attacks against the Bearer token comparison (see SIDE-HIGH-001).

- **Impact**: Cross-tenant reconnaissance at minimum. Full cross-tenant VM takeover if any sidecar secret is obtained.

- **Evidence**: The proxy CONNECT handler has no destination filtering:
  ```rust
  // proxy.rs:207
  let target = match TcpStream::connect(&host).await {
      Ok(s) => s,
      // ...
  };
  ```
  No blocklist for private IPs, metadata endpoints, or internal service ports.

- **Recommendation**:
  - Block connections to RFC 1918/RFC 6598 addresses, link-local (169.254.x.x), loopback, and the control plane's own addresses.
  - Block connections to port 9090 (sidecar port) entirely.
  - Maintain an explicit allowlist of permitted destination ranges, or at minimum a blocklist of dangerous targets.

---

## High Findings

### [SIDE-HIGH-001] Bearer Token Comparison Vulnerable to Timing Attack

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:26`
- **Category**: Cryptographic Weakness / Authentication Bypass
- **Description**: The sidecar uses `!=` (string inequality) for Bearer token comparison. This is a non-constant-time comparison that leaks information about how many bytes of the secret match.

  ```rust
  // main.rs:26
  if header != state.secret {
      return Err(StatusCode::UNAUTHORIZED);
  }
  ```

- **Attack Scenario**: An attacker with network access to the sidecar can send repeated authentication attempts, measuring response times to determine how many prefix bytes are correct. With enough measurements, the full 64-character hex token can be recovered byte-by-byte. This requires ~256 requests per character position, or approximately 16,384 requests total. Over a local network (same VM or same VPC), the timing signal is strong enough to be exploitable.

- **Impact**: Complete authentication bypass. If the sidecar is reachable (SIDE-CRIT-001 or SIDE-CRIT-005), the token can be recovered without needing to read environment variables.

- **Exploitability**: Moderate. Requires low-latency network access and multiple requests. Over loopback (127.0.0.1), timing differences are measurable. Over VPC, noisier but still potentially exploitable with statistical analysis. The 64-character hex token provides 256 bits of entropy, but timing attacks reduce this to ~16K requests.

- **Recommendation**: Use `subtle::ConstantTimeEq` or `ring::constant_time::verify_slices_are_equal` for token comparison. This is a one-line fix.

---

### [SIDE-HIGH-002] Unauthenticated Health and Metrics Endpoints Enable Reconnaissance

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:41-52, 132-137`
- **Category**: Information Disclosure
- **Description**: The `/health` and `/metrics` endpoints require no authentication. While `/metrics` is currently a stub, `/health` reveals:
  - Whether the sidecar is alive (the sidecar exists on this VM)
  - Whether the OpenClaw gateway is running (`openclaw_running: true/false`)

  This information is useful for:
  - Confirming a target VM is a Slopbox agent
  - Determining whether the gateway has been restarted or is down
  - Fingerprinting the service for targeted attacks

- **Attack Scenario**: An attacker on the same VPC (or an agent using the SSRF in SIDE-CRIT-005) can scan for sidecar instances on port 9090 and check which VMs have running gateways.

- **Impact**: Facilitates targeting of cross-tenant attacks. Useful in attack chain reconnaissance.

- **Recommendation**: Require authentication for all endpoints, or restrict `/health` to return only a boolean `ok` without process state details.

---

### [SIDE-HIGH-003] No TLS -- Sidecar Secret Transmitted in Plaintext

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/sidecar_client.rs:31` and `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:159`
- **Category**: Credential Exposure / Transport Security
- **Description**: The sidecar communicates over plaintext HTTP. The `SidecarClient` in the control plane sends the `SIDECAR_SECRET` in the `Authorization: Bearer` header over unencrypted HTTP:

  ```rust
  // sidecar_client.rs:31
  fn url(address: &str, path: &str) -> String {
      format!("http://{address}:9090{path}")
  }
  ```

  This means:
  - Any network-level observer (switch, router, VPC peer, cloud provider employee) can read the sidecar secret.
  - Any agent that can sniff traffic on the VM's network interface can capture the secret when the control plane sends a config update.
  - ARP spoofing within a VPC could redirect sidecar traffic.

- **Impact**: The sidecar secret -- the sole authentication mechanism -- is exposed to any network-level attacker. On shared infrastructure (VPC, cloud network), this is a realistic threat.

- **Evidence**: The sidecar listens on plain TCP with no TLS:
  ```rust
  // main.rs:162-163
  let listener = tokio::net::TcpListener::bind(addr)
      .await
      .expect("failed to bind listener");
  ```
  No `rustls`, `native-tls`, or any TLS configuration anywhere in the sidecar crate.

- **Recommendation**:
  - Add mTLS between the control plane and sidecar. The control plane can provision a per-VM TLS certificate during cloud-init.
  - At minimum, use a WireGuard tunnel or SSH tunnel for the control plane to sidecar connection.
  - If TLS is truly impractical, use HMAC-signed request bodies with nonces to prevent replay.

---

### [SIDE-HIGH-004] `..` Path Traversal Check is Insufficient -- Symlink Following

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:88`
- **Category**: Path Traversal / Arbitrary File Write
- **Description**: The path traversal check rejects paths containing the literal string `..`. However, this check is insufficient because:

  1. **Symlink following**: `std::fs::write` follows symlinks. If an attacker can create a symlink at any writable path (e.g., via a previous write through the sidecar), they can redirect writes to arbitrary locations. For example:
     - Write a symlink at `/root/.openclaw/workspace/link` pointing to `/etc/shadow`
     - Then write to `/root/.openclaw/workspace/link` to overwrite `/etc/shadow`

  2. **The check is string-based, not path-based**: While `..` rejection does prevent the obvious `../../etc/passwd` attack, it does not prevent:
     - Absolute path writes (which are the default behavior -- the endpoint prepends `/` to the path)
     - Writes to sensitive system directories (`/etc/`, `/usr/`, `/var/`, etc.)

  3. **The check is redundant**: The real vulnerability is not `..` traversal -- it is that the endpoint already writes to ANY absolute path. The `..` check is security theater; the attacker does not need `..` when they can directly specify `/etc/shadow` as the path.

- **Impact**: The `..` check provides a false sense of security. The endpoint is fundamentally an arbitrary file write endpoint with no meaningful path restrictions.

- **Evidence**:
  ```rust
  // main.rs:88-92
  if file_path.contains("..") {
      return Err(StatusCode::BAD_REQUEST);
  }
  let abs_path = format!("/{file_path}");
  ```
  The path `/etc/shadow` does not contain `..` and will be written successfully.

- **Recommendation**: Replace the `..` check with a strict allowlist of permitted path prefixes (e.g., `/root/.openclaw/`). Additionally, use `std::fs::canonicalize()` on the parent path before writing to prevent symlink-based escapes from the allowed prefix.

---

## Medium Findings

### [SIDE-MED-001] tools_deny User-Overridable via UpdateConfigRequest

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/dto.rs:191-194` and `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/openclaw_config.rs:91-93`
- **Category**: Security Control Bypass
- **Description**: The `UpdateConfigRequest` DTO accepts a `tools_deny` field. If the user (or a compromised BFF) sends `tools_deny: []`, the resulting config will have an empty deny list, allowing the agent to use `gateway` and `nodes` tools that are denied by default. The `gateway` tool gives the agent direct access to gateway internals, potentially including the ability to read environment variables.

  ```rust
  // openclaw_config.rs:91-93
  let deny = params.tools_deny.clone().unwrap_or_else(|| {
      vec!["gateway".into(), "nodes".into()]
  });
  ```

  The default deny list (`["gateway", "nodes"]`) is only applied when `tools_deny` is `None`. A user sending `tools_deny: Some(vec![])` overrides it completely.

- **Impact**: A user (or attacker with the API key) can remove all tool restrictions, giving the agent access to gateway management tools that could expose secrets or provide network access from the gateway process context.

- **Recommendation**: Enforce a minimum deny list that always includes security-critical tools. The user-provided list should be merged with (not replace) the mandatory deny list.

---

### [SIDE-MED-002] Sidecar Has No Request Size Limits -- Disk Exhaustion via Large Writes

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:79-102`
- **Category**: Denial of Service
- **Description**: Neither the `/config` nor `/files/*` endpoints impose any request body size limit. An attacker (or a misbehaving control plane) can send multi-gigabyte payloads that exhaust the VM's disk space.

  The sidecar uses `axum::body::Bytes` for body extraction with no `Content-Length` limit or body size cap. On a small VM (e.g., Hetzner CPX11 with 20GB disk), a single large write can fill the disk and crash the gateway.

- **Impact**: VM denial of service. The gateway will fail to start, write logs, or function if the disk is full.

- **Recommendation**: Add body size limits using `axum::extract::DefaultBodyLimit` or manual `Content-Length` checks. A reasonable limit for config files is 1MB; for workspace files, 10MB.

---

### [SIDE-MED-003] Blocking File I/O in Async Context -- DoS via Slow Writes

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:69-72, 96-99`
- **Category**: Denial of Service
- **Description**: The sidecar uses synchronous `std::fs::write()` and `std::fs::create_dir_all()` inside async handler functions. These calls block the tokio runtime thread. On a single-threaded runtime (or under concurrent load), this can stall the entire sidecar.

  While the README acknowledges this ("acceptable for low-frequency config updates"), a malicious caller could send many concurrent requests to exhaust the thread pool.

- **Impact**: Sidecar becomes unresponsive, preventing the control plane from managing the VM. Could be used to create a window during which the agent operates without monitoring or enforcement.

- **Recommendation**: Use `tokio::fs` for file operations, or wrap sync calls in `tokio::task::spawn_blocking`.

---

### [SIDE-MED-004] systemctl restart via Sidecar Enables Abuse Timing Window

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:106-122`
- **Category**: Race Condition / Timing Attack
- **Description**: The `POST /restart-gateway` endpoint calls `systemctl restart openclaw`. Between the gateway shutdown and the new gateway startup, there is a window where:
  1. The agent is not running (no monitoring, no billing, no sandbox enforcement).
  2. If the agent has already written a malicious config via `/config` or `/files/*`, the restarted gateway will load that config.
  3. The restart is synchronous -- the sidecar waits for systemctl to return. During this time, the sidecar is blocked on that request but still serving others.

- **Attack Scenario**: An attacker chains: write malicious config -> restart gateway. The new gateway starts with disabled sandbox, elevated privileges, and no tool restrictions.

- **Impact**: Part of the SIDE-CRIT-003/SIDE-CRIT-004 attack chain. The restart is the trigger that activates a malicious config.

- **Recommendation**:
  - Validate config before restart (defense-in-depth at sidecar level).
  - After restart, have the sidecar verify the running config matches the expected config.
  - Log restart events to an external audit log that the agent cannot modify.

---

## Low Findings

### [SIDE-LOW-001] pgrep-Based Health Check is Spoofable

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:42-46`
- **Category**: Monitoring Integrity
- **Description**: The health check uses `pgrep -x openclaw` to determine if the gateway is running. If an attacker has gained code execution on the VM (via the attack chain above), they can create a process named `openclaw` that keeps the health check returning `true` while the actual gateway is stopped or replaced.

  ```rust
  let openclaw_running = Command::new("pgrep")
      .arg("-x")
      .arg("openclaw")
      .output()
      .is_ok_and(|o| o.status.success());
  ```

- **Impact**: The control plane's health monitoring is unreliable as an indicator of gateway integrity.

- **Recommendation**: Check the specific PID from the systemd service (e.g., `systemctl is-active openclaw`) and verify the binary's hash matches the expected value.

---

### [SIDE-LOW-002] Metrics Endpoint is a Stub -- No Resource Monitoring

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs:126-137`
- **Category**: Monitoring Gap
- **Description**: The `/metrics` endpoint returns `null` for all metrics. The CLAUDE.md requirement 4a states "VPS usage statistics be properly monitorable FROM OUTSIDE OF the sandboxed environment." This stub violates that requirement. The `StubCollector` in the monitor (`/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/monitor.rs:26-37`) simply echoes back the existing DB values, meaning no actual metrics collection occurs.

- **Impact**: No CPU or memory usage enforcement. The agent can consume unlimited CPU and memory without detection. Only bandwidth is metered (via the proxy).

- **Recommendation**: Implement actual metrics collection by reading `/proc` or using cgroup stats, reported via the sidecar to the control plane.

---

### [SIDE-LOW-003] No Audit Logging of Sidecar Operations

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs` (entire file)
- **Category**: Audit / Forensics
- **Description**: The sidecar logs only its startup message (`tracing::info!(addr, "starting sidecar")`). No handler logs any operation -- file writes, config updates, restarts, or authentication failures. If an attacker uses the sidecar to rewrite config and restart the gateway, there is no record of this on the VM (the agent could delete logs after gaining root access).

- **Impact**: No forensic trail for security incidents. Attackers can operate through the sidecar without detection.

- **Recommendation**:
  - Log all authenticated operations (config writes, file writes, restarts) with timestamp, path, and body hash.
  - Forward logs to an external logging service (not on the VM) that the agent cannot tamper with.

---

## Informational Findings

### [SIDE-INFO-001] Sidecar Runs as Root -- Excessive Privilege

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-sidecar/src/main.rs` (runs as PID 1 or systemd service as root)
- **Category**: Principle of Least Privilege
- **Description**: The sidecar runs as root to write to `/root/.openclaw/` and run `systemctl`. This is more privilege than necessary. The sidecar could bind its port, then drop privileges to a dedicated user that has write access only to the OpenClaw config directory and `systemctl restart openclaw` via sudoers.

- **Impact**: If any vulnerability in axum, tokio, or the sidecar itself allows RCE, the attacker immediately has root access. With privilege dropping, the blast radius would be limited.

- **Recommendation**: Create a `slopbox-sidecar` user with minimal permissions. Bind port 9090 before dropping privileges. Use polkit or sudo for `systemctl restart openclaw` only.

---

### [SIDE-INFO-002] SidecarClient Does Not Verify TLS or Server Identity

- **Location**: `/home/mr-idiot/claude/slopbox/backend/crates/cb-api/src/sidecar_client.rs:24-28`
- **Category**: Transport Security
- **Description**: The `SidecarClient` uses `reqwest::Client::new()` with no TLS configuration, no certificate pinning, and no server identity verification. Even if TLS were added to the sidecar in the future, the client would accept any certificate unless explicitly configured.

  ```rust
  pub fn new() -> Self {
      Self {
          http: reqwest::Client::new(),
      }
  }
  ```

- **Impact**: Informational -- there is no TLS to verify currently. But this should be addressed when TLS is implemented.

- **Recommendation**: When adding TLS, configure the client with certificate pinning or a custom CA to prevent MITM attacks within the VPC.

---

## Attack Chain Summary

The most dangerous chain through the sidecar is a **five-step sandbox escape from AI agent to root**:

```
Step 1: Agent uses web_fetch tool to probe http://127.0.0.1:9090/health
        (gateway process runs unsandboxed, can reach loopback)
        [SIDE-CRIT-001]

Step 2: Agent reads SIDECAR_SECRET from gateway process environment
        or /etc/slopbox/env via any host-context tool
        [SIDE-CRIT-002]

Step 3: Agent uses web_fetch to PUT /config with sandbox disabled
        [SIDE-CRIT-004]

Step 4: Agent uses web_fetch to POST /restart-gateway
        [SIDE-MED-004]

Step 5: Gateway restarts with no sandbox. Agent has root access.
        Agent uses PUT /files/* to write SSH keys, cron jobs,
        or replace system binaries.
        [SIDE-CRIT-003]
```

**Defense-in-depth failures**:
- The Docker sandbox (`network=none`) protects the agent's Bash/code execution from network access, but `web_fetch` and similar tools execute in the gateway process, which is unsandboxed.
- The sidecar secret is a single point of failure for VM security, and it is stored alongside other env vars with no additional protection.
- The sidecar accepts any valid JSON as config and any absolute path for file writes, with no schema validation or path restrictions.
- There is no TLS, no constant-time auth, and no audit trail.

---

## Recommendations Priority Matrix

| Priority | Finding | Effort | Impact |
|----------|---------|--------|--------|
| **P0 -- Immediate** | SIDE-CRIT-001: Bind sidecar to restricted interface, firewall rules | Medium | Blocks entire attack chain |
| **P0 -- Immediate** | SIDE-CRIT-002: Isolate SIDECAR_SECRET from gateway env | Medium | Prevents credential theft |
| **P0 -- Immediate** | SIDE-CRIT-003: Allowlist permitted paths in /files/* | Low | Prevents arbitrary file writes |
| **P0 -- Immediate** | SIDE-CRIT-004: Validate config schema, enforce security fields | Medium | Prevents config bypass |
| **P0 -- Immediate** | SIDE-CRIT-005: Add destination filtering to forward proxy | Medium | Prevents cross-tenant SSRF |
| **P1 -- This Sprint** | SIDE-HIGH-001: Constant-time token comparison | Low | Prevents timing attacks |
| **P1 -- This Sprint** | SIDE-HIGH-002: Auth-gate health/metrics endpoints | Low | Reduces reconnaissance |
| **P1 -- This Sprint** | SIDE-HIGH-003: Add TLS to sidecar | High | Prevents credential sniffing |
| **P1 -- This Sprint** | SIDE-HIGH-004: Replace `..` check with path allowlist | Low | Defense-in-depth |
| **P2 -- Next Sprint** | SIDE-MED-001: Enforce minimum tools_deny list | Low | Prevents user self-sabotage |
| **P2 -- Next Sprint** | SIDE-MED-002: Add body size limits | Low | Prevents disk exhaustion |
| **P2 -- Next Sprint** | SIDE-MED-003: Use async file I/O | Low | Prevents sidecar DoS |
| **P2 -- Next Sprint** | SIDE-MED-004: Validate config before restart | Medium | Prevents malicious restarts |
| **P3 -- Backlog** | SIDE-LOW-001: Improve health check integrity | Low | Better monitoring |
| **P3 -- Backlog** | SIDE-LOW-002: Implement real metrics | High | Requirement compliance |
| **P3 -- Backlog** | SIDE-LOW-003: Add audit logging | Medium | Forensics capability |
| **P3 -- Backlog** | SIDE-INFO-001: Drop root privileges | Medium | Least privilege |
| **P3 -- Backlog** | SIDE-INFO-002: TLS client config | Low | Future-proofing |

---

## Architectural Question: Is the Sidecar Even Necessary?

The sidecar exists because Slopbox needs an administrative channel into the VM without SSH. But the sidecar introduces a massive attack surface. Consider alternatives:

1. **Immutable config at provision time only**: Inject all config during cloud-init / Fly Machine creation. To change config, destroy and recreate the VM. This eliminates the sidecar entirely.

2. **Pull-based config**: Instead of the control plane pushing config to the sidecar, have the gateway poll a control plane endpoint for config updates. This eliminates inbound attack surface on the VM (no listening port). The agent cannot reach the control plane's config endpoint because it does not have the control plane API key.

3. **Signed config bundles**: If a sidecar must exist, make it accept only cryptographically signed config bundles (e.g., Ed25519 signatures from the control plane's private key). The signing key never leaves the control plane. The sidecar verifies the signature before applying any config.

4. **Reduce to config-only**: Remove the `/files/*` endpoint entirely. Only allow `/config` with strict schema validation. This dramatically reduces the blast radius.

The current design -- a root-running HTTP server with arbitrary file write capabilities, authenticated by a single static token that shares the same environment as the agent runtime -- is the highest-risk component in the Slopbox architecture.

---

**Blue Team**: Your move. I want to hear how you plan to defend a 168-line binary that gives root file write access to anyone who can read an environment variable on the same machine.
