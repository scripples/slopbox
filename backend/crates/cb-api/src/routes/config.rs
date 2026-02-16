use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use serde::Serialize;
use uuid::Uuid;

use cb_db::models::{Agent, Vps, VpsState};

use crate::auth::UserId;
use crate::dto::{UpdateConfigRequest, UpdateWorkspaceFileRequest};
use crate::error::ApiError;
use crate::openclaw_config::{self, ConfigParams};
use crate::state::AppState;

const GATEWAY_PORT: u16 = 18789;

const ALLOWED_WORKSPACE_FILES: &[&str] = &[
    "AGENTS.md",
    "SOUL.md",
    "IDENTITY.md",
    "TOOLS.md",
    "USER.md",
    "MEMORY.md",
    "BOOTSTRAP.md",
];

/// Validate ownership, VPS exists and is running, and return both.
async fn get_running_agent_vps(
    state: &AppState,
    user_id: Uuid,
    agent_id: Uuid,
) -> Result<(Agent, Vps), ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id {
        return Err(ApiError::NotFound);
    }

    let vps_id = agent.vps_id.ok_or(ApiError::NotFound)?;

    let vps = Vps::get_by_id(&state.db, vps_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if vps.state != VpsState::Running {
        return Err(ApiError::Conflict(format!(
            "VPS is not running (state: {})",
            serde_json::to_string(&vps.state)
                .unwrap_or_default()
                .trim_matches('"')
        )));
    }

    Ok((agent, vps))
}

fn vps_address(vps: &Vps) -> Result<&str, ApiError> {
    vps.address
        .as_deref()
        .ok_or_else(|| ApiError::Internal("VPS has no address".into()))
}

/// PUT /agents/{id}/config
///
/// Rebuild openclaw.json with overrides and apply via gateway config.patch RPC.
/// The gateway hot-reloads config changes — no restart needed.
pub async fn update_config(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<StatusCode, ApiError> {
    let (agent, vps) = get_running_agent_vps(&state, user_id.0, agent_id).await?;
    let _address = vps_address(&vps)?;

    let _config = openclaw_config::build_openclaw_config(&ConfigParams {
        agent_id,
        model: req.model,
        tools_deny: req.tools_deny,
    });

    // TODO: Implement gateway RPC client.
    //
    // The control plane connects directly to the gateway WebSocket at
    // ws://{address}:{GATEWAY_PORT}/ws, authenticates with the gateway token
    // (agent.gateway_token), and calls config.patch to apply the config diff.
    // The gateway hot-reloads channel/agent/tool config without restart.
    //
    // Flow:
    //   1. Connect to ws://{address}:18789/ws
    //   2. Complete connect handshake with gateway_token
    //   3. Send { type: "req", method: "config.patch", params: { patch: <config> } }
    //   4. Await { type: "res", ok: true }
    //   5. Close connection
    let _ = (agent.gateway_token, GATEWAY_PORT);

    Err(ApiError::Internal(
        "gateway RPC client not yet implemented".into(),
    ))
}

/// PUT /agents/{id}/workspace/{filename}
///
/// Write a workspace file (allowlisted) via gateway /tools/invoke write tool.
/// The workspace is bind-mounted into the sandbox at /workspace, so writes
/// via the gateway's tool dispatch reach the host filesystem.
pub async fn update_workspace_file(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path((agent_id, filename)): Path<(Uuid, String)>,
    Json(req): Json<UpdateWorkspaceFileRequest>,
) -> Result<StatusCode, ApiError> {
    if !ALLOWED_WORKSPACE_FILES.contains(&filename.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "file not allowed: {filename}"
        )));
    }

    let (agent, vps) = get_running_agent_vps(&state, user_id.0, agent_id).await?;
    let address = vps_address(&vps)?;

    // Write workspace file via gateway HTTP API.
    // POST /tools/invoke with { tool: "write", params: { path, content } }
    // The write tool targets the sandbox, but with workspaceAccess=rw the
    // workspace is mounted — writes to /workspace/{filename} land on the host.
    let url = format!(
        "http://{address}:{GATEWAY_PORT}/tools/invoke",
    );

    let payload = serde_json::json!({
        "tool": "write",
        "params": {
            "path": format!("/workspace/{filename}"),
            "content": req.content,
        }
    });

    let resp = reqwest::Client::new()
        .post(&url)
        .bearer_auth(&agent.gateway_token)
        .json(&payload)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("gateway request failed: {e}")))?;

    if resp.status().is_success() {
        Ok(StatusCode::NO_CONTENT)
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(ApiError::Internal(format!(
            "gateway returned {status}: {body}"
        )))
    }
}

/// POST /agents/{id}/restart
///
/// Restart OpenClaw gateway via systemctl on the VM.
/// With hot-reload, most config changes don't need this. Kept for cases
/// where a full restart is required (port/bind/TLS changes, plugin updates).
pub async fn restart_agent(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let (agent, vps) = get_running_agent_vps(&state, user_id.0, agent_id).await?;
    let _address = vps_address(&vps)?;

    // TODO: Implement gateway RPC client.
    //
    // Option A: Use config.apply RPC (replaces entire config + triggers restart).
    // Option B: Keep a minimal restart mechanism (e.g. provider reboot API).
    // Option C: Remove this endpoint entirely — hot-reload handles most cases.
    let _ = (agent.gateway_token, GATEWAY_PORT);

    Err(ApiError::Internal(
        "gateway RPC client not yet implemented".into(),
    ))
}

/// GET /agents/{id}/health
///
/// Check gateway health via its HTTP health endpoint.
#[derive(Serialize)]
pub struct AgentHealthResponse {
    pub gateway_reachable: bool,
}

pub async fn agent_health(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<AgentHealthResponse>, ApiError> {
    let (agent, vps) = get_running_agent_vps(&state, user_id.0, agent_id).await?;
    let address = vps_address(&vps)?;

    // Health check via gateway's own HTTP endpoint.
    // The gateway serves the Control UI at GET / on its main port.
    let reachable = reqwest::Client::new()
        .get(format!("http://{address}:{GATEWAY_PORT}/"))
        .bearer_auth(&agent.gateway_token)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .is_ok_and(|r| r.status().is_success() || r.status().is_redirection());

    Ok(Json(AgentHealthResponse {
        gateway_reachable: reachable,
    }))
}
