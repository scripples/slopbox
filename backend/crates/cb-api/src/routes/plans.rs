use axum::Json;
use axum::extract::State;

use cb_db::models::Plan;

use crate::dto::PlanResponse;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn list_plans(
    State(state): State<AppState>,
) -> Result<Json<Vec<PlanResponse>>, ApiError> {
    let plans = Plan::list(&state.db).await?;
    let responses: Vec<PlanResponse> = plans.into_iter().map(PlanResponse::from).collect();
    Ok(Json(responses))
}
