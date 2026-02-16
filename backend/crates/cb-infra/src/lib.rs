pub mod fly;
pub mod hetzner;
pub mod sprites;
pub mod types;

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use types::{VpsId, VpsInfo, VpsSpec};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("fly provider error: {0}")]
    Fly(#[from] fly_api::Error),

    #[error("hetzner api error: {0}")]
    HetznerApi(String),

    #[error("sprites api error: {0}")]
    Sprites(#[from] sprites_api::Error),

    #[error("sprites provisioning error: {0}")]
    SpritesProvisioning(String),

    #[error("invalid id: {0}")]
    InvalidId(String),

    #[error("missing env var: {0}")]
    MissingEnv(String),

    #[error("unknown provider: {0}")]
    UnknownProvider(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Known VPS provider backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderName {
    Fly,
    Hetzner,
    Sprites,
}

impl ProviderName {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fly => "fly",
            Self::Hetzner => "hetzner",
            Self::Sprites => "sprites",
        }
    }

    /// Which resources this provider meters on a usage basis.
    pub fn metered_resources(&self) -> MeteredResources {
        match self {
            Self::Fly | Self::Hetzner => MeteredResources::BANDWIDTH_ONLY,
            Self::Sprites => MeteredResources::BANDWIDTH_ONLY,
        }
    }
}

impl fmt::Display for ProviderName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ProviderName {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "fly" => Ok(Self::Fly),
            "hetzner" => Ok(Self::Hetzner),
            "sprites" => Ok(Self::Sprites),
            other => Err(Error::UnknownProvider(other.to_string())),
        }
    }
}

/// Describes which resource axes a provider meters on a usage basis.
///
/// Fixed-allocation providers (Fly, Hetzner) only meter bandwidth — the VPS
/// gets dedicated CPU/memory. Elastic providers (Sprites, K8s) meter all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeteredResources {
    pub bandwidth: bool,
    pub cpu: bool,
    pub memory: bool,
}

impl MeteredResources {
    /// All resources metered (elastic providers).
    pub const ALL: Self = Self {
        bandwidth: true,
        cpu: true,
        memory: true,
    };

    /// Only bandwidth metered (fixed-resource providers).
    pub const BANDWIDTH_ONLY: Self = Self {
        bandwidth: true,
        cpu: false,
        memory: false,
    };
}

/// Return metering policy for a provider identified by its name string.
///
/// Used by the proxy and monitor, which only have the `vps.provider` string
/// (not `Arc<dyn VpsProvider>`). Delegates to `ProviderName::metered_resources()`.
///
/// Unknown providers default to ALL (safest: over-enforce rather than under-enforce).
pub fn metered_resources_for(provider: &str) -> MeteredResources {
    match provider.parse::<ProviderName>() {
        Ok(name) => name.metered_resources(),
        Err(_) => {
            tracing::warn!(provider, "unknown provider, defaulting to ALL metering");
            MeteredResources::ALL
        }
    }
}

/// Backend-agnostic interface for managing agent VPSes.
///
/// Each provider (Fly.io, Hetzner, Sprites) implements this trait and owns its
/// own configuration, loaded from environment variables at construction.
#[async_trait]
pub trait VpsProvider: Send + Sync + 'static {
    /// Create and start a VPS with the given spec. Storage is provider-managed.
    async fn create_vps(&self, spec: &VpsSpec) -> Result<VpsInfo>;

    /// Start a stopped VPS.
    async fn start_vps(&self, id: &VpsId) -> Result<()>;

    /// Stop a running VPS.
    async fn stop_vps(&self, id: &VpsId) -> Result<()>;

    /// Destroy a VPS permanently.
    async fn destroy_vps(&self, id: &VpsId) -> Result<()>;

    /// Get current VPS status and metadata.
    async fn get_vps(&self, id: &VpsId) -> Result<VpsInfo>;

    /// Provider identifier.
    fn name(&self) -> ProviderName;

    /// Which resources this provider meters on a usage basis.
    /// Default delegates to `ProviderName::metered_resources()`.
    fn metered_resources(&self) -> MeteredResources {
        self.name().metered_resources()
    }
}

/// Registry of all configured VPS providers.
///
/// Each provider is constructed from environment variables at startup.
/// Providers whose required env vars are missing are silently skipped.
#[derive(Clone)]
pub struct ProviderRegistry {
    providers: HashMap<ProviderName, Arc<dyn VpsProvider>>,
}

impl ProviderRegistry {
    /// Look up a provider by name.
    pub fn get(&self, name: ProviderName) -> Option<&Arc<dyn VpsProvider>> {
        self.providers.get(&name)
    }

    /// List the names of all available providers.
    pub fn available(&self) -> Vec<ProviderName> {
        self.providers.keys().copied().collect()
    }

    /// Returns `true` if at least one provider is configured.
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

/// Build all providers whose required env vars are present.
///
/// Providers with missing configuration are skipped with a debug log.
/// Returns an error only if no providers could be constructed at all.
pub fn build_providers() -> Result<ProviderRegistry> {
    dotenvy::dotenv().ok();

    let mut providers: HashMap<ProviderName, Arc<dyn VpsProvider>> = HashMap::new();

    match fly::FlyProvider::from_env() {
        Ok(p) => {
            tracing::info!("registered Fly.io VPS provider");
            providers.insert(ProviderName::Fly, Arc::new(p));
        }
        Err(e) => tracing::debug!("skipping Fly.io provider: {e}"),
    }

    match hetzner::HetznerProvider::from_env() {
        Ok(p) => {
            tracing::info!("registered Hetzner Cloud VPS provider");
            providers.insert(ProviderName::Hetzner, Arc::new(p));
        }
        Err(e) => tracing::debug!("skipping Hetzner provider: {e}"),
    }

    match sprites::SpritesProvider::from_env() {
        Ok(p) => {
            tracing::info!("registered Sprites VPS provider");
            providers.insert(ProviderName::Sprites, Arc::new(p));
        }
        Err(e) => tracing::debug!("skipping Sprites provider: {e}"),
    }

    if providers.is_empty() {
        return Err(Error::MissingEnv(
            "no VPS providers configured (set FLY_API_TOKEN, HETZNER_API_TOKEN, and/or SPRITES_API_TOKEN)".into(),
        ));
    }

    Ok(ProviderRegistry { providers })
}
