use async_trait::async_trait;
use hcloud::apis::configuration::Configuration;
use hcloud::apis::servers_api;
use hcloud::models;
use tracing::{info, warn};

use crate::{Error, ProviderName, Result, VpsProvider};
use crate::types::{VpsId, VpsInfo, VpsSpec, VpsState};

/// Hetzner Cloud provider using the `hcloud` crate.
///
/// All configuration is loaded from environment variables via `from_env()`.
pub struct HetznerProvider {
    config: Configuration,
    location: String,
    network_id: Option<i64>,
    firewall_id: Option<i64>,
    ssh_key_names: Vec<String>,
}

impl HetznerProvider {
    /// Create from env vars:
    ///
    /// - `HETZNER_API_TOKEN` (required)
    /// - `HETZNER_LOCATION` (default: `"fsn1"`)
    /// - `HETZNER_NETWORK_ID` (optional, integer)
    /// - `HETZNER_FIREWALL_ID` (optional, integer)
    /// - `HETZNER_SSH_KEY_NAMES` (comma-separated names, optional)
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok();

        let token = std::env::var("HETZNER_API_TOKEN")
            .map_err(|_| Error::MissingEnv("HETZNER_API_TOKEN".into()))?;

        let mut config = Configuration::new();
        config.bearer_access_token = Some(token);

        let location = std::env::var("HETZNER_LOCATION").unwrap_or_else(|_| "fsn1".into());

        let network_id = std::env::var("HETZNER_NETWORK_ID")
            .ok()
            .and_then(|s| s.parse::<i64>().ok());

        let firewall_id = std::env::var("HETZNER_FIREWALL_ID")
            .ok()
            .and_then(|s| s.parse::<i64>().ok());

        let ssh_key_names: Vec<String> = std::env::var("HETZNER_SSH_KEY_NAMES")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Self {
            config,
            location,
            network_id,
            firewall_id,
            ssh_key_names,
        })
    }

    fn parse_state(status: &models::server::Status) -> VpsState {
        match status {
            models::server::Status::Running => VpsState::Running,
            models::server::Status::Initializing | models::server::Status::Starting => {
                VpsState::Starting
            }
            models::server::Status::Off | models::server::Status::Stopping => VpsState::Stopped,
            models::server::Status::Deleting => VpsState::Destroyed,
            _ => VpsState::Unknown,
        }
    }

    /// Map CPU millicores to a Hetzner server type.
    fn server_type(cpu_millicores: i32, memory_mb: i32) -> &'static str {
        match (cpu_millicores, memory_mb) {
            (0..=1000, 0..=2048) => "cpx11",
            (0..=2000, 0..=4096) => "cpx21",
            (0..=4000, 0..=8192) => "cpx31",
            _ => "cpx41",
        }
    }

    /// Generate cloud-init user data for setting up the agent.
    fn cloud_init_user_data(spec: &VpsSpec) -> String {
        let mut env_lines = String::new();
        for (k, v) in &spec.env {
            env_lines.push_str(&format!("echo '{k}={v}' >> /etc/cludbox/env\n"));
        }

        let mut file_lines = String::new();
        for f in &spec.files {
            file_lines.push_str(&format!(
                "mkdir -p $(dirname {})\ncat > {} << 'CLUDBOX_EOF'\n{}\nCLUDBOX_EOF\n",
                f.guest_path, f.guest_path, f.raw_value
            ));
        }

        format!(
            r#"#cloud-config
runcmd:
  - mkdir -p /etc/cludbox
  - {env_lines}
  - {file_lines}
  - systemctl start cludbox-agent
"#
        )
    }

    /// Extract the private IP from a server's private_net list.
    fn private_ip(server: &models::Server) -> Option<String> {
        server
            .private_net
            .first()
            .and_then(|net| net.ip.clone())
    }

    fn parse_id(raw: &str) -> Result<i64> {
        raw.parse::<i64>()
            .map_err(|_| Error::InvalidId(raw.to_string()))
    }
}

#[async_trait]
impl VpsProvider for HetznerProvider {
    async fn create_vps(&self, spec: &VpsSpec) -> Result<VpsInfo> {
        let server_type = Self::server_type(spec.cpu_millicores, spec.memory_mb);
        let user_data = Self::cloud_init_user_data(spec);

        let firewalls = self.firewall_id.map(|fw_id| {
            vec![models::CreateServerRequestFirewalls {
                firewall: fw_id,
            }]
        });

        let ssh_keys = if self.ssh_key_names.is_empty() {
            None
        } else {
            Some(self.ssh_key_names.clone())
        };

        let resp = servers_api::create_server(
            &self.config,
            servers_api::CreateServerParams {
                create_server_request: models::CreateServerRequest {
                    name: spec.name.clone(),
                    server_type: server_type.into(),
                    image: spec.image.clone(),
                    location: Some(self.location.clone()),
                    user_data: Some(user_data),
                    networks: self.network_id.map(|id| vec![id]),
                    firewalls,
                    ssh_keys,
                    volumes: None,
                    start_after_create: Some(true),
                    automount: None,
                    datacenter: None,
                    labels: None,
                    placement_group: None,
                    public_net: None,
                },
            },
        )
        .await
        .map_err(|e| Error::HetznerApi(format!("create server: {e}")))?;

        let server = resp.server;
        let address = Self::private_ip(&server);

        info!(server_id = server.id, "hetzner: server created");

        Ok(VpsInfo {
            id: VpsId(server.id.to_string()),
            state: Self::parse_state(&server.status),
            address,
        })
    }

    async fn start_vps(&self, id: &VpsId) -> Result<()> {
        let server_id = Self::parse_id(&id.0)?;

        servers_api::power_on_server(
            &self.config,
            servers_api::PowerOnServerParams { id: server_id },
        )
        .await
        .map_err(|e| Error::HetznerApi(format!("power on server: {e}")))?;

        info!(server_id = %id.0, "hetzner: server started");
        Ok(())
    }

    async fn stop_vps(&self, id: &VpsId) -> Result<()> {
        let server_id = Self::parse_id(&id.0)?;

        servers_api::shutdown_server(
            &self.config,
            servers_api::ShutdownServerParams { id: server_id },
        )
        .await
        .map_err(|e| Error::HetznerApi(format!("shutdown server: {e}")))?;

        info!(server_id = %id.0, "hetzner: server stopped");
        Ok(())
    }

    async fn destroy_vps(&self, id: &VpsId) -> Result<()> {
        let server_id = Self::parse_id(&id.0)?;

        if let Err(e) = servers_api::delete_server(
            &self.config,
            servers_api::DeleteServerParams { id: server_id },
        )
        .await
        {
            let msg = format!("{e}");
            if msg.contains("404") {
                warn!(server_id = %id.0, "hetzner: server already destroyed");
                return Ok(());
            }
            return Err(Error::HetznerApi(format!("delete server: {e}")));
        }

        info!(server_id = %id.0, "hetzner: server destroyed");
        Ok(())
    }

    async fn get_vps(&self, id: &VpsId) -> Result<VpsInfo> {
        let server_id = Self::parse_id(&id.0)?;

        let resp = servers_api::get_server(
            &self.config,
            servers_api::GetServerParams { id: server_id },
        )
        .await
        .map_err(|e| Error::HetznerApi(format!("get server: {e}")))?;

        let server = resp
            .server
            .ok_or_else(|| Error::HetznerApi("server not found in response".into()))?;

        let address = Self::private_ip(&server);

        Ok(VpsInfo {
            id: VpsId(server.id.to_string()),
            state: Self::parse_state(&server.status),
            address,
        })
    }

    fn name(&self) -> ProviderName {
        ProviderName::Hetzner
    }
}
