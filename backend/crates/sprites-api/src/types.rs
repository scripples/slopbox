use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Sprites ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sprite {
    pub id: String,
    pub name: String,
    pub organization: String,
    pub status: SpriteStatus,
    pub url: String,
    #[serde(default)]
    pub url_settings: Option<UrlSettings>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_active_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpriteStatus {
    Cold,
    Warm,
    Running,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlSettings {
    pub auth: UrlAuth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UrlAuth {
    Sprite,
    Public,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateSpriteRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_settings: Option<UrlSettings>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateSpriteRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url_settings: Option<UrlSettings>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListSpritesResponse {
    pub sprites: Vec<Sprite>,
    pub has_more: bool,
    #[serde(default)]
    pub next_continuation_token: Option<String>,
}

// ── Exec ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ExecSession {
    pub id: i64,
    pub command: String,
    pub created: DateTime<Utc>,
    pub is_active: bool,
    pub tty: bool,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub last_activity: Option<DateTime<Utc>>,
    #[serde(default)]
    pub bytes_per_second: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecResult {
    #[serde(default)]
    pub stdout: Option<String>,
    #[serde(default)]
    pub stderr: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

// ── Checkpoints ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub id: String,
    pub create_time: DateTime<Utc>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateCheckpointRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum StreamEvent {
    Info { data: String, time: DateTime<Utc> },
    Error { error: String, time: DateTime<Utc> },
    Complete { data: String, time: DateTime<Utc> },
}

// ── Network Policy ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicy {
    pub rules: Vec<NetworkPolicyRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPolicyRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<PolicyAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyAction {
    Allow,
    Deny,
}

// ── Services ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub http_port: Option<u16>,
    #[serde(default)]
    pub state: Option<ServiceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub pid: Option<i64>,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateServiceRequest {
    pub cmd: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub needs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_port: Option<u16>,
}

// ── Exec Kill (NDJSON events) ───────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
pub enum KillEvent {
    Signal { message: String, signal: String, pid: i64 },
    Timeout { message: String },
    Exited { message: String },
    Killed { message: String },
    Error { message: String },
    Complete { exit_code: i32 },
}
