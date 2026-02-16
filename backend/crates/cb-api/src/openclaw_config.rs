use cb_infra::types::FileMount;
use serde::Serialize;
use uuid::Uuid;

/// Parameters for building an OpenClaw config.
pub struct ConfigParams {
    pub agent_id: Uuid,
    pub model: Option<String>,
    pub tools_deny: Option<Vec<String>>,
}

// ── Config structs ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct OpenClawConfig {
    pub agents: AgentsConfig,
    pub tools: ToolsConfig,
    pub gateway: GatewayConfig,
    pub hooks: HooksConfig,
}

#[derive(Debug, Serialize)]
pub struct AgentsConfig {
    pub defaults: AgentDefaults,
}

#[derive(Debug, Serialize)]
pub struct AgentDefaults {
    pub workspace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub sandbox: SandboxConfig,
}

#[derive(Debug, Serialize)]
pub struct SandboxConfig {
    pub mode: String,
    pub scope: String,
    #[serde(rename = "workspaceAccess")]
    pub workspace_access: String,
    pub docker: DockerConfig,
}

#[derive(Debug, Serialize)]
pub struct DockerConfig {
    pub network: String,
    pub env: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ToolsConfig {
    pub profile: String,
    pub deny: Vec<String>,
    pub elevated: ElevatedConfig,
}

#[derive(Debug, Serialize)]
pub struct ElevatedConfig {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct GatewayConfig {
    pub bind: String,
    pub auth: GatewayAuthConfig,
    pub bonjour: bool,
    #[serde(rename = "controlUi")]
    pub control_ui: ControlUiConfig,
}

#[derive(Debug, Serialize)]
pub struct GatewayAuthConfig {
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub struct ControlUiConfig {
    #[serde(rename = "basePath")]
    pub base_path: String,
}

#[derive(Debug, Serialize)]
pub struct HooksConfig {
    pub enabled: bool,
}

// ── Builders ─────────────────────────────────────────────────────────

/// Build a locked-down OpenClaw config.
pub fn build_openclaw_config(params: &ConfigParams) -> OpenClawConfig {
    let deny = params.tools_deny.clone().unwrap_or_else(|| {
        vec!["gateway".into(), "nodes".into()]
    });

    OpenClawConfig {
        agents: AgentsConfig {
            defaults: AgentDefaults {
                workspace: "~/.openclaw/workspace".into(),
                model: params.model.clone(),
                sandbox: SandboxConfig {
                    mode: "all".into(),
                    scope: "agent".into(),
                    workspace_access: "readwrite".into(),
                    docker: DockerConfig {
                        network: "none".into(),
                        env: serde_json::Map::new(),
                    },
                },
            },
        },
        tools: ToolsConfig {
            profile: "default".into(),
            deny,
            elevated: ElevatedConfig { enabled: false },
        },
        gateway: GatewayConfig {
            bind: "0.0.0.0:18789".into(),
            auth: GatewayAuthConfig {
                mode: "token".into(),
            },
            bonjour: false,
            control_ui: ControlUiConfig {
                base_path: format!("/agents/{}/gateway", params.agent_id),
            },
        },
        hooks: HooksConfig { enabled: false },
    }
}

/// Render an OpenClaw config to pretty-printed JSON.
pub fn render_openclaw_config(config: &OpenClawConfig) -> String {
    serde_json::to_string_pretty(config).expect("OpenClawConfig is always serializable")
}

/// Build workspace files to inject at provision time.
pub fn build_workspace_files(agent_name: &str) -> Vec<FileMount> {
    let base = "/root/.openclaw/workspace";

    vec![
        FileMount {
            guest_path: format!("{base}/IDENTITY.md"),
            raw_value: format!("# Identity\n\nYou are {agent_name}, a Cludbox agent.\n"),
        },
        FileMount {
            guest_path: format!("{base}/SOUL.md"),
            raw_value: "# Soul\n\nYou are a helpful assistant.\n".into(),
        },
        FileMount {
            guest_path: format!("{base}/AGENTS.md"),
            raw_value: "# Agents\n\nNo sub-agents configured.\n".into(),
        },
    ]
}
