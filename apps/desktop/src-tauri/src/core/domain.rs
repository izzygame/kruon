use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterKind {
    Codex,
    Claude,
}

impl AdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Planning,
    Running,
    WaitingApproval,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
    ForcedStopRequired,
    Uncertain,
}

impl RunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Uncertain
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalState {
    Completed,
    Failed,
    Cancelled,
    ForcedStopRequired,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventPhase {
    Setup,
    Planning,
    Running,
    ToolCall,
    WaitingApproval,
    ApprovalDecision,
    Artifact,
    Cancelling,
    Terminal,
    Degraded,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: u32,
    pub event_id: String,
    pub run_id: String,
    pub sequence: u64,
    pub event_type: String,
    pub phase: EventPhase,
    pub occurred_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_state: Option<TerminalState>,
    pub payload: Value,
}

impl EventEnvelope {
    pub fn new(
        run_id: impl Into<String>,
        sequence: u64,
        event_type: impl Into<String>,
        phase: EventPhase,
        terminal_state: Option<TerminalState>,
        payload: Value,
    ) -> Self {
        Self {
            schema_version: EVENT_SCHEMA_VERSION,
            event_id: uuid::Uuid::new_v4().to_string(),
            run_id: run_id.into(),
            sequence,
            event_type: event_type.into(),
            phase,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            terminal_state,
            payload,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRunRequest {
    pub adapter: AdapterKind,
    pub workspace_root: String,
    pub working_directory: String,
    pub prompt: String,
    pub timeout_ms: Option<u64>,
    pub policy_id: Option<String>,
}

impl StartRunRequest {
    pub fn prompt_hash(&self) -> String {
        format!("{:x}", Sha256::digest(self.prompt.as_bytes()))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSnapshot {
    pub run_id: String,
    pub adapter: AdapterKind,
    pub workspace_root: String,
    pub working_directory: String,
    pub policy_id: Option<String>,
    pub status: RunStatus,
    pub terminal_state: Option<TerminalState>,
    pub created_at: String,
    pub updated_at: String,
    pub last_sequence: u64,
    pub prompt_hash: String,
    pub pid: Option<u32>,
    pub pgid: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    pub run: RunSnapshot,
    pub events: Vec<EventEnvelope>,
}
