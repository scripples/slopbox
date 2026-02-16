use std::env;

use async_trait::async_trait;
use sprites_api::{CreateServiceRequest, CreateSpriteRequest, SpriteStatus, SpritesClient};

use crate::types::{FileMount, VpsId, VpsInfo, VpsSpec, VpsState};
use crate::{Error, ProviderName, Result, VpsProvider};

const SERVICE_NAME: &str = "openclaw";
const GATEWAY_PORT: u16 = 18789;

pub struct SpritesProvider {
    client: SpritesClient,
}

impl SpritesProvider {
    pub fn from_env() -> Result<Self> {
        let token = env::var("SPRITES_API_TOKEN")
            .map_err(|_| Error::MissingEnv("SPRITES_API_TOKEN".into()))?;

        Ok(Self {
            client: SpritesClient::new(token),
        })
    }

    /// Execute a command and return the result, failing on non-zero exit.
    async fn exec_checked(&self, sprite: &str, cmd: &[&str]) -> Result<String> {
        let result = self.client.exec(sprite, cmd, None).await?;

        let exit_code = result.exit_code.unwrap_or(-1);
        if exit_code != 0 {
            let stderr = result.stderr.unwrap_or_default();
            let stdout = result.stdout.unwrap_or_default();
            return Err(Error::SpritesProvisioning(format!(
                "command {:?} failed (exit {}): {}{}",
                cmd, exit_code, stderr, stdout,
            )));
        }

        Ok(result.stdout.unwrap_or_default())
    }

    /// Write a file to the sprite via exec + stdin.
    async fn write_file(&self, sprite: &str, path: &str, content: &str) -> Result<()> {
        let result = self
            .client
            .exec(sprite, &["tee", path], Some(content))
            .await?;

        let exit_code = result.exit_code.unwrap_or(-1);
        if exit_code != 0 {
            let stderr = result.stderr.unwrap_or_default();
            return Err(Error::SpritesProvisioning(format!(
                "write to {path} failed (exit {exit_code}): {stderr}"
            )));
        }

        Ok(())
    }

    /// Install Docker on Ubuntu.
    async fn install_docker(&self, sprite: &str) -> Result<()> {
        self.exec_checked(
            sprite,
            &["sh", "-c", "apt-get update && apt-get install -y docker.io"],
        )
        .await?;

        // Start Docker daemon in background and wait for it
        self.exec_checked(
            sprite,
            &[
                "sh",
                "-c",
                "dockerd &>/dev/null & for i in $(seq 1 30); do docker info &>/dev/null && break; sleep 1; done; docker info &>/dev/null",
            ],
        )
        .await?;

        Ok(())
    }

    /// Install OpenClaw via npm.
    async fn install_openclaw(&self, sprite: &str) -> Result<()> {
        self.exec_checked(sprite, &["npm", "install", "-g", "@anthropic/openclaw"])
            .await?;
        Ok(())
    }
}

#[async_trait]
impl VpsProvider for SpritesProvider {
    async fn create_vps(&self, spec: &VpsSpec) -> Result<VpsInfo> {
        // 1. Create sprite
        let sprite = self
            .client
            .create_sprite(&CreateSpriteRequest {
                name: spec.name.clone(),
                url_settings: None,
            })
            .await?;

        let name = &sprite.name;

        // On any failure, clean up the sprite
        match self.provision_sprite(name, spec).await {
            Ok(info) => Ok(info),
            Err(e) => {
                tracing::error!(sprite = name, error = %e, "provisioning failed, cleaning up");
                let _ = self.client.delete_sprite(name).await;
                Err(e)
            }
        }
    }

    async fn start_vps(&self, id: &VpsId) -> Result<()> {
        self.client.start_service(&id.0, SERVICE_NAME).await?;
        Ok(())
    }

    async fn stop_vps(&self, id: &VpsId) -> Result<()> {
        self.client.stop_service(&id.0, SERVICE_NAME, None).await?;
        Ok(())
    }

    async fn destroy_vps(&self, id: &VpsId) -> Result<()> {
        self.client.delete_sprite(&id.0).await?;
        Ok(())
    }

    async fn get_vps(&self, id: &VpsId) -> Result<VpsInfo> {
        let sprite = self.client.get_sprite(&id.0).await?;

        // Check service state for more accurate status
        let state = match sprite.status {
            SpriteStatus::Running => {
                // Check if the service is actually running
                match self.client.get_service(&id.0, SERVICE_NAME).await {
                    Ok(service) => {
                        if let Some(svc_state) = &service.state {
                            if svc_state.status == "running" {
                                VpsState::Running
                            } else {
                                VpsState::Stopped
                            }
                        } else {
                            VpsState::Stopped
                        }
                    }
                    Err(_) => VpsState::Running,
                }
            }
            SpriteStatus::Warm | SpriteStatus::Cold => VpsState::Stopped,
        };

        Ok(VpsInfo {
            id: VpsId(sprite.name),
            state,
            address: Some(sprite.url),
        })
    }

    fn name(&self) -> ProviderName {
        ProviderName::Sprites
    }
}

impl SpritesProvider {
    async fn provision_sprite(&self, name: &str, spec: &VpsSpec) -> Result<VpsInfo> {
        // 2. Install Docker
        tracing::info!(sprite = name, "installing Docker");
        self.install_docker(name).await?;

        // 3. Install OpenClaw
        tracing::info!(sprite = name, "installing OpenClaw");
        self.install_openclaw(name).await?;

        // 4. Create directories and write files
        tracing::info!(
            sprite = name,
            files = spec.files.len(),
            "writing config files"
        );
        for FileMount {
            guest_path,
            raw_value,
        } in &spec.files
        {
            // Ensure parent directory exists
            if let Some(parent) = guest_path.rsplit_once('/').map(|(p, _)| p)
                && !parent.is_empty()
            {
                self.exec_checked(name, &["mkdir", "-p", parent]).await?;
            }
            self.write_file(name, guest_path, raw_value).await?;
        }

        // 5. Write env vars file
        if !spec.env.is_empty() {
            self.exec_checked(name, &["mkdir", "-p", "/etc/slopbox"])
                .await?;
            let env_content: String = spec
                .env
                .iter()
                .map(|(k, v)| format!("export {k}={v}\n"))
                .collect();
            self.write_file(name, "/etc/slopbox/env", &env_content)
                .await?;
        }

        // 6. Create and start the openclaw service
        tracing::info!(sprite = name, "creating openclaw service");
        let cmd = if spec.env.is_empty() {
            "exec openclaw gateway run".to_string()
        } else {
            "source /etc/slopbox/env && exec openclaw gateway run".to_string()
        };

        self.client
            .create_service(
                name,
                SERVICE_NAME,
                &CreateServiceRequest {
                    cmd: "sh".into(),
                    args: vec!["-c".into(), cmd],
                    needs: vec![],
                    http_port: Some(GATEWAY_PORT),
                },
            )
            .await?;

        tracing::info!(sprite = name, "starting openclaw service");
        self.client.start_service(name, SERVICE_NAME).await?;

        // Get the sprite URL for the address
        let sprite = self.client.get_sprite(name).await?;

        Ok(VpsInfo {
            id: VpsId(sprite.name),
            state: VpsState::Running,
            address: Some(sprite.url),
        })
    }
}
