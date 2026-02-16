pub mod agents;
pub mod channels;
pub mod config;
pub mod plans;
pub mod usage;
pub mod users;
pub mod vps;

use axum::Router;
use axum::middleware;
use axum::routing::{delete, get, post, put};

use crate::auth::auth_middleware;
use crate::state::AppState;

pub fn api_router(state: AppState) -> Router {
    let authed = Router::new()
        // Agents
        .route("/agents", post(agents::create_agent).get(agents::list_agents))
        .route(
            "/agents/{id}",
            get(agents::get_agent).delete(agents::delete_agent),
        )
        // VPS lifecycle
        .route(
            "/agents/{id}/vps",
            post(vps::provision_vps).delete(vps::destroy_vps),
        )
        .route("/agents/{id}/vps/start", post(vps::start_vps))
        .route("/agents/{id}/vps/stop", post(vps::stop_vps))
        // Channels
        .route(
            "/agents/{id}/channels",
            post(channels::add_channel).get(channels::list_channels),
        )
        .route(
            "/agents/{id}/channels/{kind}",
            delete(channels::remove_channel),
        )
        // Config targeting
        .route("/agents/{id}/config", put(config::update_config))
        .route(
            "/agents/{id}/workspace/{filename}",
            put(config::update_workspace_file),
        )
        .route("/agents/{id}/restart", post(config::restart_agent))
        .route("/agents/{id}/health", get(config::agent_health))
        // Usage
        .route("/agents/{id}/usage", get(usage::get_usage))
        // Overage budget
        .route(
            "/users/me/overage-budget",
            get(usage::get_overage_budget).put(usage::set_overage_budget),
        )
        // Users
        .route("/users/me", get(users::get_me))
        // Plans
        .route("/plans", get(plans::list_plans))
        // Auth middleware
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let gateway = crate::gateway_proxy::gateway_router();

    Router::new()
        .merge(authed)
        .merge(gateway)
        .with_state(state)
}
