use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Machine types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct CreateMachineRequest {
    pub name: String,
    pub region: String,
    pub config: MachineConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct MachineConfig {
    pub image: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,
    pub guest: GuestConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mounts: Option<Vec<MachineMount>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<MachineFile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_destroy: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GuestConfig {
    pub cpus: u32,
    pub cpu_kind: String,
    pub memory_mb: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct MachineMount {
    pub volume: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MachineFile {
    pub guest_path: String,
    pub raw_value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub state: String,
    pub region: String,
    pub private_ip: Option<String>,
}
