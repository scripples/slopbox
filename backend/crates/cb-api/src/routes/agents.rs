use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use uuid::Uuid;

use cb_db::models::{Agent, Plan, User, Vps, VpsConfig, VpsState};

use crate::auth::UserId;
use crate::dto::{AgentResponse, CreateAgentRequest};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn create_agent(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Json(req): Json<CreateAgentRequest>,
) -> Result<(StatusCode, Json<AgentResponse>), ApiError> {
    let user = User::get_by_id(&state.db, user_id.0).await?;
    let plan_id = user
        .plan_id
        .ok_or(ApiError::LimitExceeded("user has no plan".into()))?;
    let plan = Plan::get_by_id(&state.db, plan_id).await?;

    let count = Agent::count_for_user(&state.db, user_id.0).await?;
    if count >= plan.max_agents as i64 {
        return Err(ApiError::LimitExceeded(format!(
            "agent limit reached ({}/{})",
            count, plan.max_agents
        )));
    }

    let agent = Agent::insert(&state.db, user_id.0, &req.name).await?;
    Ok((StatusCode::CREATED, Json(AgentResponse::from_agent(agent, None))))
}

pub async fn list_agents(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
) -> Result<Json<Vec<AgentResponse>>, ApiError> {
    let agents = Agent::list_for_user(&state.db, user_id.0).await?;
    let mut responses = Vec::with_capacity(agents.len());
    for agent in agents {
        let vps_with_provider = match agent.vps_id {
            Some(vps_id) => {
                if let Ok(vps) = Vps::get_by_id(&state.db, vps_id).await {
                    let provider = VpsConfig::get_by_id(&state.db, vps.vps_config_id)
                        .await
                        .map(|c| c.provider)
                        .unwrap_or_default();
                    Some((vps, provider))
                } else {
                    None
                }
            }
            None => None,
        };
        responses.push(AgentResponse::from_agent(agent, vps_with_provider));
    }
    Ok(Json(responses))
}

pub async fn get_agent(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(id): Path<Uuid>,
) -> Result<Json<AgentResponse>, ApiError> {
    let agent = Agent::get_by_id(&state.db, id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    let vps_with_provider = match agent.vps_id {
        Some(vps_id) => {
            if let Ok(vps) = Vps::get_by_id(&state.db, vps_id).await {
                let provider = VpsConfig::get_by_id(&state.db, vps.vps_config_id)
                    .await
                    .map(|c| c.provider)
                    .unwrap_or_default();
                Some((vps, provider))
            } else {
                None
            }
        }
        None => None,
    };

    Ok(Json(AgentResponse::from_agent(agent, vps_with_provider)))
}

pub async fn delete_agent(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let agent = Agent::get_by_id(&state.db, id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    // Destroy VPS if one is attached
    if let Some(vps_id) = agent.vps_id
        && let Ok(vps) = Vps::get_by_id(&state.db, vps_id).await
        && vps.state != VpsState::Destroyed
    {
        if let Some(ref vm_id) = vps.provider_vm_id
            && let Ok(config) = VpsConfig::get_by_id(&state.db, vps.vps_config_id).await
            && let Ok(name) = config.provider.parse::<cb_infra::ProviderName>()
            && let Some(provider) = state.providers.get(name)
        {
            let _ = provider
                .destroy_vps(&cb_infra::types::VpsId(vm_id.clone()))
                .await;
        }
        Vps::set_state(&state.db, vps.id, VpsState::Destroyed).await?;
    }

    Agent::delete(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
