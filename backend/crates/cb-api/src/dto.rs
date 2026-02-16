use chrono::{DateTime, NaiveDate, Utc};
use cb_db::models::{Agent, Plan, User, Vps, VpsState};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Requests ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ProvisionVpsRequest {
    pub vps_config_id: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct SetOverageBudgetRequest {
    pub budget_cents: i64,
}

// ── Responses ──────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AgentResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub vps: Option<VpsResponse>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AgentResponse {
    pub fn from_agent(agent: Agent, vps: Option<Vps>) -> Self {
        Self {
            id: agent.id,
            user_id: agent.user_id,
            name: agent.name,
            vps: vps.map(VpsResponse::from),
            created_at: agent.created_at,
            updated_at: agent.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct VpsResponse {
    pub id: Uuid,
    pub vps_config_id: Uuid,
    pub name: String,
    pub provider: String,
    pub state: VpsState,
    pub address: Option<String>,
    pub storage_used_bytes: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Vps> for VpsResponse {
    fn from(v: Vps) -> Self {
        Self {
            id: v.id,
            vps_config_id: v.vps_config_id,
            name: v.name,
            provider: v.provider,
            state: v.state,
            address: v.address,
            storage_used_bytes: v.storage_used_bytes,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct UsageMetric {
    pub used: i64,
    pub limit: i64,
    pub exceeded: bool,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub allowed: bool,
    pub bandwidth: UsageMetric,
    pub storage: UsageMetric,
    pub cpu: Option<UsageMetric>,
    pub memory: Option<UsageMetric>,
    pub overage_cost_cents: i64,
    pub overage_budget_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct OverageBudgetResponse {
    pub budget_cents: i64,
    pub period_start: NaiveDate,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub plan: Option<PlanResponse>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl UserResponse {
    pub fn from_user(user: User, plan: Option<Plan>) -> Self {
        Self {
            id: user.id,
            email: user.email,
            name: user.name,
            plan: plan.map(PlanResponse::from),
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlanResponse {
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
}

impl From<Plan> for PlanResponse {
    fn from(p: Plan) -> Self {
        Self {
            id: p.id,
            name: p.name,
            max_agents: p.max_agents,
            max_vpses: p.max_vpses,
            max_bandwidth_bytes: p.max_bandwidth_bytes,
            max_storage_bytes: p.max_storage_bytes,
            max_cpu_ms: p.max_cpu_ms,
            max_memory_mb_seconds: p.max_memory_mb_seconds,
            overage_bandwidth_cost_per_gb_cents: p.overage_bandwidth_cost_per_gb_cents,
            overage_cpu_cost_per_hour_cents: p.overage_cpu_cost_per_hour_cents,
            overage_memory_cost_per_gb_hour_cents: p.overage_memory_cost_per_gb_hour_cents,
        }
    }
}

// ── Channel DTOs ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddChannelRequest {
    pub channel_kind: String,
    pub credentials: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ChannelResponse {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub channel_kind: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<cb_db::models::AgentChannel> for ChannelResponse {
    fn from(channel: cb_db::models::AgentChannel) -> Self {
        Self {
            id: channel.id,
            agent_id: channel.agent_id,
            channel_kind: channel.channel_kind,
            enabled: channel.enabled,
            created_at: channel.created_at,
            updated_at: channel.updated_at,
        }
    }
}

// ── Config DTOs ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub model: Option<String>,
    pub tools_deny: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateWorkspaceFileRequest {
    pub content: String,
}
