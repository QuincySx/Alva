// INPUT:  gpui (Rgba, rgba)
// OUTPUT: pub struct AgentStatus, pub enum AgentStatusKind
// POS:    Defines agent runtime status types with color-coded indicator and label helpers.
use gpui::Rgba;

#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub session_id: String,
    pub kind: AgentStatusKind,
    pub detail: Option<String>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatusKind {
    Idle,
    Running,
    WaitingHitl,
    Error,
    Offline,
}

impl AgentStatusKind {
    /// Status indicator color (RGBA).
    pub fn color(&self) -> Rgba {
        match self {
            Self::Idle => gpui::rgba(0x6B7280FF),        // gray
            Self::Running => gpui::rgba(0x10B981FF),     // green
            Self::WaitingHitl => gpui::rgba(0xF59E0BFF), // yellow
            Self::Error => gpui::rgba(0xEF4444FF),       // red
            Self::Offline => gpui::rgba(0x374151FF),     // dark gray
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Running => "Running",
            Self::WaitingHitl => "Waiting",
            Self::Error => "Error",
            Self::Offline => "Offline",
        }
    }
}
