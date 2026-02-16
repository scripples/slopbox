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

fn sprites_client(state: &AppState) -> Result<&sprites_api::SpritesClient, ApiError> {
    state
        .sprites_client
        .as_ref()
        .ok_or_else(|| ApiError::Internal("sprites client not configured".into()))
}

/// PUT /agents/{id}/config
///
/// Rebuild openclaw.json with overrides and apply.
/// For sprites: writes config file via exec and restarts the service.
/// For other providers: applies via gateway config.patch RPC (not yet implemented).
pub async fn update_config(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<UpdateConfigRequest>,
) -> Result<StatusCode, ApiError> {
    let (agent, vps) = get_running_agent_vps(&state, user_id.0, agent_id).await?;

    let config = openclaw_config::build_openclaw_config(&ConfigParams {
        agent_id,
        model: req.model,
        tools_deny: req.tools_deny,
    });
    let config_json = openclaw_config::render_openclaw_config(&config);

    if vps.provider == "sprites" {
        let client = sprites_client(&state)?;
        let vm_id = vps
            .provider_vm_id
            .as_deref()
            .ok_or_else(|| ApiError::Internal("VPS has no provider VM ID".into()))?;

        // Write config file
        let result = client
            .exec(
                vm_id,
                &["tee", "/root/.openclaw/openclaw.json"],
                Some(&config_json),
            )
            .await
            .map_err(|e| ApiError::Internal(format!("failed to write config: {e}")))?;

        if result.exit_code.unwrap_or(-1) != 0 {
            return Err(ApiError::Internal(format!(
                "failed to write config: {}",
                result.stderr.unwrap_or_default()
            )));
        }

        // Restart service: stop then start
        let _ = client.stop_service(vm_id, "openclaw", None).await;
        client
            .start_service(vm_id, "openclaw")
            .await
            .map_err(|e| ApiError::Internal(format!("failed to start service: {e}")))?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        // Hetzner and other providers: write config via gateway tools/invoke
        let address = vps_address(&vps)?;
        let url = format!("http://{address}:{GATEWAY_PORT}/tools/invoke");

        let payload = serde_json::json!({
            "tool": "write",
            "params": {
                "path": "/root/.openclaw/openclaw.json",
                "content": config_json,
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
}

/// PUT /agents/{id}/workspace/{filename}
///
/// Write a workspace file (allowlisted).
/// Primary path: gateway /tools/invoke write tool.
/// Sprites fallback: direct exec write.
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

    if vps.provider == "sprites" {
        // Sprites: write directly via exec
        let client = sprites_client(&state)?;
        let vm_id = vps
            .provider_vm_id
            .as_deref()
            .ok_or_else(|| ApiError::Internal("VPS has no provider VM ID".into()))?;

        let path = format!("/root/.openclaw/workspace/{filename}");

        // Ensure directory exists
        let _ = client
            .exec(vm_id, &["mkdir", "-p", "/root/.openclaw/workspace"], None)
            .await;

        let result = client
            .exec(vm_id, &["tee", &path], Some(&req.content))
            .await
            .map_err(|e| ApiError::Internal(format!("failed to write file: {e}")))?;

        if result.exit_code.unwrap_or(-1) != 0 {
            return Err(ApiError::Internal(format!(
                "failed to write file: {}",
                result.stderr.unwrap_or_default()
            )));
        }

        Ok(StatusCode::NO_CONTENT)
    } else {
        // Other providers: use gateway HTTP API
        let address = vps_address(&vps)?;
        let url = format!("http://{address}:{GATEWAY_PORT}/tools/invoke");

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
}

/// POST /agents/{id}/restart
///
/// Restart OpenClaw gateway.
/// For sprites: stop + start the openclaw service.
/// For other providers: not yet implemented.
pub async fn restart_agent(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let (agent, vps) = get_running_agent_vps(&state, user_id.0, agent_id).await?;

    if vps.provider == "sprites" {
        let client = sprites_client(&state)?;
        let vm_id = vps
            .provider_vm_id
            .as_deref()
            .ok_or_else(|| ApiError::Internal("VPS has no provider VM ID".into()))?;

        let _ = client.stop_service(vm_id, "openclaw", None).await;
        client
            .start_service(vm_id, "openclaw")
            .await
            .map_err(|e| ApiError::Internal(format!("failed to start service: {e}")))?;

        Ok(StatusCode::NO_CONTENT)
    } else if vps.provider == "hetzner" {
        // Hetzner: restart via provider API (reboot the server)
        let provider_name: cb_infra::ProviderName = "hetzner"
            .parse()
            .map_err(|_| ApiError::Internal("unknown provider".into()))?;
        let provider = state
            .providers
            .get(provider_name)
            .ok_or_else(|| ApiError::Internal("hetzner provider not configured".into()))?;

        let vm_id = vps
            .provider_vm_id
            .as_deref()
            .ok_or_else(|| ApiError::Internal("VPS has no provider VM ID".into()))?;

        // Stop + start = restart
        let _ = provider
            .stop_vps(&cb_infra::types::VpsId(vm_id.to_string()))
            .await;
        provider
            .start_vps(&cb_infra::types::VpsId(vm_id.to_string()))
            .await?;

        Ok(StatusCode::NO_CONTENT)
    } else {
        let _ = (agent.gateway_token, GATEWAY_PORT);
        Err(ApiError::Internal(
            "restart not yet implemented for this provider".into(),
        ))
    }
}

/// GET /agents/{id}/health
///
/// Check gateway health.
/// For sprites: check sprite + service state via API.
/// For other providers: HTTP health check against gateway.
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

    if vps.provider == "sprites" {
        let client = sprites_client(&state)?;
        let vm_id = vps
            .provider_vm_id
            .as_deref()
            .ok_or_else(|| ApiError::Internal("VPS has no provider VM ID".into()))?;

        let reachable = match client.get_service(vm_id, "openclaw").await {
            Ok(service) => service
                .state
                .as_ref()
                .is_some_and(|s| s.status == "running"),
            Err(_) => false,
        };

        Ok(Json(AgentHealthResponse {
            gateway_reachable: reachable,
        }))
    } else {
        let address = vps_address(&vps)?;
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
}
