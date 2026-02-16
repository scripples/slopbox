mod auth;
mod config;
mod dto;
mod error;
mod gateway_proxy;
mod monitor;
mod openclaw_config;
mod proxy;
mod routes;
mod state;

use std::sync::Arc;

use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

use crate::config::AppConfig;
use crate::monitor::{StubCollector, spawn_monitor};
use crate::routes::api_router;
use crate::state::AppState;

#[tokio::main]
async fn main() {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let config = AppConfig::from_env();

    // Database
    let db = cb_db::create_pool(&config.database_url)
        .await
        .expect("failed to connect to database");

    cb_db::run_migrations(&db)
        .await
        .expect("failed to run migrations");

    // VPS providers
    let providers = cb_infra::build_providers().expect("failed to build VPS providers");
    tracing::info!(providers = ?providers.available(), "VPS providers ready");

    // Forward proxy
    proxy::spawn_proxy(config.proxy_listen_addr, db.clone());

    // Background monitor
    let collector = Arc::new(StubCollector);
    spawn_monitor(db.clone(), collector, providers.clone(), config.monitor_interval_secs);

    let state = AppState {
        db,
        providers,
        config: config.clone(),
    };

    let app = api_router(state).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .expect("failed to bind listener");

    tracing::info!(addr = %config.listen_addr, "starting control plane API");

    axum::serve(listener, app).await.expect("server error");
}
