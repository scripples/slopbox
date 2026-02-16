use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Opaque provider-side VPS identifier (e.g. Fly Machine ID or Hetzner Server ID).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VpsId(pub String);

/// Specification for creating a VPS.
/// Region is not included — the provider uses its own configured region.
#[derive(Debug, Clone)]
pub struct VpsSpec {
    pub name: String,
    pub image: String,
    pub cpu_millicores: i32,
    pub memory_mb: i32,
    pub env: HashMap<String, String>,
    pub files: Vec<FileMount>,
}

/// A file to inject into the VPS.
#[derive(Debug, Clone)]
pub struct FileMount {
    pub guest_path: String,
    pub raw_value: String,
}

/// VPS status and metadata returned from the provider.
#[derive(Debug, Clone)]
pub struct VpsInfo {
    pub id: VpsId,
    pub state: VpsState,
    pub address: Option<String>,
}

/// Provider-reported VPS state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpsState {
    Starting,
    Running,
    Stopped,
    Destroyed,
    Unknown,
}
