//! Typed Rust client for the Fly.io Machines API.
//!
//! Covers the subset needed for managing agent VMs:
//! machines (create, get, start, stop, delete).

mod types;

pub use types::*;

const BASE_URL: &str = "https://api.machines.dev/v1";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("fly api request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("fly api {endpoint} returned {status}: {body}")]
    Api {
        endpoint: &'static str,
        status: reqwest::StatusCode,
        body: String,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

/// Client for the Fly.io Machines REST API.
#[derive(Clone)]
pub struct FlyClient {
    token: String,
    app: String,
    http: reqwest::Client,
}

impl FlyClient {
    pub fn new(token: impl Into<String>, app: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            app: app.into(),
            http: reqwest::Client::new(),
        }
    }

    pub fn app(&self) -> &str {
        &self.app
    }

    fn url(&self, path: &str) -> String {
        format!("{BASE_URL}/apps/{}{path}", self.app)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    async fn check(resp: reqwest::Response, endpoint: &'static str) -> Result<reqwest::Response> {
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api { endpoint, status, body });
        }
        Ok(resp)
    }

    /// Like `check` but also treats 404 as success (for delete idempotency).
    async fn check_allow_404(resp: reqwest::Response, endpoint: &'static str) -> Result<reqwest::Response> {
        let status = resp.status();
        if !status.is_success() && status.as_u16() != 404 {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api { endpoint, status, body });
        }
        Ok(resp)
    }

    // ── Machines ─────────────────────────────────────────────────────

    pub async fn create_machine(&self, req: &CreateMachineRequest) -> Result<Machine> {
        let resp = self
            .http
            .post(self.url("/machines"))
            .header("Authorization", self.auth())
            .json(req)
            .send()
            .await?;

        Self::check(resp, "create machine")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn get_machine(&self, machine_id: &str) -> Result<Machine> {
        let resp = self
            .http
            .get(self.url(&format!("/machines/{machine_id}")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "get machine")
            .await?
            .json()
            .await
            .map_err(Error::from)
    }

    pub async fn start_machine(&self, machine_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/machines/{machine_id}/start")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "start machine").await?;
        Ok(())
    }

    pub async fn stop_machine(&self, machine_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(self.url(&format!("/machines/{machine_id}/stop")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check(resp, "stop machine").await?;
        Ok(())
    }

    pub async fn delete_machine(&self, machine_id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(self.url(&format!("/machines/{machine_id}")))
            .header("Authorization", self.auth())
            .send()
            .await?;

        Self::check_allow_404(resp, "delete machine").await?;
        Ok(())
    }
}
