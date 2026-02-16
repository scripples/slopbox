pub mod admin;
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

use crate::auth::{admin_middleware, auth_middleware, status_middleware};
use crate::state::AppState;

pub fn api_router(state: AppState) -> Router {
    // Admin routes — require admin role
    let admin_routes = Router::new()
        .route("/admin/users", get(admin::list_users))
        .route("/admin/users/{id}/status", put(admin::set_user_status))
        .route("/admin/users/{id}/role", put(admin::set_user_role))
        .route("/admin/vpses", get(admin::list_vpses))
        .route("/admin/vpses/{id}/stop", post(admin::stop_vps))
        .route("/admin/vpses/{id}/destroy", post(admin::destroy_vps))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            admin_middleware,
        ));

    // Routes that require active status
    let active_routes = Router::new()
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
        // Status middleware — rejects non-active users (applied first, runs second)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            status_middleware,
        ));

    // Routes accessible to any authenticated user (including pending)
    let authed_routes = Router::new()
        .route("/users/me", get(users::get_me))
        .route("/plans", get(plans::list_plans))
        .merge(active_routes)
        .merge(admin_routes);

    // All authed routes get auth middleware
    let authed = authed_routes.layer(middleware::from_fn_with_state(
        state.clone(),
        auth_middleware,
    ));

    let gateway = crate::gateway_proxy::gateway_router();

    Router::new()
        .merge(authed)
        .merge(gateway)
        .with_state(state)
}
