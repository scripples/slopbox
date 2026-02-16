//! Typed Rust client for the Sprites API.
//!
//! Covers sprites (CRUD), exec (HTTP POST, list sessions, kill),
//! checkpoints (create, list, get, restore), network policy,
//! proxy metadata, and services.
//!
//! WebSocket endpoints (exec WS, proxy tunnel, attach) are out of scope —
//! this crate covers the HTTP REST surface only.

mod types;

pub use types::*;

const BASE_URL: &str = "https://api.sprites.dev/v1";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sprites api request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("sprites api {endpoint} returned {status}: {body}")]
    Api {
        endpoint: &'static str,
        status: reqwest::StatusCode,
        body: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Client for the Sprites REST API.
#[derive(Clone)]
pub struct SpritesClient {
    token: String,
    http: reqwest::Client,
}

impl SpritesClient {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            http: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{BASE_URL}{path}")
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn check(resp: reqwest::Response, endpoint: &'static str) -> Result<reqwest::Response> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                endpoint,
                status,
                body,
            });
        }
        Ok(resp)
    }

    async fn check_allow_404(
        resp: reqwest::Response,
        endpoint: &'static str,
    ) -> Result<reqwest::Response> {
        let status = resp.status();
        if !status.is_success() && status.as_u16() != 404 {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                endpoint,
                status,
                body,
            });
        }
        Ok(resp)
    }

    // ── Sprites ─────────────────────────────────────────────────────

    pub async fn create_sprite(&self, req: &CreateSpriteRequest) -> Result<Sprite> {
        let resp = self
            .http
            .post(self.url("/sprites"))
            .header("Authorization", self.auth())
            .json(req)
            .send()
            .await?;

        Self::check(resp, "create sprite")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn list_sprites(
        &self,
        prefix: Option<&str>,
        max_results: Option<u32>,
        continuation_token: Option<&str>,
    ) -> Result<ListSpritesResponse> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(p) = prefix {
            query.push(("prefix", p.to_string()));
        }
        if let Some(m) = max_results {
            query.push(("max_results", m.to_string()));
        }
        if let Some(t) = continuation_token {
            query.push(("continuation_token", t.to_string()));
        }

        let resp = self
            .http
            .get(self.url("/sprites"))
            .header("Authorization", self.auth())
            .query(&query)
            .send()
            .await?;

        Self::check(resp, "list sprites")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn get_sprite(&self, name: &str) -> Result<Sprite> {
        let resp = self
            .http
            .get(self.url(&format!("/sprites/{name}")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "get sprite")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn update_sprite(&self, name: &str, req: &UpdateSpriteRequest) -> Result<Sprite> {
        let resp = self
            .http
            .put(self.url(&format!("/sprites/{name}")))
            .header("Authorization", self.auth())
            .json(req)
            .send()
            .await?;

        Self::check(resp, "update sprite")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn delete_sprite(&self, name: &str) -> Result<()> {
        let resp = self
            .http
            .delete(self.url(&format!("/sprites/{name}")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check_allow_404(resp, "delete sprite").await?;
        Ok(())
    }

    // ── Exec ────────────────────────────────────────────────────────

    /// Execute a command via HTTP POST and return the result.
    ///
    /// For interactive/streaming exec, use a WebSocket client against
    /// `WSS /v1/sprites/{name}/exec` directly.
    pub async fn exec(
        &self,
        sprite: &str,
        cmd: &[&str],
        stdin_body: Option<&str>,
    ) -> Result<ExecResult> {
        let mut query: Vec<(&str, String)> = cmd.iter().map(|c| ("cmd", c.to_string())).collect();
        if stdin_body.is_some() {
            query.push(("stdin", "true".into()));
        }

        let mut builder = self
            .http
            .post(self.url(&format!("/sprites/{sprite}/exec")))
            .header("Authorization", self.auth())
            .query(&query);

        if let Some(body) = stdin_body {
            builder = builder.body(body.to_string());
        }

        let resp = builder.send().await?;

        Self::check(resp, "exec")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    /// List active exec sessions on a sprite.
    pub async fn list_exec_sessions(&self, sprite: &str) -> Result<Vec<ExecSession>> {
        let resp = self
            .http
            .get(self.url(&format!("/sprites/{sprite}/exec")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "list exec sessions")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    /// Kill an exec session by ID.
    ///
    /// Returns the raw NDJSON response body. Callers can parse lines as
    /// [`KillEvent`] for structured access.
    pub async fn kill_exec_session(
        &self,
        sprite: &str,
        session_id: i64,
        signal: Option<&str>,
        timeout: Option<&str>,
    ) -> Result<String> {
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(s) = signal {
            query.push(("signal", s));
        }
        if let Some(t) = timeout {
            query.push(("timeout", t));
        }

        let resp = self
            .http
            .post(self.url(&format!("/sprites/{sprite}/exec/{session_id}/kill")))
            .header("Authorization", self.auth())
            .query(&query)
            .send()
            .await?;

        Self::check(resp, "kill exec session")
            .await?
            .text()
            .await
            .map_err(Error::from)
    }

    // ── Checkpoints ─────────────────────────────────────────────────

    /// Create a checkpoint. Returns the raw NDJSON stream body.
    /// Callers can parse lines as [`StreamEvent`].
    pub async fn create_checkpoint(
        &self,
        sprite: &str,
        req: &CreateCheckpointRequest,
    ) -> Result<String> {
        let resp = self
            .http
            .post(self.url(&format!("/sprites/{sprite}/checkpoint")))
            .header("Authorization", self.auth())
            .json(req)
            .send()
            .await?;

        Self::check(resp, "create checkpoint")
            .await?
            .text()
            .await
            .map_err(Error::from)
    }

    pub async fn list_checkpoints(&self, sprite: &str) -> Result<Vec<Checkpoint>> {
        let resp = self
            .http
            .get(self.url(&format!("/sprites/{sprite}/checkpoints")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "list checkpoints")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn get_checkpoint(&self, sprite: &str, checkpoint_id: &str) -> Result<Checkpoint> {
        let resp = self
            .http
            .get(self.url(&format!("/sprites/{sprite}/checkpoints/{checkpoint_id}")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "get checkpoint")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    /// Restore a checkpoint. Returns the raw NDJSON stream body.
    /// Callers can parse lines as [`StreamEvent`].
    pub async fn restore_checkpoint(&self, sprite: &str, checkpoint_id: &str) -> Result<String> {
        let resp = self
            .http
            .post(self.url(&format!(
                "/sprites/{sprite}/checkpoints/{checkpoint_id}/restore"
            )))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "restore checkpoint")
            .await?
            .text()
            .await
            .map_err(Error::from)
    }

    // ── Network Policy ──────────────────────────────────────────────

    pub async fn get_network_policy(&self, sprite: &str) -> Result<NetworkPolicy> {
        let resp = self
            .http
            .get(self.url(&format!("/sprites/{sprite}/policy/network")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "get network policy")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn set_network_policy(
        &self,
        sprite: &str,
        policy: &NetworkPolicy,
    ) -> Result<NetworkPolicy> {
        let resp = self
            .http
            .post(self.url(&format!("/sprites/{sprite}/policy/network")))
            .header("Authorization", self.auth())
            .json(policy)
            .send()
            .await?;

        Self::check(resp, "set network policy")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    // ── Services ────────────────────────────────────────────────────

    pub async fn list_services(&self, sprite: &str) -> Result<Vec<Service>> {
        let resp = self
            .http
            .get(self.url(&format!("/sprites/{sprite}/services")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "list services")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn get_service(&self, sprite: &str, service: &str) -> Result<Service> {
        let resp = self
            .http
            .get(self.url(&format!("/sprites/{sprite}/services/{service}")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "get service")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    /// Create or update a service. Uses PUT.
    pub async fn create_service(
        &self,
        sprite: &str,
        service_name: &str,
        req: &CreateServiceRequest,
    ) -> Result<Service> {
        let resp = self
            .http
            .put(self.url(&format!("/sprites/{sprite}/services/{service_name}")))
            .header("Authorization", self.auth())
            .json(req)
            .send()
            .await?;

        Self::check(resp, "create service")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    /// Start a service. Returns the raw NDJSON stream body.
    pub async fn start_service(&self, sprite: &str, service: &str) -> Result<String> {
        let resp = self
            .http
            .post(self.url(&format!("/sprites/{sprite}/services/{service}/start")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "start service")
            .await?
            .text()
            .await
            .map_err(Error::from)
    }

    /// Stop a service. Returns the raw NDJSON stream body.
    pub async fn stop_service(
        &self,
        sprite: &str,
        service: &str,
        timeout: Option<&str>,
    ) -> Result<String> {
        let mut query: Vec<(&str, &str)> = Vec::new();
        if let Some(t) = timeout {
            query.push(("timeout", t));
        }

        let resp = self
            .http
            .post(self.url(&format!("/sprites/{sprite}/services/{service}/stop")))
            .header("Authorization", self.auth())
            .query(&query)
            .send()
            .await?;

        Self::check(resp, "stop service")
            .await?
            .text()
            .await
            .map_err(Error::from)
    }

    /// Get service logs. Returns the raw NDJSON stream body.
    pub async fn get_service_logs(
        &self,
        sprite: &str,
        service: &str,
        lines: Option<u32>,
    ) -> Result<String> {
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(n) = lines {
            query.push(("lines", n.to_string()));
        }

        let resp = self
            .http
            .get(self.url(&format!("/sprites/{sprite}/services/{service}/logs")))
            .header("Authorization", self.auth())
            .query(&query)
            .send()
            .await?;

        Self::check(resp, "get service logs")
            .await?
            .text()
            .await
            .map_err(Error::from)
    }
}
