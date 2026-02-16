use cb_infra::ProviderRegistry;
use sqlx::PgPool;

use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub providers: ProviderRegistry,
    pub config: AppConfig,
}
