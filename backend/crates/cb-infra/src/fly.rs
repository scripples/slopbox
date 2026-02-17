use async_trait::async_trait;
use tracing::info;

use crate::types::{VpsId, VpsInfo, VpsSpec, VpsState};
use crate::{Error, ProviderName, Result, VpsProvider};

/// Fly.io Machines API provider.
///
/// Delegates to `fly_api::FlyClient` for all HTTP calls.
pub struct FlyProvider {
    client: fly_api::FlyClient,
    region: String,
}

impl FlyProvider {
    /// Create from env vars: `FLY_API_TOKEN` (required), `FLY_APP_NAME`, `FLY_REGION`.
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let token = std::env::var("FLY_API_TOKEN")
            .map_err(|_| Error::MissingEnv("FLY_API_TOKEN".into()))?;
        let app = std::env::var("FLY_APP_NAME").unwrap_or_else(|_| "slopbox-agents".into());
        let region = std::env::var("FLY_REGION").unwrap_or_else(|_| "iad".into());

        Ok(Self {
            client: fly_api::FlyClient::new(token, app),
            region,
        })
    }

    fn parse_state(state: &str) -> VpsState {
        match state {
            "started" => VpsState::Running,
            "starting" => VpsState::Starting,
            "stopped" => VpsState::Stopped,
            "destroyed" | "destroying" => VpsState::Destroyed,
            _ => VpsState::Unknown,
        }
    }

    /// Map CPU millicores to Fly performance preset.
    fn guest_config(cpu_millicores: i32, memory_mb: i32) -> fly_api::GuestConfig {
        let (cpus, cpu_kind) = match cpu_millicores {
            0..=1000 => (1, "shared"),
            1001..=2000 => (2, "shared"),
            2001..=4000 => (2, "performance"),
            _ => (4, "performance"),
        };
        fly_api::GuestConfig {
            cpus,
            cpu_kind: cpu_kind.into(),
            memory_mb: memory_mb as u32,
        }
    }
}

#[async_trait]
impl VpsProvider for FlyProvider {
    async fn create_vps(&self, spec: &VpsSpec) -> Result<VpsInfo> {
        let files: Option<Vec<fly_api::MachineFile>> = if spec.files.is_empty() {
            None
        } else {
            Some(
                spec.files
                    .iter()
                    .map(|f| fly_api::MachineFile {
                        guest_path: f.guest_path.clone(),
                        raw_value: f.raw_value.clone(),
                    })
                    .collect(),
            )
        };

        let machine = self
            .client
            .create_machine(&fly_api::CreateMachineRequest {
                name: spec.name.clone(),
                region: self.region.clone(),
                config: fly_api::MachineConfig {
                    image: spec.image.clone().unwrap_or_else(|| "ubuntu:24.04".into()),
                    env: Some(spec.env.clone()),
                    guest: Self::guest_config(spec.cpu_millicores, spec.memory_mb),
                    mounts: None,
                    files,
                    auto_destroy: Some(false),
                },
            })
            .await?;

        let app = self.client.app();
        let address = machine
            .private_ip
            .clone()
            .or_else(|| Some(format!("{}.vm.{app}.internal", machine.id)));

        info!(machine_id = %machine.id, state = %machine.state, "fly: machine created");

        Ok(VpsInfo {
            id: VpsId(machine.id),
            state: Self::parse_state(&machine.state),
            address,
        })
    }

    async fn start_vps(&self, id: &VpsId) -> Result<()> {
        self.client.start_machine(&id.0).await?;
        info!(machine_id = %id.0, "fly: machine started");
        Ok(())
    }

    async fn stop_vps(&self, id: &VpsId) -> Result<()> {
        self.client.stop_machine(&id.0).await?;
        info!(machine_id = %id.0, "fly: machine stopped");
        Ok(())
    }

    async fn destroy_vps(&self, id: &VpsId) -> Result<()> {
        self.client.delete_machine(&id.0).await?;
        info!(machine_id = %id.0, "fly: machine destroyed");
        Ok(())
    }

    async fn get_vps(&self, id: &VpsId) -> Result<VpsInfo> {
        let machine = self.client.get_machine(&id.0).await?;
        let app = self.client.app();
        let address = machine
            .private_ip
            .clone()
            .or_else(|| Some(format!("{}.vm.{app}.internal", machine.id)));

        Ok(VpsInfo {
            id: VpsId(machine.id),
            state: Self::parse_state(&machine.state),
            address,
        })
    }

    fn name(&self) -> ProviderName {
        ProviderName::Fly
    }
}
