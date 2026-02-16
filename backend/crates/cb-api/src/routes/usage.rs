use axum::extract::{Path, State};
use axum::{Extension, Json};
use uuid::Uuid;

use cb_db::models::{Agent, OverageBudget, Plan, User, Vps, VpsUsagePeriod};

use crate::auth::UserId;
use crate::dto::{OverageBudgetResponse, SetOverageBudgetRequest, UsageMetric, UsageResponse};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn get_usage(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<UsageResponse>, ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    let vps_id = agent.vps_id.ok_or(ApiError::NotFound)?;
    let vps = Vps::get_by_id(&state.db, vps_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    let user = User::get_by_id(&state.db, user_id.0).await?;
    let plan_id = user
        .plan_id
        .ok_or(ApiError::BadRequest("user has no plan".into()))?;
    let plan = Plan::get_by_id(&state.db, plan_id).await?;

    let period = VpsUsagePeriod::get_current(&state.db, vps_id).await?;
    let metering = cb_infra::metered_resources_for(&vps.provider);

    let bandwidth = UsageMetric {
        used: period.bandwidth_bytes,
        limit: plan.max_bandwidth_bytes,
        exceeded: period.bandwidth_bytes > plan.max_bandwidth_bytes,
    };

    let storage = UsageMetric {
        used: vps.storage_used_bytes,
        limit: plan.max_storage_bytes,
        exceeded: vps.storage_used_bytes > plan.max_storage_bytes,
    };

    let cpu = metering.cpu.then_some(UsageMetric {
        used: period.cpu_used_ms,
        limit: plan.max_cpu_ms,
        exceeded: period.cpu_used_ms > plan.max_cpu_ms,
    });

    let memory = metering.memory.then_some(UsageMetric {
        used: period.memory_used_mb_seconds,
        limit: plan.max_memory_mb_seconds,
        exceeded: period.memory_used_mb_seconds > plan.max_memory_mb_seconds,
    });

    // Compute overage info using aggregate user-level usage
    let aggregate = VpsUsagePeriod::get_user_aggregate(&state.db, user_id.0).await?;
    let overage_cost_cents = plan.overage_cost_cents(&aggregate);
    let budget = OverageBudget::get_current(&state.db, user_id.0).await?;

    let allowed = !bandwidth.exceeded
        && !storage.exceeded
        && !cpu.as_ref().is_some_and(|m| m.exceeded)
        && !memory.as_ref().is_some_and(|m| m.exceeded)
        || overage_cost_cents <= budget.budget_cents;

    Ok(Json(UsageResponse {
        allowed,
        bandwidth,
        storage,
        cpu,
        memory,
        overage_cost_cents,
        overage_budget_cents: budget.budget_cents,
    }))
}

pub async fn get_overage_budget(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
) -> Result<Json<OverageBudgetResponse>, ApiError> {
    let budget = OverageBudget::get_current(&state.db, user_id.0).await?;

    Ok(Json(OverageBudgetResponse {
        budget_cents: budget.budget_cents,
        period_start: budget.period_start,
    }))
}

pub async fn set_overage_budget(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Json(body): Json<SetOverageBudgetRequest>,
) -> Result<Json<OverageBudgetResponse>, ApiError> {
    let budget = OverageBudget::set_budget(&state.db, user_id.0, body.budget_cents).await?;

    Ok(Json(OverageBudgetResponse {
        budget_cents: budget.budget_cents,
        period_start: budget.period_start,
    }))
}
