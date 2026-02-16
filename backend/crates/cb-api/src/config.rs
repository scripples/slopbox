use std::env;
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub database_url: String,
    pub listen_addr: SocketAddr,
    pub jwt_secret: String,
    pub frontend_origin: String,
    pub monitor_interval_secs: u64,
    pub proxy_listen_addr: SocketAddr,
    pub proxy_external_addr: String,
}

impl AppConfig {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL").expect("DATABASE_URL must be set"),
            listen_addr: env::var("LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:8080".into())
                .parse()
                .expect("LISTEN_ADDR must be a valid socket address"),
            jwt_secret: env::var("JWT_SECRET").expect("JWT_SECRET must be set"),
            frontend_origin: env::var("FRONTEND_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:3000".into()),
            monitor_interval_secs: env::var("MONITOR_INTERVAL_SECS")
                .unwrap_or_else(|_| "60".into())
                .parse()
                .expect("MONITOR_INTERVAL_SECS must be a valid u64"),
            proxy_listen_addr: env::var("PROXY_LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3128".into())
                .parse()
                .expect("PROXY_LISTEN_ADDR must be a valid socket address"),
            proxy_external_addr: env::var("PROXY_EXTERNAL_ADDR")
                .unwrap_or_else(|_| "cb-api:3128".into()),
        }
    }
}
