use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

// ── Plan ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Plan {
    pub id: Uuid,
    pub name: String,
    pub max_agents: i32,
    pub max_vpses: i32,
    pub max_bandwidth_bytes: i64,
    pub max_storage_bytes: i64,
    pub max_cpu_ms: i64,
    pub max_memory_mb_seconds: i64,
    pub overage_bandwidth_cost_per_gb_cents: i64,
    pub overage_cpu_cost_per_hour_cents: i64,
    pub overage_memory_cost_per_gb_hour_cents: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct NewPlan<'a> {
    pub name: &'a str,
    pub max_agents: i32,
    pub max_vpses: i32,
    pub max_bandwidth_bytes: i64,
    pub max_storage_bytes: i64,
    pub max_cpu_ms: i64,
    pub max_memory_mb_seconds: i64,
    pub overage_bandwidth_cost_per_gb_cents: i64,
    pub overage_cpu_cost_per_hour_cents: i64,
    pub overage_memory_cost_per_gb_hour_cents: i64,
}

impl Plan {
    pub async fn insert(pool: &PgPool, plan: &NewPlan<'_>) -> sqlx::Result<Self> {
        sqlx::query_as(
            r#"INSERT INTO plans (name, max_agents, max_vpses, max_bandwidth_bytes, max_storage_bytes, max_cpu_ms, max_memory_mb_seconds,
                                  overage_bandwidth_cost_per_gb_cents, overage_cpu_cost_per_hour_cents, overage_memory_cost_per_gb_hour_cents)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
               RETURNING *"#,
        )
        .bind(plan.name)
        .bind(plan.max_agents)
        .bind(plan.max_vpses)
        .bind(plan.max_bandwidth_bytes)
        .bind(plan.max_storage_bytes)
        .bind(plan.max_cpu_ms)
        .bind(plan.max_memory_mb_seconds)
        .bind(plan.overage_bandwidth_cost_per_gb_cents)
        .bind(plan.overage_cpu_cost_per_hour_cents)
        .bind(plan.overage_memory_cost_per_gb_hour_cents)
        .fetch_one(pool)
        .await
    }

    pub async fn get_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Self> {
        sqlx::query_as("SELECT * FROM plans WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    pub async fn list(pool: &PgPool) -> sqlx::Result<Vec<Self>> {
        sqlx::query_as("SELECT * FROM plans ORDER BY name")
            .fetch_all(pool)
            .await
    }

    pub async fn add_vps_config(pool: &PgPool, plan_id: Uuid, vps_config_id: Uuid) -> sqlx::Result<()> {
        sqlx::query(
            "INSERT INTO plan_vps_configs (plan_id, vps_config_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(plan_id)
        .bind(vps_config_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn remove_vps_config(pool: &PgPool, plan_id: Uuid, vps_config_id: Uuid) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM plan_vps_configs WHERE plan_id = $1 AND vps_config_id = $2")
            .bind(plan_id)
            .bind(vps_config_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Compute total overage cost in cents given aggregate usage.
    ///
    /// Returns 0 when usage is within plan limits. Only counts the portion
    /// that exceeds each limit, converted to the appropriate unit and
    /// multiplied by the per-unit overage rate.
    pub fn overage_cost_cents(&self, usage: &AggregateUsage) -> i64 {
        let bw_over = (usage.bandwidth_bytes - self.max_bandwidth_bytes).max(0);
        let bw_cost = bw_over as f64 / 1_073_741_824.0 // bytes -> GB
            * self.overage_bandwidth_cost_per_gb_cents as f64;

        let cpu_over = (usage.cpu_used_ms - self.max_cpu_ms).max(0);
        let cpu_cost = cpu_over as f64 / 3_600_000.0 // ms -> hours
            * self.overage_cpu_cost_per_hour_cents as f64;

        let mem_over = (usage.memory_used_mb_seconds - self.max_memory_mb_seconds).max(0);
        let mem_cost = mem_over as f64 / (1024.0 * 3600.0) // MB*s -> GB*hours
            * self.overage_memory_cost_per_gb_hour_cents as f64;

        (bw_cost + cpu_cost + mem_cost).ceil() as i64
    }
}

// ── VpsConfig ───────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct VpsConfig {
    pub id: Uuid,
    pub name: String,
    pub provider: String,
    pub image: String,
    pub cpu_millicores: i32,
    pub memory_mb: i32,
    pub disk_gb: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl VpsConfig {
    pub async fn insert(
        pool: &PgPool,
        name: &str,
        provider: &str,
        image: &str,
        cpu_millicores: i32,
        memory_mb: i32,
        disk_gb: i32,
    ) -> sqlx::Result<Self> {
        sqlx::query_as(
            "INSERT INTO vps_configs (name, provider, image, cpu_millicores, memory_mb, disk_gb) VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
        )
        .bind(name)
        .bind(provider)
        .bind(image)
        .bind(cpu_millicores)
        .bind(memory_mb)
        .bind(disk_gb)
        .fetch_one(pool)
        .await
    }

    pub async fn get_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Self> {
        sqlx::query_as("SELECT * FROM vps_configs WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    pub async fn list_for_plan(pool: &PgPool, plan_id: Uuid) -> sqlx::Result<Vec<Self>> {
        sqlx::query_as(
            r#"SELECT vc.* FROM vps_configs vc
               JOIN plan_vps_configs pvc ON pvc.vps_config_id = vc.id
               WHERE pvc.plan_id = $1
               ORDER BY vc.cpu_millicores, vc.memory_mb"#,
        )
        .bind(plan_id)
        .fetch_all(pool)
        .await
    }
}

// ── User ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub plan_id: Option<Uuid>,
    pub email_verified: Option<DateTime<Utc>>,
    pub image: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    pub async fn insert(pool: &PgPool, email: &str, name: Option<&str>) -> sqlx::Result<Self> {
        sqlx::query_as("INSERT INTO users (email, name) VALUES ($1, $2) RETURNING *")
            .bind(email)
            .bind(name)
            .fetch_one(pool)
            .await
    }

    pub async fn get_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Self> {
        sqlx::query_as("SELECT * FROM users WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    pub async fn get_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Self> {
        sqlx::query_as("SELECT * FROM users WHERE email = $1")
            .bind(email)
            .fetch_one(pool)
            .await
    }

    pub async fn set_plan(pool: &PgPool, user_id: Uuid, plan_id: Option<Uuid>) -> sqlx::Result<()> {
        sqlx::query("UPDATE users SET plan_id = $1 WHERE id = $2")
            .bind(plan_id)
            .bind(user_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}

// ── OAuthAccount (read-only from Rust — Auth.js writes these) ──────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OAuthAccount {
    pub id: Uuid,
    pub user_id: Uuid,
    pub r#type: String,
    pub provider: String,
    pub provider_account_id: String,
    pub refresh_token: Option<String>,
    pub access_token: Option<String>,
    pub expires_at: Option<i32>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub id_token: Option<String>,
    pub session_state: Option<String>,
}

impl OAuthAccount {
    pub async fn get_by_user_id(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<Self>> {
        sqlx::query_as("SELECT * FROM accounts WHERE user_id = $1 ORDER BY provider")
            .bind(user_id)
            .fetch_all(pool)
            .await
    }
}

// ── Session (read-only from Rust — Auth.js manages sessions) ───────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub session_token: String,
    pub user_id: Uuid,
    pub expires: DateTime<Utc>,
}

impl Session {
    /// Look up a session by its token, returning `None` if expired or not found.
    pub async fn get_valid_by_token(pool: &PgPool, token: &str) -> sqlx::Result<Option<Self>> {
        sqlx::query_as("SELECT * FROM sessions WHERE session_token = $1 AND expires > now()")
            .bind(token)
            .fetch_optional(pool)
            .await
    }
}

// ── Vps ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type, Serialize, Deserialize)]
#[sqlx(type_name = "vps_state", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum VpsState {
    Provisioning,
    Running,
    Stopped,
    Destroyed,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Vps {
    pub id: Uuid,
    pub user_id: Uuid,
    pub vps_config_id: Uuid,
    pub name: String,
    pub provider: String,
    pub provider_vm_id: Option<String>,
    pub address: Option<String>,
    pub state: VpsState,
    pub storage_used_bytes: i64,
    pub cpu_used_ms: Option<i64>,
    pub memory_used_mb_seconds: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Vps {
    pub async fn insert(
        pool: &PgPool,
        user_id: Uuid,
        vps_config_id: Uuid,
        name: &str,
        provider: &str,
    ) -> sqlx::Result<Self> {
        sqlx::query_as(
            r#"INSERT INTO vpses (user_id, vps_config_id, name, provider)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
        )
        .bind(user_id)
        .bind(vps_config_id)
        .bind(name)
        .bind(provider)
        .fetch_one(pool)
        .await
    }

    pub async fn get_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Self> {
        sqlx::query_as("SELECT * FROM vpses WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<Self>> {
        sqlx::query_as(
            "SELECT * FROM vpses WHERE user_id = $1 ORDER BY created_at",
        )
        .bind(user_id)
        .fetch_all(pool)
        .await
    }

    pub async fn count_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<i64> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM vpses WHERE user_id = $1 AND state != 'destroyed'",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;
        Ok(count)
    }

    pub async fn list_by_state(pool: &PgPool, state: VpsState) -> sqlx::Result<Vec<Self>> {
        sqlx::query_as("SELECT * FROM vpses WHERE state = $1 ORDER BY created_at")
            .bind(state)
            .fetch_all(pool)
            .await
    }

    pub async fn update_provider_refs(
        pool: &PgPool,
        id: Uuid,
        provider_vm_id: Option<&str>,
        address: Option<&str>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "UPDATE vpses SET provider_vm_id = $1, address = $2 WHERE id = $3",
        )
        .bind(provider_vm_id)
        .bind(address)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn set_state(pool: &PgPool, id: Uuid, state: VpsState) -> sqlx::Result<()> {
        sqlx::query("UPDATE vpses SET state = $1 WHERE id = $2")
            .bind(state)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_usage(
        pool: &PgPool,
        id: Uuid,
        storage_used_bytes: i64,
        cpu_used_ms: Option<i64>,
        memory_used_mb_seconds: Option<i64>,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"UPDATE vpses
               SET storage_used_bytes     = $1,
                   cpu_used_ms            = $2,
                   memory_used_mb_seconds = $3
               WHERE id = $4"#,
        )
        .bind(storage_used_bytes)
        .bind(cpu_used_ms)
        .bind(memory_used_mb_seconds)
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// ── Agent ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub user_id: Uuid,
    pub vps_id: Option<Uuid>,
    pub name: String,
    pub gateway_token: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Agent {
    fn generate_gateway_token() -> String {
        use rand::Rng;
        let bytes: [u8; 32] = rand::rng().random();
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub async fn insert(pool: &PgPool, user_id: Uuid, name: &str) -> sqlx::Result<Self> {
        let token = Self::generate_gateway_token();
        sqlx::query_as(
            "INSERT INTO agents (user_id, name, gateway_token) VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(user_id)
        .bind(name)
        .bind(&token)
        .fetch_one(pool)
        .await
    }

    pub async fn get_by_id(pool: &PgPool, id: Uuid) -> sqlx::Result<Self> {
        sqlx::query_as("SELECT * FROM agents WHERE id = $1")
            .bind(id)
            .fetch_one(pool)
            .await
    }

    pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Vec<Self>> {
        sqlx::query_as("SELECT * FROM agents WHERE user_id = $1 ORDER BY created_at")
            .bind(user_id)
            .fetch_all(pool)
            .await
    }

    pub async fn count_for_user(pool: &PgPool, user_id: Uuid) -> sqlx::Result<i64> {
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(pool)
            .await?;
        Ok(count)
    }

    pub async fn assign_vps(pool: &PgPool, agent_id: Uuid, vps_id: Option<Uuid>) -> sqlx::Result<()> {
        sqlx::query("UPDATE agents SET vps_id = $1 WHERE id = $2")
            .bind(vps_id)
            .bind(agent_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
        sqlx::query("DELETE FROM agents WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn get_by_id_and_token(pool: &PgPool, id: Uuid, token: &str) -> sqlx::Result<Self> {
        sqlx::query_as("SELECT * FROM agents WHERE id = $1 AND gateway_token = $2")
            .bind(id)
            .bind(token)
            .fetch_one(pool)
            .await
    }

    pub async fn rotate_gateway_token(pool: &PgPool, id: Uuid) -> sqlx::Result<String> {
        let token = Self::generate_gateway_token();
        sqlx::query("UPDATE agents SET gateway_token = $1 WHERE id = $2")
            .bind(&token)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(token)
    }
}

// ── VpsUsagePeriod ──────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct VpsUsagePeriod {
    pub vps_id: Uuid,
    pub period_start: NaiveDate,
    pub bandwidth_bytes: i64,
    pub cpu_used_ms: i64,
    pub memory_used_mb_seconds: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl VpsUsagePeriod {
    /// Atomically increment bandwidth for the current calendar month.
    pub async fn add_bandwidth(pool: &PgPool, vps_id: Uuid, bytes: i64) -> sqlx::Result<()> {
        sqlx::query(
            r#"INSERT INTO vps_usage_periods (vps_id, period_start, bandwidth_bytes)
               VALUES ($1, date_trunc('month', now())::date, $2)
               ON CONFLICT (vps_id, period_start)
               DO UPDATE SET bandwidth_bytes = vps_usage_periods.bandwidth_bytes + EXCLUDED.bandwidth_bytes"#,
        )
        .bind(vps_id)
        .bind(bytes)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Atomically increment CPU and memory deltas for the current calendar month.
    pub async fn add_cpu_memory(
        pool: &PgPool,
        vps_id: Uuid,
        cpu_delta_ms: i64,
        mem_delta_mb_seconds: i64,
    ) -> sqlx::Result<()> {
        sqlx::query(
            r#"INSERT INTO vps_usage_periods (vps_id, period_start, cpu_used_ms, memory_used_mb_seconds)
               VALUES ($1, date_trunc('month', now())::date, $2, $3)
               ON CONFLICT (vps_id, period_start)
               DO UPDATE SET cpu_used_ms = vps_usage_periods.cpu_used_ms + EXCLUDED.cpu_used_ms,
                             memory_used_mb_seconds = vps_usage_periods.memory_used_mb_seconds + EXCLUDED.memory_used_mb_seconds"#,
        )
        .bind(vps_id)
        .bind(cpu_delta_ms)
        .bind(mem_delta_mb_seconds)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Fetch the current month's usage row, returning default zeros if none exists.
    pub async fn get_current(pool: &PgPool, vps_id: Uuid) -> sqlx::Result<Self> {
        sqlx::query_as(
            r#"SELECT * FROM vps_usage_periods
               WHERE vps_id = $1 AND period_start = date_trunc('month', now())::date"#,
        )
        .bind(vps_id)
        .fetch_optional(pool)
        .await
        .map(|opt| {
            opt.unwrap_or(Self {
                vps_id,
                period_start: Utc::now().date_naive().with_day(1).unwrap_or(Utc::now().date_naive()),
                bandwidth_bytes: 0,
                cpu_used_ms: 0,
                memory_used_mb_seconds: 0,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        })
    }

    /// Sum usage across all of a user's VPSes for the current month.
    pub async fn get_user_aggregate(pool: &PgPool, user_id: Uuid) -> sqlx::Result<AggregateUsage> {
        let row: (i64, i64, i64) = sqlx::query_as(
            r#"SELECT COALESCE(SUM(u.bandwidth_bytes), 0),
                      COALESCE(SUM(u.cpu_used_ms), 0),
                      COALESCE(SUM(u.memory_used_mb_seconds), 0)
               FROM vps_usage_periods u
               JOIN vpses v ON v.id = u.vps_id
               WHERE v.user_id = $1
                 AND u.period_start = date_trunc('month', now())::date
                 AND v.state != 'destroyed'"#,
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        Ok(AggregateUsage {
            bandwidth_bytes: row.0,
            cpu_used_ms: row.1,
            memory_used_mb_seconds: row.2,
        })
    }
}

// ── AggregateUsage ──────────────────────────────────────────────────

/// Summed usage across all of a user's VPSes for a billing period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateUsage {
    pub bandwidth_bytes: i64,
    pub cpu_used_ms: i64,
    pub memory_used_mb_seconds: i64,
}

// ── OverageBudget ───────────────────────────────────────────────────

/// Per-user monthly overage budget in cents.
///
/// Missing row = $0 budget (no overage allowed beyond plan limits).
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OverageBudget {
    pub user_id: Uuid,
    pub period_start: NaiveDate,
    pub budget_cents: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OverageBudget {
    /// Fetch the current month's overage budget, defaulting to 0 if no row exists.
    pub async fn get_current(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Self> {
        sqlx::query_as(
            r#"SELECT * FROM overage_budgets
               WHERE user_id = $1 AND period_start = date_trunc('month', now())::date"#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map(|opt| {
            opt.unwrap_or(Self {
                user_id,
                period_start: Utc::now().date_naive().with_day(1).unwrap_or(Utc::now().date_naive()),
                budget_cents: 0,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        })
    }

    /// Upsert the current month's overage budget.
    pub async fn set_budget(pool: &PgPool, user_id: Uuid, budget_cents: i64) -> sqlx::Result<Self> {
        sqlx::query_as(
            r#"INSERT INTO overage_budgets (user_id, period_start, budget_cents)
               VALUES ($1, date_trunc('month', now())::date, $2)
               ON CONFLICT (user_id, period_start)
               DO UPDATE SET budget_cents = EXCLUDED.budget_cents
               RETURNING *"#,
        )
        .bind(user_id)
        .bind(budget_cents)
        .fetch_one(pool)
        .await
    }
}

// ── AgentChannel ───────────────────────────────────────────────────

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AgentChannel {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub channel_kind: String,
    pub credentials: serde_json::Value,
    pub enabled: bool,
    pub webhook_secret: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentChannel {
    fn generate_webhook_secret() -> String {
        use rand::Rng;
        let bytes: [u8; 32] = rand::rng().random();
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub async fn insert(
        pool: &PgPool,
        agent_id: Uuid,
        channel_kind: &str,
        credentials: &serde_json::Value,
    ) -> sqlx::Result<Self> {
        let webhook_secret = Self::generate_webhook_secret();
        sqlx::query_as(
            r#"INSERT INTO agent_channels (agent_id, channel_kind, credentials, webhook_secret)
               VALUES ($1, $2, $3, $4)
               RETURNING *"#,
        )
        .bind(agent_id)
        .bind(channel_kind)
        .bind(credentials)
        .bind(&webhook_secret)
        .fetch_one(pool)
        .await
    }

    pub async fn get_by_agent_and_kind(
        pool: &PgPool,
        agent_id: Uuid,
        channel_kind: &str,
    ) -> sqlx::Result<Self> {
        sqlx::query_as(
            "SELECT * FROM agent_channels WHERE agent_id = $1 AND channel_kind = $2",
        )
        .bind(agent_id)
        .bind(channel_kind)
        .fetch_one(pool)
        .await
    }

    pub async fn list_for_agent(pool: &PgPool, agent_id: Uuid) -> sqlx::Result<Vec<Self>> {
        sqlx::query_as(
            "SELECT * FROM agent_channels WHERE agent_id = $1 ORDER BY channel_kind",
        )
        .bind(agent_id)
        .fetch_all(pool)
        .await
    }

    pub async fn update_credentials(
        pool: &PgPool,
        id: Uuid,
        credentials: &serde_json::Value,
    ) -> sqlx::Result<Self> {
        sqlx::query_as(
            "UPDATE agent_channels SET credentials = $1 WHERE id = $2 RETURNING *",
        )
        .bind(credentials)
        .bind(id)
        .fetch_one(pool)
        .await
    }

    pub async fn delete_by_agent_and_kind(
        pool: &PgPool,
        agent_id: Uuid,
        channel_kind: &str,
    ) -> sqlx::Result<()> {
        sqlx::query(
            "DELETE FROM agent_channels WHERE agent_id = $1 AND channel_kind = $2",
        )
        .bind(agent_id)
        .bind(channel_kind)
        .execute(pool)
        .await?;
        Ok(())
    }
}
