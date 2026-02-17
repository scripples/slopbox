# Red Team Security Analysis

**Date**: 2026-02-16 | **Scope**: Admin endpoint security, authentication flow, authorization model | **Analyst**: Red Team

---

## Executive Summary

The admin authentication system in `cb-api` contains **two critical vulnerabilities** and several high/medium-severity issues. The most severe finding is that JWT tokens never expire, meaning any leaked JWT grants permanent access. Combined with the non-constant-time static token comparison, an attacker can time-attack the `ADMIN_API_TOKEN` or exploit a stolen JWT indefinitely. The `cleanup_stuck` endpoint can destroy legitimately provisioning VPSes with no time-guard. Additionally, the JWT validation disables all spec-required claims, meaning any HS256-signed payload with a valid UUID `sub` is accepted -- no audience, issuer, or expiration checks.

---

## Critical Findings

### [CRIT-001] JWT Tokens Never Expire -- Permanent Access on Leak

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/auth.rs:29-30`
- **Category**: Authentication Bypass / Credential Lifetime
- **Code**:
  ```rust
  validation.required_spec_claims.clear();  // line 29
  validation.validate_exp = false;          // line 30
  ```
- **Attack Scenario**:
  1. Attacker obtains any JWT ever issued for an admin user (log file, HTTP referer leak, browser local storage, compromised frontend, stolen backup).
  2. Because `validate_exp = false`, the token is accepted forever -- even years after issuance.
  3. Because `required_spec_claims.clear()` removes the `exp` requirement, tokens can be minted without `exp` at all.
  4. Attacker uses this JWT against `admin_middleware` (auth.rs:118) and gains permanent admin access.
  5. Even if the admin user is later frozen or deleted from the database, the token remains valid as a JWT signature -- the only check that could catch a deleted user is the `User::get_by_id` call, but a frozen user's JWT would still pass validation up to the DB lookup.
- **Impact**: **Critical**. A single leaked JWT grants irrevocable admin access. There is no revocation mechanism, no token rotation, and no expiration. Combined with the fact that JWTs are passed in query strings for WebSocket connections (auth.rs:151-157), they will appear in server access logs, reverse proxy logs, and browser history.
- **Evidence**: `validate_exp = false` on line 30, `required_spec_claims.clear()` on line 29.
- **Recommendation**:
  1. Set `validation.validate_exp = true` and require `exp` in `required_spec_claims`.
  2. Set a maximum token lifetime (e.g., 1 hour for admin tokens).
  3. Add `iss` and `aud` validation to prevent cross-service token reuse.
  4. Consider a token revocation list or short-lived JWTs + refresh token pattern.

### [CRIT-002] Timing Side-Channel on Static Admin Token Comparison

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/auth.rs:113`
- **Category**: Authentication Bypass via Timing Attack
- **Code**:
  ```rust
  if token == state.config.admin_api_token {  // line 113
      return next.run(req).await;
  }
  ```
- **Attack Scenario**:
  1. `==` on `&str` uses byte-by-byte comparison that short-circuits on first mismatch (standard `PartialEq` for `str`).
  2. Attacker sends many requests with candidate tokens, measuring response time.
  3. When the first byte matches, the comparison takes slightly longer (proceeds to second byte).
  4. By iteratively guessing each byte position, the attacker recovers the full `ADMIN_API_TOKEN`.
  5. With the static token, the attacker bypasses all JWT and DB checks -- `admin_middleware` returns `next.run(req).await` immediately (line 114), no user lookup, no role check, no status check.
  6. The token is a static string from an env var (config.rs:37) -- it has no expiration and no rotation mechanism.
- **Impact**: **Critical**. Full admin access recovery via network-observable timing. The static token path skips all authorization checks (no user lookup, no role check, no status check). An attacker with network proximity (same datacenter, compromised CDN, or co-located VM) can extract the token.
- **Evidence**: Standard `==` comparison at auth.rs:113. No use of `constant_time_eq`, `hmac::Mac::verify`, or `ring::constant_time::verify_slices_are_equal`.
- **Recommendation**:
  1. Replace `token == state.config.admin_api_token` with a constant-time comparison, e.g., `use subtle::ConstantTimeEq;` or hash both sides with HMAC and compare MACs.
  2. Consider eliminating the static token entirely in favor of JWT-only admin auth.
  3. If the static token is kept, add rate limiting on failed admin auth attempts.

---

## High Findings

### [HIGH-001] JWT Has No Audience, Issuer, or Subject Validation -- Cross-Service Token Reuse

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/auth.rs:28-30`
- **Category**: Authentication Weakness
- **Code**:
  ```rust
  let mut validation = Validation::new(Algorithm::HS256);
  validation.required_spec_claims.clear();
  validation.validate_exp = false;
  ```
- **Attack Scenario**:
  1. The JWT validation only checks the HS256 signature against `JWT_SECRET`.
  2. If any other service (Auth.js backend, internal microservice, staging environment) shares the same `JWT_SECRET` or if the secret is reused across environments, JWTs from that service are accepted here.
  3. A regular user JWT from one service can be presented to the admin endpoint. The `admin_middleware` will attempt to validate it as a JWT (auth.rs:118), find a valid `sub` UUID, look up the user, and check the role. However, if the JWT was minted for a different audience (e.g., a frontend session), the only guard is the `UserRole::Admin` check in the database.
  4. More critically: if a different service uses the same `JWT_SECRET` and mints a JWT with an admin user's UUID as `sub`, that JWT is accepted verbatim.
- **Impact**: **High**. Cross-environment or cross-service token reuse could escalate to admin access. No defense-in-depth against secret reuse.
- **Recommendation**: Add `aud` (audience) and `iss` (issuer) claims to JWTs and validate them. This prevents tokens minted for the frontend from being accepted by the admin API, and vice versa.

### [HIGH-002] `cleanup_stuck` Destroys Legitimately Provisioning VPSes

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/routes/admin.rs:370-401`
- **Category**: Availability / Data Destruction
- **Code**:
  ```rust
  // TODO: Add a time threshold (e.g. only destroy VPSes stuck in "provisioning"
  // for more than 15 minutes) to avoid accidentally destroying VPSes that are
  // legitimately still provisioning.
  pub async fn cleanup_stuck(
      State(state): State<AppState>,
  ) -> Result<Json<serde_json::Value>, ApiError> {
      let stuck = Vps::list_by_state(&state.db, DbVpsState::Provisioning).await?;
      // ... destroys ALL provisioning VPSes
  ```
- **Attack Scenario**:
  1. An admin (or attacker who has compromised admin credentials) calls `POST /admin/cleanup`.
  2. ALL VPSes currently in `Provisioning` state are destroyed -- including ones that started provisioning 5 seconds ago.
  3. The TODO comment on line 370-372 explicitly acknowledges this gap.
  4. Even without malice, an automated script or accidental invocation destroys in-flight provisions.
  5. Best-effort provider destroy means the cloud resources are deleted but errors are silently swallowed (`let _ = provider.destroy_vps(...)`).
- **Impact**: **High**. Mass destruction of customer VPSes, data loss, service disruption. No confirmation, no time guard, no dry-run mode.
- **Recommendation**:
  1. Add a time threshold (e.g., only clean up VPSes stuck in `Provisioning` for > 15 minutes, as the TODO suggests).
  2. Add a `dry_run` query parameter that lists what would be cleaned up without acting.
  3. Log which VPSes are destroyed and notify affected users.
  4. Consider requiring a confirmation token or two-step invocation.

### [HIGH-003] Admin Can Escalate Any User to Admin Without Audit Trail

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/routes/admin.rs:188-196`
- **Category**: Authorization / Privilege Escalation
- **Code**:
  ```rust
  pub async fn set_user_role(
      State(state): State<AppState>,
      Path(user_id): Path<Uuid>,
      Json(req): Json<SetRoleRequest>,
  ) -> Result<StatusCode, ApiError> {
      User::get_by_id(&state.db, user_id).await?;
      User::set_role(&state.db, user_id, req.role).await?;
      Ok(StatusCode::NO_CONTENT)
  }
  ```
- **Attack Scenario**:
  1. An admin promotes any user to admin via `PUT /admin/users/{id}/role` with `{"role": "admin"}`.
  2. There is no self-demotion guard: an admin can demote themselves, potentially locking everyone out if they are the last admin.
  3. There is no audit logging of role changes.
  4. When combined with the static token auth (CRIT-002), an attacker with the token can silently create additional admin accounts as persistence mechanisms.
- **Impact**: **High**. Silent privilege escalation with no audit trail enables persistence after initial compromise.
- **Recommendation**:
  1. Add audit logging for all role and status changes.
  2. Prevent an admin from demoting themselves if they are the last admin.
  3. Consider requiring multi-admin approval for role changes.

---

## Medium Findings

### [MED-001] Static `ADMIN_API_TOKEN` Has No Rotation or Expiration

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/config.rs:37`
- **Category**: Secret Management
- **Code**:
  ```rust
  admin_api_token: env::var("ADMIN_API_TOKEN").expect("ADMIN_API_TOKEN must be set"),
  ```
- **Attack Scenario**:
  1. The token is read from the environment once at startup and never rotated.
  2. If the token leaks (CI logs, .env file in version control, developer laptop, Fly.io secrets dump), it remains valid until the process is restarted with a new value.
  3. There is no mechanism to rotate the token without downtime.
- **Impact**: **Medium**. Leaked static credentials persist indefinitely.
- **Recommendation**: Support periodic rotation (e.g., accept both old and new tokens during a grace period) or replace with JWT-only admin auth.

### [MED-002] Admin Static Token Path Bypasses All User Checks

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/auth.rs:112-115`
- **Category**: Authorization Bypass
- **Code**:
  ```rust
  // Check for static admin token first
  if token == state.config.admin_api_token {
      return next.run(req).await;  // No user lookup, no role check, no status check
  }
  ```
- **Attack Scenario**:
  1. The static token path returns immediately -- no `UserId` is inserted into request extensions.
  2. Admin handlers that need to know *who* is acting cannot distinguish between different admin operators.
  3. If a handler ever checks `req.extensions().get::<UserId>()`, it will get `None` for static-token requests, which could cause a panic or logic error.
  4. No accountability: actions taken with the static token are unattributable.
- **Impact**: **Medium**. No audit attribution for static-token admin actions. Potential for handler panics if they assume `UserId` is present.
- **Recommendation**:
  1. Insert a synthetic "system" `UserId` for static-token requests so all handlers have a consistent identity.
  2. Log which authentication method was used for each admin request.

### [MED-003] Error Messages Leak Internal State

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/error.rs:47-49`
- **Category**: Information Disclosure
- **Code**:
  ```rust
  ApiError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
  ApiError::Infra(_) => StatusCode::BAD_GATEWAY,
  // ...
  let body = serde_json::json!({ "error": self.to_string() });
  ```
- **Attack Scenario**:
  1. Database errors (sqlx errors) and infra errors are serialized directly into the response body via `self.to_string()`.
  2. The `Database(#[from] sqlx::Error)` variant includes the full sqlx error message, which can contain SQL query fragments, table names, constraint names, and PostgreSQL error details.
  3. The `Infra(#[from] cb_infra::Error)` variant can leak provider API error details (Fly.io/Hetzner API responses).
  4. The `Internal(String)` variant on line 229 of admin.rs leaks "VPS has no provider VM ID", revealing internal architecture.
  5. Attacker uses these error details to map the database schema and infrastructure.
- **Impact**: **Medium**. Information disclosure aids further attacks.
- **Recommendation**: Return generic error messages to clients. Log detailed errors server-side only.

### [MED-004] JWT in WebSocket Query Parameter Logged and Cached

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/auth.rs:151-157`
- **Category**: Credential Exposure
- **Code**:
  ```rust
  pub fn authenticate_gateway_request(
      headers: &axum::http::HeaderMap,
      query: Option<&str>,
      jwt_secret: &str,
  ) -> Option<UserId> {
      // Try query param first (WebSocket)
      if let Some(query) = query {
          for param in query.split('&') {
              if let Some(token) = param.strip_prefix("token=")
  ```
- **Attack Scenario**:
  1. WebSocket connections pass the JWT as `?token=<jwt>` in the URL.
  2. This URL appears in: server access logs (TraceLayer is enabled, main.rs:97), reverse proxy logs, CDN logs, browser history, HTTP referer headers.
  3. Combined with CRIT-001 (tokens never expire), a JWT leaked via logs grants permanent access.
- **Impact**: **Medium**. JWT credential exposure through logging and URL handling.
- **Recommendation**: Use a short-lived, single-use ticket exchanged for a session, rather than passing the full JWT in the query string. Alternatively, use a `Sec-WebSocket-Protocol` subprotocol header for token transport.

### [MED-005] No Rate Limiting on Admin Endpoints

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/routes/mod.rs:20-41`
- **Category**: Brute Force / DoS
- **Attack Scenario**:
  1. No rate limiting middleware on admin routes (or any routes).
  2. An attacker can brute-force the `ADMIN_API_TOKEN` (exacerbating CRIT-002's timing attack).
  3. An attacker can brute-force JWT tokens (though this is computationally infeasible with a strong secret).
  4. `cleanup_stuck` can be called repeatedly, creating race conditions with concurrent provisioning.
- **Impact**: **Medium**. Enables brute-force and abuse of destructive endpoints.
- **Recommendation**: Add rate limiting middleware (e.g., `tower-governor` or a custom token bucket) on admin routes, especially auth failure paths.

---

## Low Findings

### [LOW-001] Admin `set_user_status` Double-Fetches User

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/routes/admin.rs:165-186`
- **Category**: TOCTOU Race Condition
- **Code**:
  ```rust
  // Verify user exists
  User::get_by_id(&state.db, user_id).await?;       // First fetch

  // If activating a user, auto-assign the demo plan if they don't have one
  if req.status == UserStatus::Active {
      let user = User::get_by_id(&state.db, user_id).await?;  // Second fetch
  ```
- **Attack Scenario**:
  1. Between the first `get_by_id` (existence check) and the second `get_by_id` (plan check), the user could be deleted or modified by a concurrent request.
  2. This is a minor TOCTOU race -- the practical impact is low because all operations are on the same database and the window is small, but it represents a logic smell.
- **Impact**: **Low**. Minor race condition, unlikely to cause real harm.
- **Recommendation**: Remove the redundant first `get_by_id` call -- the second call already verifies existence.

### [LOW-002] `destroy_vps` Silently Swallows Provider Errors

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/routes/admin.rs:249-255`
- **Category**: Error Handling / Orphaned Resources
- **Code**:
  ```rust
  if let Some(vm_id) = &vps.provider_vm_id
      && let Ok((provider, _config)) = super::vps::provider_for_vps(&state, &vps).await
  {
      let _ = provider
          .destroy_vps(&cb_infra::types::VpsId(vm_id.clone()))
          .await;
  }
  ```
- **Attack Scenario**:
  1. If the provider API call fails (network error, rate limit, invalid credentials), the error is silently discarded (`let _ = ...`).
  2. The database state is set to `Destroyed` regardless, but the cloud VM continues running.
  3. An attacker who knows this can trigger destroy, then exploit the still-running VM (which the control plane believes is dead) as a ghost instance with no monitoring.
- **Impact**: **Low** (requires prior admin access). Orphaned cloud resources, billing leakage, unmonitored VMs.
- **Recommendation**: Log provider destroy failures. Consider a "pending_destroy" state and retry mechanism.

### [LOW-003] `gateway_token` Leaked in Admin Agent List Response

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/routes/admin.rs:69-90`
- **Category**: Information Disclosure (mitigated by admin-only access)
- **Code**:
  ```rust
  pub struct AdminAgentResponse {
      pub id: Uuid,
      pub user_id: Uuid,
      pub name: String,
      pub vps_id: Option<Uuid>,  // gateway_token is NOT included
  ```
- **Assessment**: The `AdminAgentResponse` DTO correctly excludes `gateway_token` from the response. This is good practice. However, the `Agent` model includes `gateway_token` with `Serialize` derived (models.rs:505-514). If any admin endpoint inadvertently returns the raw `Agent` struct instead of the DTO, the token would leak. This is a low-risk design fragility.
- **Impact**: **Low / Informational**. Currently safe, but fragile -- one accidental `Json(agent)` instead of `Json(AdminAgentResponse::from(agent))` would leak gateway tokens.
- **Recommendation**: Consider adding `#[serde(skip_serializing)]` on `Agent.gateway_token` to prevent accidental serialization.

---

## Informational Findings

### [INFO-001] No Request Logging / Audit Trail for Admin Actions

- **Location**: All admin handlers in `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/routes/admin.rs`
- **Category**: Compliance / Forensics
- **Assessment**: No admin action is logged beyond the generic `TraceLayer` HTTP access log. There is no structured audit log recording who performed what admin action, on which resource, at what time. This makes incident investigation and compliance auditing impossible.

### [INFO-002] `Agent::generate_gateway_token` Uses `rand::rng().random()`

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-db/src/models.rs:517-520`
- **Code**:
  ```rust
  fn generate_gateway_token() -> String {
      use rand::Rng;
      let bytes: [u8; 32] = rand::rng().random();
      bytes.iter().map(|b| format!("{b:02x}")).collect()
  }
  ```
- **Assessment**: `rand::rng()` returns `ThreadRng` which is backed by a CSPRNG (ChaCha12). The 32-byte (256-bit) token provides adequate entropy. This is acceptable. However, the hex encoding produces a 64-character token where only hex characters are valid, which is fine but could be more compact with base64.

### [INFO-003] CORS Configuration Allows Credentials

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-api/src/main.rs:61-81`
- **Code**:
  ```rust
  let cors = CorsLayer::new()
      .allow_origin(AllowOrigin::exact(...))
      .allow_credentials(true);
  ```
- **Assessment**: CORS is configured with `allow_credentials(true)` and a specific origin (`FRONTEND_ORIGIN`). This is correctly configured -- it does NOT use wildcard origin, which would be a vulnerability. However, if `FRONTEND_ORIGIN` defaults to `http://localhost:3000` (config.rs:26), the production deployment must override this or localhost origins will be accepted.

### [INFO-004] Forward Proxy `gateway_token` Comparison in Database Query

- **Location**: `/home/mr-idiot/claude/cludbox/backend/crates/cb-db/src/models.rs:584-589`
- **Code**:
  ```rust
  pub async fn get_by_id_and_token(pool: &PgPool, id: Uuid, token: &str) -> sqlx::Result<Self> {
      sqlx::query_as("SELECT * FROM agents WHERE id = $1 AND gateway_token = $2")
  ```
- **Assessment**: The forward proxy authenticates agents by comparing `gateway_token` in a SQL query. PostgreSQL's string comparison is not constant-time, but the comparison happens server-side in the database, making network-level timing attacks against the proxy significantly harder than against the in-process `==` in CRIT-002. Still, a co-located attacker with very precise timing could theoretically distinguish.

---

## Recommendations Priority Matrix

| Priority | Finding | Effort | Impact |
|----------|---------|--------|--------|
| **P0 - Immediate** | CRIT-001: Enable JWT expiration validation | Low | Critical -- permanent access on any token leak |
| **P0 - Immediate** | CRIT-002: Constant-time admin token comparison | Low | Critical -- token extractable via timing |
| **P1 - This Sprint** | HIGH-001: Add `aud`/`iss` JWT claims | Low | High -- cross-service token reuse |
| **P1 - This Sprint** | HIGH-002: Add time threshold to `cleanup_stuck` | Low | High -- prevents mass VPS destruction |
| **P1 - This Sprint** | HIGH-003: Add audit logging for role/status changes | Medium | High -- forensics and compliance |
| **P2 - Next Sprint** | MED-001: Implement admin token rotation | Medium | Medium -- reduces blast radius of leaks |
| **P2 - Next Sprint** | MED-002: Insert synthetic UserId for static-token path | Low | Medium -- audit attribution |
| **P2 - Next Sprint** | MED-003: Sanitize error messages in production | Low | Medium -- reduces information disclosure |
| **P2 - Next Sprint** | MED-004: Replace JWT-in-URL with ticket exchange | Medium | Medium -- credential exposure in logs |
| **P2 - Next Sprint** | MED-005: Add rate limiting to admin endpoints | Medium | Medium -- brute force mitigation |
| **P3 - Backlog** | LOW-001: Fix double-fetch TOCTOU | Low | Low |
| **P3 - Backlog** | LOW-002: Log provider destroy failures | Low | Low |
| **P3 - Backlog** | LOW-003: Add `skip_serializing` on `gateway_token` | Low | Low |

---

## Attack Chain Summary

The most dangerous attack chain combines multiple findings:

1. **CRIT-002** (timing attack) extracts the `ADMIN_API_TOKEN` over ~10K-100K requests.
2. With the static token, attacker calls `PUT /admin/users/{id}/role` (**HIGH-003**) to promote a controlled user account to admin, establishing persistence.
3. Attacker mints a JWT for the promoted user. Because of **CRIT-001**, this JWT never expires.
4. Attacker calls `POST /admin/cleanup` (**HIGH-002**) to destroy all provisioning VPSes as a disruption attack.
5. Throughout, **MED-003** error messages leak database schema details, and **INFO-001** (no audit logging) means the attack is invisible.

This chain demonstrates how individually medium-severity issues compound into a full platform compromise with no forensic evidence.
